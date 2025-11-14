# Hardlink-Based Sysroot Assembly for Bitzel

## The Problem with Symlinks

You correctly identified the critical issue with my initial symlink approach:

**Problem**: When multiple dependencies contribute files to the same directory:
```
glibc:do_install provides:  /usr/lib/libc.so
libz:do_install provides:   /usr/lib/libz.so
ncurses:do_install provides: /usr/lib/libncurses.so
```

With symlinks, you can't have:
```
recipe-sysroot/usr/lib/ -> artifacts/glibc/.../usr/lib/    # Only gets glibc files!
```

You need ALL files from ALL dependencies in the same directory.

## How BitBake Actually Does It: Hardlinks

### What are Hardlinks?

**Hardlink**: Multiple directory entries pointing to the same inode (physical file on disk).

```
artifacts/glibc/do_install-<sig>/sysroot/usr/lib/libc.so (inode 12345)
artifacts/libz/do_install-<sig>/sysroot/usr/lib/libz.so  (inode 67890)

↓ hardlink (not copy!)

recipe-sysroot/usr/lib/libc.so      (inode 12345) ← same inode!
recipe-sysroot/usr/lib/libz.so      (inode 67890) ← same inode!
```

**Benefits**:
- ✅ Multiple files in same directory
- ✅ No disk space duplication (same inode)
- ✅ Fast (no copying)
- ✅ Atomic (filesystem operation)

**Limitation**:
- ❌ Only works on same filesystem
- ❌ Falls back to copy if crossing filesystem boundaries

### BitBake's copyhardlinktree() Implementation

From `meta/lib/oe/path.py`:

```python
def copyhardlinktree(src, dst):
    """
    Make the hard link when possible, otherwise copy.
    """
    # Check if source and destination are on same filesystem
    if (os.stat(src).st_dev == os.stat(dst).st_dev):
        # Same filesystem: can use hardlinks

        # First, copy directory structure using tar
        # (avoids race condition with multiple writers)
        cmd = 'tar -cf - -C %s -p . | tar -xf - -C %s' % (src, dst)
        subprocess.check_output(cmd, shell=True)

        # Then, hardlink files with cp -afl
        # -a: archive mode (preserve permissions, timestamps, ownership)
        # -f: force
        # -l: CREATE HARDLINKS instead of copying
        # --preserve=xattr: maintain extended attributes
        cmd = 'cp -afl --preserve=xattr %s %s' % (src, dst)
        subprocess.check_output(cmd, shell=True)
    else:
        # Different filesystem: fall back to standard copy
        copytree(src, dst)
```

**Key insight**: `cp -afl` creates hardlinks instead of copying file data.

### Assembling recipe-sysroot from Multiple Dependencies

**Example**: `busybox:do_compile` depends on:
- `glibc:do_install` (provides libc, headers)
- `libz:do_install` (provides libz)
- `ncurses:do_install` (provides libncurses)

**Process**:

```python
# For busybox:do_compile sandbox
sandbox_sysroot = sandbox_root / "work/recipe-sysroot"

# Iterate through each dependency
for dep in ["glibc:do_install", "libz:do_install", "ncurses:do_install"]:
    dep_sysroot = artifacts / f"{dep}-<sig>" / "sysroot"

    # Hardlink all files from dependency sysroot into recipe-sysroot
    copyhardlinktree(dep_sysroot, sandbox_sysroot)
```

**Result**:

```
sandbox/busybox-do_compile-<uuid>/work/recipe-sysroot/
  usr/
    include/
      stdio.h          (inode 11111) ← hardlink from glibc
      zlib.h           (inode 22222) ← hardlink from libz
      ncurses.h        (inode 33333) ← hardlink from ncurses
    lib/
      libc.so          (inode 44444) ← hardlink from glibc
      libz.so          (inode 55555) ← hardlink from libz
      libncurses.so    (inode 66666) ← hardlink from ncurses
```

All files from all dependencies are now in the same directory structure!

## File Conflict Detection

### The Problem

What if TWO dependencies try to provide the SAME file?

```
glibc-2.38:do_install provides:  /usr/lib/ld.so
glibc-2.37:do_install provides:  /usr/lib/ld.so  # CONFLICT!
```

This is a **serious error** - can't have two different versions of the same file.

### BitBake's Solution: Manifest Tracking

Each task's sysroot contribution is tracked in a **manifest file**:

```
artifacts/glibc/do_install-<sig>/manifest.txt:
  /usr/lib/libc.so.6
  /usr/lib/ld.so
  /usr/include/stdio.h
  /usr/include/stdlib.h

artifacts/glibc-old/do_install-<sig>/manifest.txt:
  /usr/lib/libc.so.6    # CONFLICT with glibc!
  /usr/lib/ld.so        # CONFLICT with glibc!
```

When assembling sysroot, check for conflicts:

```python
def copyhardlinktree_with_conflict_detection(
    src_dir: Path,
    dst_dir: Path,
    manifest: Path,
    previous_manifests: List[Path],
    whitelist: List[str]
):
    """
    Hardlink files from src to dst, detecting conflicts.
    """
    # Load this dependency's manifest (files it provides)
    new_files = set(read_manifest(manifest))

    # Load all previously staged files
    staged_files = {}
    for prev_manifest in previous_manifests:
        for file in read_manifest(prev_manifest):
            staged_files[file] = prev_manifest

    # Check for conflicts
    conflicts = []
    for file in new_files:
        # Skip whitelisted paths (e.g., /usr/share/licenses/)
        if any(file.startswith(w) for w in whitelist):
            continue

        # Check if file already staged by different recipe
        if file in staged_files:
            conflicts.append({
                'file': file,
                'existing_from': staged_files[file],
                'new_from': manifest
            })

    if conflicts:
        raise SysrootConflictError(conflicts)

    # No conflicts - proceed with hardlinking
    copyhardlinktree(src_dir, dst_dir)

    # Track these files for future conflict detection
    previous_manifests.append(manifest)
```

### SSTATE_DUPWHITELIST

Some duplicates are harmless (e.g., identical license files):

```python
DUPWHITELIST = [
    '/usr/share/licenses/',
    '/usr/share/doc/',
    '/etc/sgml/',  # SGML catalog files
]
```

Files under these paths are allowed to be duplicated.

### Error Message Example

```
ERROR: busybox-1.36.1-r0 do_prepare_recipe_sysroot:
The recipe busybox is trying to install files into a shared area when those files already exist.

Files that conflict:
  /usr/lib/libc.so.6 (from glibc-2.38)
  /usr/lib/ld.so (from glibc-2.38)

These files were previously provided by:
  glibc-old-2.37

Please either:
1. Fix the recipes to not provide conflicting files
2. Add to SSTATE_DUPWHITELIST if conflict is harmless
```

## Rust Implementation for Bitzel

### Data Structures

```rust
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs as unix_fs;

/// Manifest tracking which files a task provides
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysrootManifest {
    /// Recipe and task that provided these files
    pub recipe: String,
    pub task: String,
    pub signature: String,

    /// List of files provided (relative paths)
    pub files: Vec<PathBuf>,
}

/// Conflict detected during sysroot assembly
#[derive(Debug, Clone)]
pub struct SysrootConflict {
    pub file: PathBuf,
    pub existing_from: String,  // "glibc-2.38:do_install-<sig>"
    pub new_from: String,        // "glibc-2.37:do_install-<sig>"
}

#[derive(Debug)]
pub struct SysrootConflictError {
    pub conflicts: Vec<SysrootConflict>,
}
```

### Hardlink Tree Builder

```rust
pub struct HardlinkTreeBuilder {
    /// Whitelist for harmless duplicates
    dup_whitelist: Vec<PathBuf>,
}

impl HardlinkTreeBuilder {
    pub fn new() -> Self {
        Self {
            dup_whitelist: vec![
                PathBuf::from("/usr/share/licenses"),
                PathBuf::from("/usr/share/doc"),
                PathBuf::from("/etc/sgml"),
            ],
        }
    }

    /// Check if source and destination are on same filesystem
    fn same_filesystem(src: &Path, dst: &Path) -> std::io::Result<bool> {
        use std::os::unix::fs::MetadataExt;

        let src_dev = fs::metadata(src)?.dev();
        let dst_dev = fs::metadata(dst)?.dev();

        Ok(src_dev == dst_dev)
    }

    /// Hardlink tree from src to dst (like BitBake's copyhardlinktree)
    pub fn copyhardlinktree(
        &self,
        src: &Path,
        dst: &Path,
    ) -> std::io::Result<()> {
        // Create destination directory
        fs::create_dir_all(dst)?;

        if Self::same_filesystem(src, dst)? {
            // Same filesystem: use hardlinks
            self.copyhardlinktree_hardlink(src, dst)?;
        } else {
            // Different filesystem: fall back to copy
            self.copyhardlinktree_copy(src, dst)?;
        }

        Ok(())
    }

    /// Use cp -afl to create hardlinks
    fn copyhardlinktree_hardlink(
        &self,
        src: &Path,
        dst: &Path,
    ) -> std::io::Result<()> {
        use std::process::Command;

        // First, create directory structure
        // (using tar to avoid race conditions)
        let tar_cmd = format!(
            "tar -cf - -C {} -p . | tar -xf - -C {}",
            src.display(),
            dst.display()
        );

        Command::new("sh")
            .arg("-c")
            .arg(&tar_cmd)
            .output()?;

        // Then, hardlink files with cp -afl
        let cp_cmd = vec![
            "cp",
            "-afl",              // archive + force + hardlink
            "--preserve=xattr",  // preserve extended attributes
            src.to_str().unwrap(),
            dst.to_str().unwrap(),
        ];

        let output = Command::new("cp")
            .args(&cp_cmd[1..])
            .output()?;

        if !output.status.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("cp -afl failed: {}", String::from_utf8_lossy(&output.stderr))
            ));
        }

        Ok(())
    }

    /// Fall back to recursive copy
    fn copyhardlinktree_copy(
        &self,
        src: &Path,
        dst: &Path,
    ) -> std::io::Result<()> {
        use walkdir::WalkDir;

        for entry in WalkDir::new(src) {
            let entry = entry?;
            let rel_path = entry.path().strip_prefix(src).unwrap();
            let dst_path = dst.join(rel_path);

            if entry.file_type().is_dir() {
                fs::create_dir_all(&dst_path)?;
            } else {
                fs::copy(entry.path(), &dst_path)?;
            }
        }

        Ok(())
    }
}
```

### Sysroot Assembler with Conflict Detection

```rust
pub struct SysrootAssembler {
    hardlink_builder: HardlinkTreeBuilder,
    dup_whitelist: Vec<PathBuf>,
}

impl SysrootAssembler {
    pub fn new() -> Self {
        Self {
            hardlink_builder: HardlinkTreeBuilder::new(),
            dup_whitelist: vec![
                PathBuf::from("/usr/share/licenses"),
                PathBuf::from("/usr/share/doc"),
            ],
        }
    }

    /// Assemble recipe-sysroot from multiple dependency sysroots
    pub fn assemble_sysroot(
        &self,
        dependencies: &[TaskDependency],
        artifact_cache: &Path,
        output_sysroot: &Path,
    ) -> Result<(), SysrootConflictError> {
        // Track which files have been staged and by whom
        let mut staged_files: HashMap<PathBuf, String> = HashMap::new();
        let mut conflicts = Vec::new();

        for dep in dependencies {
            // Load dependency's sysroot and manifest
            let dep_artifact = artifact_cache.join(format!(
                "{}/{}-{}",
                dep.recipe,
                dep.task,
                dep.signature
            ));

            let dep_sysroot = dep_artifact.join("sysroot");
            let dep_manifest_path = dep_artifact.join("manifest.json");

            // Load manifest
            let manifest: SysrootManifest = serde_json::from_str(
                &fs::read_to_string(&dep_manifest_path)?
            )?;

            // Check for conflicts
            for file in &manifest.files {
                // Skip whitelisted paths
                if self.is_whitelisted(file) {
                    continue;
                }

                // Check if already staged
                if let Some(existing_from) = staged_files.get(file) {
                    conflicts.push(SysrootConflict {
                        file: file.clone(),
                        existing_from: existing_from.clone(),
                        new_from: format!("{}:{}-{}", dep.recipe, dep.task, dep.signature),
                    });
                }
            }

            // If conflicts found, don't continue
            if !conflicts.is_empty() {
                return Err(SysrootConflictError { conflicts });
            }

            // No conflicts - hardlink files into sysroot
            self.hardlink_builder.copyhardlinktree(
                &dep_sysroot,
                output_sysroot,
            )?;

            // Track staged files
            for file in &manifest.files {
                staged_files.insert(
                    file.clone(),
                    format!("{}:{}-{}", dep.recipe, dep.task, dep.signature)
                );
            }
        }

        Ok(())
    }

    fn is_whitelisted(&self, file: &Path) -> bool {
        self.dup_whitelist.iter().any(|w| file.starts_with(w))
    }
}
```

### Manifest Generation

When a task completes, generate manifest of files it provides:

```rust
pub fn generate_sysroot_manifest(
    sysroot_dir: &Path,
    recipe: &str,
    task: &str,
    signature: &str,
) -> std::io::Result<SysrootManifest> {
    use walkdir::WalkDir;

    let mut files = Vec::new();

    for entry in WalkDir::new(sysroot_dir) {
        let entry = entry?;

        if entry.file_type().is_file() {
            // Store relative path
            let rel_path = entry.path().strip_prefix(sysroot_dir).unwrap();
            files.push(rel_path.to_path_buf());
        }
    }

    // Sort for determinism
    files.sort();

    Ok(SysrootManifest {
        recipe: recipe.to_string(),
        task: task.to_string(),
        signature: signature.to_string(),
        files,
    })
}
```

## Complete Example: busybox:do_compile

### Step 1: Dependencies Complete

After `glibc:do_install`, `libz:do_install`, `ncurses:do_install` complete:

```
artifacts/
  glibc/
    do_install-abc123/
      sysroot/
        usr/lib/libc.so.6
        usr/include/stdio.h
      manifest.json:
        {"files": ["/usr/lib/libc.so.6", "/usr/include/stdio.h"], ...}

  libz/
    do_install-def456/
      sysroot/
        usr/lib/libz.so.1
        usr/include/zlib.h
      manifest.json:
        {"files": ["/usr/lib/libz.so.1", "/usr/include/zlib.h"], ...}
```

### Step 2: Assemble busybox Sysroot

```rust
let dependencies = vec![
    TaskDependency {
        recipe: "glibc".to_string(),
        task: "do_install".to_string(),
        signature: "abc123".to_string(),
    },
    TaskDependency {
        recipe: "libz".to_string(),
        task: "do_install".to_string(),
        signature: "def456".to_string(),
    },
];

let assembler = SysrootAssembler::new();
assembler.assemble_sysroot(
    &dependencies,
    &artifacts_dir,
    &sandbox_sysroot,
)?;
```

**Result** (all hardlinks to artifact cache):

```
sandbox/busybox-do_compile-uuid/work/recipe-sysroot/
  usr/
    lib/
      libc.so.6     → hardlink to artifacts/glibc/.../libc.so.6
      libz.so.1     → hardlink to artifacts/libz/.../libz.so.1
    include/
      stdio.h       → hardlink to artifacts/glibc/.../stdio.h
      zlib.h        → hardlink to artifacts/libz/.../zlib.h
```

### Step 3: Execute Task

Compiler finds headers and libraries via hardlinks!

```bash
cd /sandbox/busybox-do_compile-uuid/work
gcc -c busybox.c -I/work/recipe-sysroot/usr/include -L/work/recipe-sysroot/usr/lib
```

## Performance Considerations

### Hardlink vs Copy vs Symlink

| Method | Disk Usage | Speed | Multi-Source | Cross-FS |
|--------|------------|-------|--------------|----------|
| **Hardlink** | No duplication | Fast | ✅ Yes | ❌ No |
| **Symlink** | No duplication | Fast | ❌ No | ✅ Yes |
| **Copy** | Full duplication | Slow | ✅ Yes | ✅ Yes |

**Hardlink is best** when:
- Artifact cache and sandbox are on same filesystem (common)
- Multiple dependencies provide different files to same directory

**Fallback to copy** when:
- Artifact cache on different filesystem (e.g., network mount)

### BitBake's Performance Note

From BitBake mailing list:
> "For performance, it's always been envisaged that the recipe specific
> sysroots would consist of hardlinks to minimise disk usage and IO and
> to try and minimise the performance impact of this change."

Hardlinks are **critical** for performance with recipe-specific sysroots.

## Summary

✅ **Use hardlinks** (not symlinks) for sysroot assembly
✅ **Multiple dependencies** can provide files to same directory
✅ **Detect conflicts** when same file from different sources
✅ **Use manifest tracking** to identify conflict sources
✅ **Whitelist harmless duplicates** (licenses, docs)
✅ **Fall back to copy** when crossing filesystem boundaries

This aligns bitzel with BitBake's proven approach while maintaining Bazel-like sandboxing.
