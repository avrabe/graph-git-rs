# Overlay Mounts vs Hardlinks for Sysroot Assembly

## The Question

Can we use **overlay mounts** instead of hardlinks for assembling recipe-sysroot from multiple dependencies?

**TL;DR**: YES! Overlay mounts are **superior** to hardlinks in almost every way.

---

## Current Plan: Hardlinks (from HARDLINK_SYSROOT_DESIGN.md)

### How it works:
```bash
# Task depends on glibc and libz
# Both provide files to /usr/lib/

# Step 1: Hardlink glibc sysroot
cp -afl artifacts/glibc/do_populate_sysroot-abc123/sysroot/ recipe-sysroot/

# Step 2: Hardlink libz sysroot
cp -afl artifacts/libz/do_populate_sysroot-def456/sysroot/ recipe-sysroot/

# Result:
recipe-sysroot/
  usr/lib/libc.so    (hardlink to cache)
  usr/lib/libz.so    (hardlink to cache)
```

### Issues:
1. ❌ Requires disk space (inodes, even if data blocks shared)
2. ❌ Requires tree creation time (cp -afl for each dep)
3. ❌ Manual file conflict detection needed
4. ❌ Cleanup required after task
5. ❌ Can accidentally modify cache if permissions wrong

---

## Better Approach: OverlayFS

### What is OverlayFS?

**OverlayFS** is a Linux kernel filesystem that layers multiple directories into a single merged view.

- **Lower directories**: Read-only layers (dependencies)
- **Upper directory**: Writable layer (optional, for modifications)
- **Merged view**: What the process sees

**Used by**: Docker, Podman, LXC, systemd-nspawn

### How it works for sysroot assembly:

```bash
# Task depends on glibc and libz
# Mount overlay inside namespace:

mount -t overlay overlay \
  -o lowerdir=/cache/libz/sysroot:/cache/glibc/sysroot \
  -o upperdir=/tmp/upper \
  -o workdir=/tmp/work \
  /work/recipe-sysroot

# Result: Task sees merged view at /work/recipe-sysroot
/work/recipe-sysroot/
  usr/lib/libc.so    (from glibc layer - read-only)
  usr/lib/libz.so    (from libz layer - read-only)
```

**Key insight**: The merge happens **in the kernel**, not on disk!

---

## Comparison

| Feature | Hardlinks | OverlayFS |
|---------|-----------|-----------|
| **Disk space** | Uses inodes (1 per file) | Zero (pure mount) |
| **Setup time** | O(n) files to hardlink | O(1) mount operation |
| **Cleanup** | Must delete hardlinks | Unmount (instant) |
| **File conflicts** | Manual detection needed | Automatic (first layer wins) |
| **Read-only cache** | Requires permission setup | Guaranteed read-only |
| **Multiple deps** | Multiple cp -afl calls | Single mount (all layers) |
| **Kernel support** | Always available | Requires OverlayFS kernel module |
| **Speed** | Slower (tree copy) | **Instant** |
| **Safety** | Can corrupt cache | **Cannot write to cache** |

**Winner**: OverlayFS for almost everything!

---

## Implementation with hakoniwa

```rust
use hakoniwa::Sandbox;

pub fn execute_task_with_overlay_sysroot(
    task: &Task,
    dependencies: &[Dependency],
    artifact_cache: &Path,
    sandbox_dir: &Path,
) -> Result<()> {

    // 1. Collect dependency sysroot paths (from cache)
    let sysroot_layers: Vec<PathBuf> = dependencies
        .iter()
        .map(|dep| {
            artifact_cache
                .join(&dep.recipe)
                .join(format!("{}-{}", dep.task, dep.signature))
                .join("sysroot")
        })
        .collect();

    // 2. Build overlay lowerdir string
    // Format: "layer1:layer2:layer3" (last = highest priority)
    let lowerdir = sysroot_layers
        .iter()
        .rev()  // Reverse so first dep has lowest priority
        .map(|p| p.to_str().unwrap())
        .collect::<Vec<_>>()
        .join(":");

    // 3. Create sandbox with overlay mount
    Sandbox::new()
        // Mount sandbox work directory
        .mount("/work", sandbox_dir)

        // Create overlay mount for recipe-sysroot
        .overlay(
            "/work/recipe-sysroot",  // Target mount point
            &lowerdir,                // Read-only layers
            None,                     // No upper dir = fully read-only
        )

        // Create overlay mount for recipe-sysroot-native
        .overlay(
            "/work/recipe-sysroot-native",
            &build_native_lowerdir(dependencies)?,
            None,
        )

        // Isolation
        .unshare_user()
        .unshare_pid()
        .unshare_mount()
        .unshare_network()

        // Execute task
        .env("WORKDIR", "/work")
        .run(&["bash", "-c", &task.script])?;

    Ok(())
}
```

**That's it!** No hardlink tree creation, no cleanup, **instant setup**.

---

## Handling File Conflicts

### With Hardlinks:
```rust
// Must explicitly check before hardlinking
let mut file_owners: HashMap<PathBuf, String> = HashMap::new();

for dep in dependencies {
    let manifest = load_manifest(dep)?;
    for file in &manifest.files {
        if let Some(existing) = file_owners.get(file) {
            if !is_whitelisted(file) {
                return Err(format!("Conflict: {} provided by both {} and {}",
                    file, existing, dep.recipe));
            }
        }
        file_owners.insert(file.clone(), dep.recipe.clone());
    }
}
```

### With OverlayFS:
```rust
// OverlayFS handles automatically:
// - Last layer in lowerdir wins (highest priority)
// - No error, just uses first matching file

// Optional: Can still check manifests for explicit conflicts
let conflicts = detect_conflicts(dependencies)?;
if !conflicts.is_empty() {
    warn!("File conflicts detected (overlay will use layer order):");
    for conflict in conflicts {
        warn!("  {} from multiple deps: {:?}", conflict.file, conflict.providers);
    }
}
```

**Decision**:
- If conflict is in SSTATE_DUPWHITELIST → overlay handles it (fine)
- If conflict is NOT whitelisted → warn user, but let overlay handle it

**Layer order determines priority**:
```rust
// Higher priority dependencies should be LAST in lowerdir
let lowerdir = dependencies
    .sort_by_key(|d| d.priority)  // Sort by priority
    .iter()
    .map(|d| d.sysroot_path.to_str().unwrap())
    .collect::<Vec<_>>()
    .join(":");
```

---

## Advantages Beyond Performance

### 1. **Instant Teardown**
```rust
// After task completes:
// - Unmount overlay: instant
// - Delete sandbox_dir: just the build outputs

// With hardlinks:
// - Must rm -rf entire hardlink tree
// - Thousands of files to unlink
```

### 2. **Guaranteed Cache Integrity**
```rust
// OverlayFS with no upperdir = read-only
// Task CANNOT modify cache, even with bugs

// With hardlinks:
// - If permissions wrong, task can modify cache files
// - Hardlink means same inode = corruption spreads
```

### 3. **Easier Debugging**
```bash
# Can inspect overlay layers directly:
ls -la /cache/glibc/do_populate_sysroot-abc123/sysroot/
ls -la /cache/libz/do_populate_sysroot-def456/sysroot/

# With hardlinks: recipe-sysroot is a merged copy
# - Can't tell which file came from which dep
# - Have to check manifests
```

### 4. **Dynamic Layer Addition**
```rust
// Can add layers on the fly (remount)
// Useful for incremental builds

mount -t overlay overlay \
  -o remount,lowerdir=new_dep:old_deps \
  /work/recipe-sysroot
```

---

## Potential Issues and Solutions

### Issue 1: Kernel Support

**Problem**: Requires CONFIG_OVERLAY_FS kernel module

**Check**:
```bash
# Check if overlay supported:
cat /proc/filesystems | grep overlay
```

**Solution**:
- Most modern kernels have it (since Linux 3.18)
- If not available, fall back to hardlinks
- Can detect at runtime:

```rust
pub enum SysrootStrategy {
    Overlay,   // Preferred
    Hardlink,  // Fallback
}

impl SysrootStrategy {
    pub fn detect() -> Self {
        if overlay_supported() {
            SysrootStrategy::Overlay
        } else {
            SysrootStrategy::Hardlink
        }
    }
}

fn overlay_supported() -> bool {
    std::fs::read_to_string("/proc/filesystems")
        .map(|s| s.contains("overlay"))
        .unwrap_or(false)
}
```

### Issue 2: Nested Overlays

**Problem**: Can't nest overlay mounts (overlay on overlay)

**Impact**: Not relevant for our use case
- We mount overlay once per sandbox
- No nesting needed

### Issue 3: File Conflicts

**Problem**: Overlay doesn't error on conflicts, just uses first layer

**Solution**:
```rust
// Check manifests before creating overlay
pub fn validate_no_conflicts(
    dependencies: &[Dependency],
    whitelist: &[PathBuf],
) -> Result<(), ConflictError> {
    let mut file_providers: HashMap<PathBuf, Vec<String>> = HashMap::new();

    for dep in dependencies {
        let manifest = load_manifest(dep)?;
        for file in &manifest.files {
            file_providers
                .entry(file.clone())
                .or_insert_with(Vec::new)
                .push(dep.recipe.clone());
        }
    }

    let conflicts: Vec<_> = file_providers
        .iter()
        .filter(|(file, providers)| {
            providers.len() > 1 && !is_whitelisted(file, whitelist)
        })
        .collect();

    if !conflicts.is_empty() {
        return Err(ConflictError { conflicts });
    }

    Ok(())
}
```

### Issue 4: Whiteouts

**Problem**: Overlay uses "whiteout" files to hide lower layers

**Impact**: Not relevant - we only use read-only overlays (no upperdir)

---

## Complete Example

### Before (Hardlinks):
```rust
pub fn assemble_sysroot(deps: &[Dep], cache: &Path, output: &Path) -> Result<()> {
    // Create output directory
    fs::create_dir_all(output)?;

    // Hardlink each dependency (SLOW)
    for dep in deps {
        let dep_sysroot = cache.join(&dep.path).join("sysroot");

        // Use cp -afl (blocks on I/O)
        Command::new("cp")
            .args(&["-afl", dep_sysroot.to_str().unwrap(), output.to_str().unwrap()])
            .output()?;
    }

    // Later: cleanup (SLOW)
    fs::remove_dir_all(output)?;

    Ok(())
}
```

**Time**: O(n files) for setup + O(n files) for cleanup

### After (OverlayFS):
```rust
pub fn create_overlay_sysroot(deps: &[Dep], cache: &Path) -> String {
    // Just build lowerdir string (INSTANT)
    deps.iter()
        .map(|d| cache.join(&d.path).join("sysroot").to_str().unwrap())
        .collect::<Vec<_>>()
        .join(":")
}

// In sandbox:
Sandbox::new()
    .overlay("/work/recipe-sysroot", &lowerdir, None)  // INSTANT mount
    .run(&["bash", "-c", &script])?;

// Cleanup: INSTANT unmount (automatic on sandbox exit)
```

**Time**: O(1) for setup + O(1) for cleanup

---

## Implementation Strategy

### Phase 1: Add OverlayFS Support to hakoniwa
```rust
// Contribute to hakoniwa crate:
impl Sandbox {
    pub fn overlay(
        mut self,
        target: &str,
        lowerdir: &str,
        upperdir: Option<&str>,
    ) -> Self {
        // Store overlay spec
        self.overlays.push(OverlayMount {
            target: target.to_string(),
            lowerdir: lowerdir.to_string(),
            upperdir: upperdir.map(String::from),
        });
        self
    }
}

// During sandbox creation:
fn create_overlay_mounts(&self) -> Result<()> {
    for overlay in &self.overlays {
        let opts = if let Some(upper) = &overlay.upperdir {
            format!("lowerdir={},upperdir={},workdir=/tmp/overlay-work",
                overlay.lowerdir, upper)
        } else {
            format!("lowerdir={}", overlay.lowerdir)
        };

        // Mount overlay
        nix::mount::mount(
            Some("overlay"),
            &overlay.target,
            Some("overlay"),
            MsFlags::empty(),
            Some(opts.as_str()),
        )?;
    }
    Ok(())
}
```

### Phase 2: Use in Bitzel
```rust
pub fn execute_task(
    task: &Task,
    dependencies: &[Dependency],
    artifact_cache: &Path,
) -> Result<()> {
    // Build overlay lowerdirs for sysroots
    let target_sysroot_lowerdir = dependencies
        .iter()
        .filter(|d| d.task == "do_populate_sysroot")
        .map(|d| artifact_cache.join(&d.recipe).join(&d.signature_path).join("sysroot"))
        .map(|p| p.to_str().unwrap().to_string())
        .collect::<Vec<_>>()
        .join(":");

    let native_sysroot_lowerdir = dependencies
        .iter()
        .filter(|d| d.task == "do_populate_sysroot_native")
        .map(|d| artifact_cache.join(&d.recipe).join(&d.signature_path).join("sysroot"))
        .map(|p| p.to_str().unwrap().to_string())
        .collect::<Vec<_>>()
        .join(":");

    // Create sandbox with overlay mounts
    Sandbox::new()
        .mount("/work", sandbox_dir)

        // Overlay for target sysroot
        .overlay("/work/recipe-sysroot", &target_sysroot_lowerdir, None)

        // Overlay for native sysroot
        .overlay("/work/recipe-sysroot-native", &native_sysroot_lowerdir, None)

        // Isolation
        .unshare_user()
        .unshare_pid()
        .unshare_mount()

        .env("WORKDIR", "/work")
        .run(&["bash", "-c", &task.script])?;

    Ok(())
}
```

**Result**: Zero disk I/O for sysroot assembly!

---

## Benchmarks (Estimated)

### Setup Time for 100 Dependencies

| Method | Time | I/O Operations |
|--------|------|----------------|
| **Hardlinks** | ~5-10s | O(n files) hardlink syscalls |
| **OverlayFS** | ~10ms | 1 mount syscall |

**Speedup**: ~500-1000x faster!

### Disk Space for 100 Dependencies

| Method | Inodes Used | Disk Space |
|--------|-------------|------------|
| **Hardlinks** | ~10,000 | 0 bytes (data) + inode overhead |
| **OverlayFS** | 0 | 0 bytes |

### Cleanup Time

| Method | Time |
|--------|------|
| **Hardlinks** | ~2-5s (unlink all files) |
| **OverlayFS** | <1ms (unmount) |

---

## Conclusion

**OverlayFS is superior to hardlinks in every way**:

✅ **Instant setup** (1 mount vs thousands of hardlinks)
✅ **Zero disk space** (no inodes at all)
✅ **Instant cleanup** (unmount vs rm -rf)
✅ **Guaranteed read-only** (cannot corrupt cache)
✅ **Automatic merging** (kernel handles it)
✅ **Simpler code** (no cp -afl, no manifest checking)

**Only requirement**: Linux kernel with OverlayFS support (standard since 3.18)

**Fallback**: If overlay not available, use hardlinks

---

## Recommendation

1. **Implement overlay support in hakoniwa** (or use directly with nix crate)
2. **Use overlay as primary strategy** for sysroot assembly
3. **Keep hardlink code as fallback** for old kernels
4. **Detect at runtime** which to use

This aligns perfectly with the namespace sandboxing approach and eliminates a major source of complexity and I/O overhead!

**The user's insight is spot-on** - overlay mounts are the right solution here.
