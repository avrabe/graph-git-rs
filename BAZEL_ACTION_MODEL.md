# Bazel's Per-Action Sandbox Model and BitBake Task Execution

## How Bazel Actually Works: Per-Action Sandboxing

### 1. The Action Graph

Bazel builds are structured as a **bipartite, directed, acyclic graph**:
- **Nodes**: Actions (commands) and Artifacts (files)
- **Edges**: Action inputs → Action → Action outputs

Example:
```
[glibc.c] → [CppCompile] → [glibc.o] → [CppLink] → [libc.so]
                                                          ↓
[busybox.c] → [CppCompile] → [busybox.o] → [CppLink] → [busybox]
                                              ↑
                                         [libc.so] (input)
```

### 2. Permanent Execroot Structure

Bazel maintains a **permanent execroot** that persists across builds:

```
${output_base}/
  execroot/
    __main__/                           # Workspace root (symlink forest)
      src/                              # Symlinks to source files
        glibc/
          glibc.c -> /real/path/glibc.c
        busybox/
          busybox.c -> /real/path/busybox.c

      bazel-out/                        # All build outputs (PERMANENT)
        k8-fastbuild/                   # Configuration-specific outputs
          bin/                          # Compiled binaries
            glibc/
              libc.so                   # Output from CppLink(glibc)
            busybox/
              busybox                   # Output from CppLink(busybox)

          genfiles/                     # Generated source files

          testlogs/                     # Test execution logs
            busybox/
              test.log
              test.xml

      _tmp/
        actions/                        # Action stdout/stderr logs
          12345.out                     # stdout from action #12345
          12345.err                     # stderr from action #12345
```

**Key insight**: `bazel-out/` is **permanent** and **shared** across all actions. It's where outputs accumulate.

### 3. Temporary Sandbox Per Action

For **each action execution**, Bazel creates a **temporary sandbox**:

```
${output_base}/
  sandbox/
    linux-sandbox/
      <random-12345>/                   # TEMPORARY sandbox for CppCompile(glibc)
        execroot/
          __main__/
            src/
              glibc/
                glibc.c -> ../../../../../../../execroot/__main__/src/glibc/glibc.c

            bazel-out/
              k8-fastbuild/
                bin/                    # Symlinks to outputs from dependencies
                  # (empty - no dependencies for this action)

      <random-67890>/                   # TEMPORARY sandbox for CppLink(busybox)
        execroot/
          __main__/
            src/
              busybox/
                busybox.c -> ../../../../../../../execroot/__main__/src/busybox/busybox.c

            bazel-out/
              k8-fastbuild/
                bin/
                  glibc/
                    libc.so -> ../../../../../../../execroot/__main__/bazel-out/.../bin/glibc/libc.so
                  busybox/
                    busybox.o -> ../../../../../../../execroot/__main__/bazel-out/.../bin/busybox/busybox.o
```

### 4. Action Execution Workflow

For each action:

**A. Sandbox Creation**:
1. Create temporary directory: `${output_base}/sandbox/linux-sandbox/<random-id>/`
2. Create `execroot/__main__/` subdirectory structure
3. **Symlink input files**:
   - Source files from workspace
   - Output artifacts from **dependency actions** (from permanent `bazel-out/`)
4. **Create output directories** (initially empty)

**B. Action Execution**:
1. Set working directory to sandbox `execroot/__main__/`
2. Execute command with:
   - Inputs visible via symlinks
   - Outputs written to `bazel-out/<config>/bin/` (inside sandbox)
3. Capture stdout/stderr to `${output_base}/execroot/__main__/_tmp/actions/<action-id>.{out,err}`

**C. Output Collection**:
1. **Copy outputs** from sandbox to permanent execroot:
   ```
   sandbox/<id>/execroot/__main__/bazel-out/k8-fastbuild/bin/foo.o
     →
   execroot/__main__/bazel-out/k8-fastbuild/bin/foo.o
   ```
2. Update **action cache** (maps action key → output paths)

**D. Sandbox Cleanup**:
1. Delete temporary sandbox directory
2. Symlinks are removed automatically (just delete the directory)
3. Permanent execroot retains outputs in `bazel-out/`

### 5. How Actions Link Together

**Example**: `CppLink(busybox)` depends on `CppCompile(busybox)` and `CppLink(glibc)`

**Step 1: CppCompile(busybox) executes**:
- Sandbox created with symlink to `busybox.c`
- Executes: `gcc -c busybox.c -o busybox.o`
- Output `busybox.o` copied to permanent execroot: `bazel-out/k8-fastbuild/bin/busybox/busybox.o`
- Sandbox deleted

**Step 2: CppLink(glibc) executes** (in parallel):
- Similar process
- Output `libc.so` → `bazel-out/k8-fastbuild/bin/glibc/libc.so`

**Step 3: CppLink(busybox) executes**:
- Sandbox created with symlinks to:
  - `busybox.o` (from permanent execroot)
  - `libc.so` (from permanent execroot)
- Executes: `gcc busybox.o -lc -o busybox`
- Finds `libc.so` via symlink in sandbox's `bazel-out/`
- Output `busybox` → permanent execroot
- Sandbox deleted

### 6. Execution Logs

**Build Action Logs**:
- Stored in: `${output_base}/execroot/__main__/_tmp/actions/<action-id>.{out,err}`
- Persisted across builds (until cleaned)
- Accessible via `aquery` or build event protocol

**Test Logs**:
- Stored in: `bazel-out/<config>/testlogs/<package>/<test>/test.log`
- Includes stdout, stderr, exit code, timing
- Accessible via `bazel-testlogs/` symlink

**Viewing Logs**:
```bash
# Show action logs
bazel aquery 'mnemonic("CppCompile", //src:target)' --output=text

# Show test logs
bazel test //src:test --test_output=all

# Find log files
ls -la bazel-out/_tmp/actions/
ls -la bazel-testlogs/
```

## Applying to BitBake Tasks

### BitBake Task Graph

BitBake has a similar action graph structure:

```
[recipe.bb] → [do_fetch] → [sources] → [do_unpack] → [${S}/] → [do_patch] → [patched ${S}/]
                                                                                    ↓
                                                                              [do_configure]
                                                                                    ↓
                                                                              [Makefile]
                                                                                    ↓
                                                    [glibc sysroot] → [do_compile] → [${B}/*.o]
                                                                                    ↓
                                                                              [do_install]
                                                                                    ↓
                                                                         [${D}/, sysroot/]
```

### Proposed Directory Structure

**Permanent Artifact Store** (analogous to Bazel's execroot/bazel-out):

```
${CACHE_DIR}/
  artifacts/                            # Permanent artifact storage
    glibc/
      do_compile-<signature>/
        metadata.json                   # Task info, inputs, deps
        build/                          # ${B} outputs
          *.o
          glibc
        logs/
          log.do_compile
          run.do_compile

      do_install-<signature>/
        metadata.json
        image/                          # ${D} outputs
          usr/
            lib/
              libc.so
            include/
              stdio.h
        sysroot/                        # Files for dependent recipes
          usr/
            lib/
              libc.so
            include/
              stdio.h
        logs/
          log.do_install

    busybox/
      do_compile-<signature>/
        metadata.json
        build/
          busybox.o
          busybox
        logs/
          log.do_compile

      do_install-<signature>/
        metadata.json
        image/
          bin/
            busybox
        sysroot/
          bin/
            busybox
        logs/
          log.do_install
```

**Temporary Sandbox Per Task** (analogous to Bazel's sandbox/linux-sandbox/<id>):

```
${CACHE_DIR}/
  sandboxes/
    busybox-do_compile-<random-uuid>/   # TEMPORARY for busybox:do_compile
      work/                             # Task working directory

        # Symlinks to dependency outputs
        recipe-sysroot/
          usr/
            include/
              stdio.h -> ../../../../artifacts/glibc/do_install-<sig>/sysroot/usr/include/stdio.h
            lib/
              libc.so -> ../../../../artifacts/glibc/do_install-<sig>/sysroot/usr/lib/libc.so

        recipe-sysroot-native/
          usr/
            bin/
              gcc -> ../../../../artifacts/gcc-native/do_install-<sig>/sysroot/usr/bin/gcc

        # Symlinks to previous task outputs
        src/                            # From do_unpack
          busybox-1.36.1/ -> ../../../../artifacts/busybox/do_unpack-<sig>/src/busybox-1.36.1/

        # Real directories for outputs
        build/                          # ${B} - where compilation happens
        temp/                           # Logs
```

### Task Execution Workflow

**For `busybox:do_compile`**:

**1. Determine Inputs**:
- Source directory: Output from `busybox:do_patch`
- Dependency sysroots: Output from `glibc:do_install`, `ncurses:do_install`
- Native tools: Output from `gcc-native:do_install`

**2. Create Sandbox**:
```rust
let sandbox_id = Uuid::new_v4();
let sandbox_root = cache_dir.join("sandboxes")
    .join(format!("busybox-do_compile-{}", sandbox_id));

// Create work directory
fs::create_dir_all(&sandbox_root.join("work"))?;

// Symlink source from do_patch output
symlink_tree(
    &artifacts.join("busybox/do_patch-<sig>/src"),
    &sandbox_root.join("work/src")
)?;

// Build recipe-sysroot from dependencies
for dep in &["glibc:do_install", "ncurses:do_install"] {
    let dep_sysroot = artifacts.join(format!("{}-<sig>/sysroot", dep));
    symlink_tree(
        &dep_sysroot,
        &sandbox_root.join("work/recipe-sysroot")
    )?;
}

// Build recipe-sysroot-native
let gcc_sysroot = artifacts.join("gcc-native/do_install-<sig>/sysroot");
symlink_tree(
    &gcc_sysroot,
    &sandbox_root.join("work/recipe-sysroot-native")
)?;

// Create output directories
fs::create_dir_all(&sandbox_root.join("work/build"))?;
fs::create_dir_all(&sandbox_root.join("work/temp"))?;
```

**3. Execute Task**:
```rust
let mut cmd = Command::new("bash");
cmd.arg("-c").arg(&task_script);

cmd.current_dir(&sandbox_root.join("work"));

cmd.env("S", "/work/src/busybox-1.36.1");
cmd.env("B", "/work/build");
cmd.env("WORKDIR", "/work");
cmd.env("STAGING_INCDIR", "/work/recipe-sysroot/usr/include");
cmd.env("STAGING_LIBDIR", "/work/recipe-sysroot/usr/lib");

// Redirect stdout/stderr to log files
let stdout_log = File::create(&sandbox_root.join("work/temp/log.do_compile"))?;
let stderr_log = File::create(&sandbox_root.join("work/temp/log.do_compile.err"))?;

cmd.stdout(stdout_log);
cmd.stderr(stderr_log);

let result = cmd.output()?;
```

**4. Collect Outputs**:
```rust
// Copy outputs to permanent artifact storage
let artifact_dir = artifacts.join(format!(
    "busybox/do_compile-{}", task_signature
));

// Copy build outputs
fs::create_dir_all(&artifact_dir.join("build"))?;
copy_tree(
    &sandbox_root.join("work/build"),
    &artifact_dir.join("build")
)?;

// Copy logs
fs::create_dir_all(&artifact_dir.join("logs"))?;
copy_tree(
    &sandbox_root.join("work/temp"),
    &artifact_dir.join("logs")
)?;

// Store metadata
let metadata = TaskMetadata {
    recipe: "busybox".to_string(),
    task: "do_compile".to_string(),
    signature: task_signature.clone(),
    inputs: task_inputs.clone(),
    outputs: vec![
        "build/busybox.o",
        "build/busybox",
    ],
    exit_code: result.status.code().unwrap_or(-1),
    duration_ms: execution_time,
};

fs::write(
    &artifact_dir.join("metadata.json"),
    serde_json::to_string_pretty(&metadata)?
)?;
```

**5. Cleanup Sandbox**:
```rust
// Delete temporary sandbox
fs::remove_dir_all(&sandbox_root)?;
// Symlinks are automatically removed
```

### How Tasks Link Together

**Example: `busybox:do_install` depends on `busybox:do_compile`**

**After `busybox:do_compile` completes**:
- Outputs stored in: `artifacts/busybox/do_compile-<sig>/build/busybox`
- Sandbox deleted

**When `busybox:do_install` executes**:
- Sandbox created with symlinks to:
  - `build/` from `do_compile` → symlinked as `${B}/`
  - `src/` from `do_patch` → symlinked as `${S}/`
- Task script runs: `make install DESTDIR=${D}`
- Finds `busybox` binary in `${B}/` (symlinked to do_compile output)
- Installs to `${D}/bin/busybox`
- Outputs copied to: `artifacts/busybox/do_install-<sig>/image/bin/busybox`
- Sysroot outputs created: `artifacts/busybox/do_install-<sig>/sysroot/bin/busybox`

**When `core-image:do_rootfs` executes**:
- Sandbox created with symlinks to:
  - `artifacts/busybox/do_install-<sig>/image/` → merged into rootfs
  - `artifacts/base-files/do_install-<sig>/image/` → merged into rootfs
  - etc.
- Assembles complete filesystem
- Outputs: `artifacts/core-image/do_rootfs-<sig>/rootfs/`

### Key Differences from Current Implementation

| Aspect | Current (Wrong) | Proposed (Correct) |
|--------|----------------|-------------------|
| **Sandbox per task** | No - reuses same sandbox | Yes - new sandbox for each task execution |
| **Dependency outputs** | Not available | Symlinked into sandbox |
| **Output storage** | Not persistent | Permanent artifact cache by signature |
| **Task linking** | Manual path management | Automatic via symlink farm |
| **Logs** | Lost after execution | Stored in artifact cache |
| **Parallelism** | Limited | Full - sandboxes are isolated |
| **Caching** | Signature-only | Signature + artifacts |
| **Debugging** | No preserved state | Sandbox can be preserved on failure |

### Benefits

1. **Hermetic Execution**:
   - Each task sees only declared inputs
   - Undeclared dependencies cause failures
   - Builds are reproducible

2. **Proper Caching**:
   - Cache hit = reuse artifact outputs
   - No need to re-execute task
   - Artifacts can be shared across machines

3. **Parallel Execution**:
   - Tasks with satisfied dependencies run in parallel
   - Each has isolated sandbox
   - No conflicts

4. **Debugging**:
   - Preserved artifacts show exactly what task produced
   - Logs stored permanently
   - Can inspect sandbox on failure (with `--preserve-sandbox`)

5. **Remote Execution**:
   - Input/output declaration enables remote workers
   - Workers fetch inputs from CAS
   - Workers push outputs to CAS

### Implementation Checklist

- [ ] **Artifact Cache**:
  - [ ] Create permanent artifact storage structure
  - [ ] Implement artifact storage by `<recipe>-<task>-<signature>`
  - [ ] Store metadata (inputs, outputs, logs)

- [ ] **TaskSpec Enhancement**:
  - [ ] Add `TaskInputs` (source files, dependency tasks, env vars)
  - [ ] Add `TaskOutputs` (build, image, sysroot, deploy)
  - [ ] Track dependency signatures

- [ ] **SandboxBuilder**:
  - [ ] Create temporary sandbox directory per task
  - [ ] Implement `symlink_tree()` for dependency outputs
  - [ ] Build `recipe-sysroot/` from dependency sysroot outputs
  - [ ] Build `recipe-sysroot-native/` from native dependencies
  - [ ] Symlink previous task outputs (do_compile → do_install)

- [ ] **Output Collection**:
  - [ ] Copy task outputs to artifact cache
  - [ ] Store execution logs
  - [ ] Store task metadata (JSON)
  - [ ] Update artifact cache index

- [ ] **Sandbox Cleanup**:
  - [ ] Delete temporary sandbox after success
  - [ ] Preserve sandbox on failure (optional flag)
  - [ ] Clean old sandboxes

- [ ] **Integration**:
  - [ ] Extract dependency info from recipes (DEPENDS, RDEPENDS)
  - [ ] Extract task dependencies (do_compile[depends])
  - [ ] Determine which outputs go into sysroot
  - [ ] Handle native vs target separation

This design aligns bitzel with Bazel's proven per-action sandbox model while adapting it to BitBake's specific task execution model.
