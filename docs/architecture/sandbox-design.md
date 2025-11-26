# Bazel-Style Sandboxing for BitBake Tasks

## Problem Statement

In BitBake, tasks have complex dependency chains where:
- `busybox:do_compile` depends on `glibc:do_install` (needs headers/libraries)
- `busybox:do_install` depends on `busybox:do_compile` (needs built artifacts)
- Cross-compilation requires access to target sysroots with headers, libraries, etc.

Our current Basic sandbox just remaps paths, but doesn't properly handle:
1. Mapping outputs from dependency tasks as inputs to current task
2. Creating proper sysroot directory structure
3. Ensuring hermetic builds by only exposing declared dependencies

## How Bazel Actually Works

### 1. Execroot Structure

```
${output_base}/
  execroot/
    __main__/                    # Workspace root
      src/                       # Source files
      bazel-out/                 # All build outputs
        k8-fastbuild/
          bin/                   # Binary outputs
            package/
              target.o
          genfiles/              # Generated source files
```

### 2. Sandbox Creation (Per-Action)

For each action, Bazel:

1. **Creates temporary sandbox directory**:
   ```
   ${output_base}/sandbox/linux-sandbox/<random-number>/
     execroot/
       __main__/
         src/           -> symlink to real src files
         bazel-out/     -> symlinks to dependency outputs
   ```

2. **Symlink farm for inputs**:
   - Creates symlinks for ALL declared input files
   - Symlinks point to real files in execroot or source tree
   - Only includes inputs explicitly declared in action

3. **Executes action** with cwd = sandbox execroot

4. **Collects outputs**:
   - Copies known outputs from sandbox to real execroot
   - Deletes sandbox

### 3. Input Declaration

Actions declare inputs explicitly:
```python
ctx.actions.run(
    inputs = depset([src_file] + [dep.outputs for dep in ctx.attr.deps]),
    outputs = [output_file],
    ...
)
```

This ensures:
- Only declared dependencies are visible
- Undeclared dependencies cause build failures (hermetic builds)
- Cache invalidation is correct (based on input hashes)

### 4. Sysroot Handling for Cross-Compilation

**Problem**: GNU linkers resolve symlinks before checking if a file is in sysroot
- Symlinked `libc.so.6` appears to be outside sysroot
- Breaks cross-compilation in Bazel's sandbox

**Solutions**:
1. Use `--incompatible_use_specific_tool_files` to declare sysroot files explicitly
2. Use `--experimental_sandbox_base=/dev/shm` for better performance
3. Configure `cc_toolchain.sysroot_files` to ship sysroot into sandbox

## Applying to BitBake/Bitzel

### BitBake Task Dependencies and Outputs

In BitBake, each task produces outputs in specific directories:

```
${WORKDIR}/                    # Per-recipe work directory
  temp/                        # Logs, temp files
  recipe-sysroot/              # Dependencies' sysroots (for this recipe)
  recipe-sysroot-native/       # Native tools' sysroots
  ${S}/                        # Source directory
  ${B}/                        # Build directory
  ${D}/                        # Install destination (image)
  image/                       # Installed files
  sysroot-destdir/             # Files for dependent recipes
  deploy-*/                    # Deployed artifacts

${TMPDIR}/                     # Shared build artifacts
  work/                        # All recipe work dirs
  sysroots-components/         # Sysroot components
  deploy/                      # Shared deploy artifacts
```

### Task Input/Output Mapping

**Example: busybox:do_compile**

**Inputs** (must be declared):
1. Source files: `${S}/**` (from do_unpack, do_patch)
2. Build system: `${B}/Makefile` (from do_configure)
3. **Dependency sysroots**:
   - `recipe-sysroot/` (contains glibc headers/libs)
   - `recipe-sysroot-native/` (contains native gcc, binutils)
4. Environment variables: `CC`, `CFLAGS`, `LDFLAGS`, etc.

**Outputs**:
1. Built objects: `${B}/*.o`, `${B}/busybox`
2. Logs: `${WORKDIR}/temp/log.do_compile`

**Example: busybox:do_install**

**Inputs**:
1. Built binaries: `${B}/busybox` (from do_compile)
2. Install scripts: `${S}/Makefile` install target

**Outputs**:
1. Installed files: `${D}/bin/busybox`, `${D}/usr/share/...`
2. Sysroot files: `${WORKDIR}/sysroot-destdir/` (for dependent recipes)

### Proposed Sandbox Structure for Bitzel

For each task execution, create:

```
${CACHE_DIR}/sandboxes/<task-signature>/
  work/                        # Maps to ${WORKDIR}
    recipe-sysroot/            # Symlinks to dependency sysroots
      usr/
        include/               -> ${CACHE_DIR}/artifacts/glibc-<sig>/sysroot/usr/include
        lib/                   -> ${CACHE_DIR}/artifacts/glibc-<sig>/sysroot/usr/lib
    recipe-sysroot-native/     # Symlinks to native tools
      usr/
        bin/
          gcc                  -> ${CACHE_DIR}/artifacts/gcc-native-<sig>/sysroot/usr/bin/gcc
    src/                       # Symlinks to source files
      busybox-1.36.1/          -> ${SOURCE_DIR}/busybox-1.36.1/
    build/                     # Real directory for build outputs
    image/                     # Real directory for install outputs
    sysroot-destdir/           # Real directory for sysroot outputs
    temp/                      # Real directory for logs
```

### Key Differences from Current Implementation

**Current (Wrong)**:
- Just remaps `/work` to sandbox directory
- No dependency sysroot mapping
- Tasks can't find headers/libraries from dependencies
- Not hermetic (tasks could access anything on host)

**Proposed (Correct)**:
- **Input declaration**: Tasks declare which dependency outputs they need
- **Symlink farm**: Create symlinks for all declared inputs
- **Sysroot aggregation**: Assemble `recipe-sysroot/` from dependency outputs
- **Native sysroot**: Assemble `recipe-sysroot-native/` from native tool outputs
- **Hermetic**: Only declared inputs are visible
- **Output collection**: Move outputs to artifact cache by signature

### Implementation Plan

#### Phase 1: Task Input/Output Declaration

Extend `TaskSpec` to declare inputs:

```rust
pub struct TaskSpec {
    pub recipe: String,
    pub task: String,
    pub signature: String,

    // New fields
    pub inputs: TaskInputs,
    pub outputs: TaskOutputs,
}

pub struct TaskInputs {
    // Source files
    pub source_files: Vec<PathBuf>,

    // Dependency outputs (from other tasks)
    pub dep_tasks: Vec<TaskDependency>,

    // Environment variables
    pub env_vars: HashMap<String, String>,
}

pub struct TaskDependency {
    pub recipe: String,
    pub task: String,
    pub signature: String,

    // What outputs to include
    pub output_type: DependencyOutputType,
}

pub enum DependencyOutputType {
    // Include in recipe-sysroot (headers, libraries)
    Sysroot,

    // Include in recipe-sysroot-native (native tools)
    SysrootNative,

    // Direct file access (e.g., do_compile -> do_configure)
    Direct(Vec<PathBuf>),
}

pub struct TaskOutputs {
    // Build artifacts
    pub build_outputs: Vec<PathBuf>,  // ${B}/**

    // Installed files
    pub install_outputs: Vec<PathBuf>,  // ${D}/**

    // Sysroot contributions
    pub sysroot_outputs: Vec<PathBuf>,  // ${WORKDIR}/sysroot-destdir/**

    // Deployed artifacts
    pub deploy_outputs: Vec<PathBuf>,  // ${WORKDIR}/deploy-*/**
}
```

#### Phase 2: Artifact Cache

Store task outputs by signature:

```
${CACHE_DIR}/
  artifacts/
    <recipe>-<task>-<signature>/
      metadata.json            # Task info, inputs, outputs
      build/                   # ${B} outputs
      image/                   # ${D} outputs
      sysroot/                 # Sysroot files for dependencies
      deploy/                  # Deploy files
      logs/                    # Execution logs
```

#### Phase 3: Sandbox Builder

Create sandbox with symlink farm:

```rust
pub struct SandboxBuilder {
    cache_dir: PathBuf,
    sandbox_root: PathBuf,
}

impl SandboxBuilder {
    pub fn create_sandbox(&self, spec: &TaskSpec) -> Result<Sandbox> {
        let sandbox = self.sandbox_root.join(&spec.signature);

        // Create work directory structure
        fs::create_dir_all(sandbox.join("work"))?;

        // 1. Symlink source files
        self.link_source_files(&sandbox, &spec.inputs.source_files)?;

        // 2. Build recipe-sysroot from dependencies
        self.build_recipe_sysroot(&sandbox, &spec.inputs.dep_tasks)?;

        // 3. Build recipe-sysroot-native from native dependencies
        self.build_native_sysroot(&sandbox, &spec.inputs.dep_tasks)?;

        // 4. Create real directories for outputs
        fs::create_dir_all(sandbox.join("work/build"))?;
        fs::create_dir_all(sandbox.join("work/image"))?;
        fs::create_dir_all(sandbox.join("work/sysroot-destdir"))?;
        fs::create_dir_all(sandbox.join("work/temp"))?;

        Ok(Sandbox { root: sandbox, spec: spec.clone() })
    }

    fn build_recipe_sysroot(
        &self,
        sandbox: &Path,
        deps: &[TaskDependency],
    ) -> Result<()> {
        let sysroot = sandbox.join("work/recipe-sysroot");

        for dep in deps {
            if matches!(dep.output_type, DependencyOutputType::Sysroot) {
                // Find dependency's sysroot outputs in cache
                let dep_artifact = self.cache_dir
                    .join("artifacts")
                    .join(format!("{}-{}-{}", dep.recipe, dep.task, dep.signature));

                let dep_sysroot = dep_artifact.join("sysroot");

                // Symlink all files from dep sysroot into recipe-sysroot
                self.symlink_tree(&dep_sysroot, &sysroot)?;
            }
        }

        Ok(())
    }

    fn symlink_tree(&self, src: &Path, dst: &Path) -> Result<()> {
        // Recursively create symlinks for all files in src -> dst
        // Merge multiple sources into same destination tree
        for entry in WalkDir::new(src) {
            let entry = entry?;
            let rel_path = entry.path().strip_prefix(src)?;
            let dst_path = dst.join(rel_path);

            if entry.file_type().is_dir() {
                fs::create_dir_all(&dst_path)?;
            } else {
                fs::create_dir_all(dst_path.parent().unwrap())?;
                unix::fs::symlink(entry.path(), &dst_path)?;
            }
        }
        Ok(())
    }
}
```

#### Phase 4: Output Collection

After task execution, collect outputs to artifact cache:

```rust
impl Sandbox {
    pub fn collect_outputs(&self) -> Result<TaskArtifacts> {
        let work = self.root.join("work");

        let artifacts = TaskArtifacts {
            build_outputs: self.collect_dir(&work.join("build"))?,
            install_outputs: self.collect_dir(&work.join("image"))?,
            sysroot_outputs: self.collect_dir(&work.join("sysroot-destdir"))?,
            logs: self.collect_dir(&work.join("temp"))?,
        };

        // Store in artifact cache
        let cache_path = self.cache_dir
            .join("artifacts")
            .join(format!("{}-{}-{}",
                self.spec.recipe, self.spec.task, self.spec.signature));

        self.store_artifacts(&artifacts, &cache_path)?;

        Ok(artifacts)
    }
}
```

### Benefits of This Approach

1. **Hermetic Builds**:
   - Only declared dependencies are visible
   - Undeclared dependencies cause build failures
   - Reproducible across machines

2. **Correct Caching**:
   - Task signature includes all input signatures
   - Cache hits only when all inputs unchanged
   - Can reuse cached artifacts

3. **Parallel Execution**:
   - Tasks with satisfied dependencies can run in parallel
   - Each task has isolated sandbox
   - No conflicts between concurrent tasks

4. **Debugging**:
   - Sandbox preserved on failure
   - Can inspect what inputs were available
   - Can re-run task in same environment

5. **Remote Execution**:
   - Input/output declaration enables shipping to remote workers
   - Workers can fetch inputs from CAS
   - Workers can push outputs to CAS

### BitBake Sysroot Structure

In BitBake, `recipe-sysroot` contains:

```
recipe-sysroot/
  usr/
    include/           # Headers from dependencies
    lib/               # Libraries from dependencies
    share/             # Shared files
  etc/                 # Configuration files
  sysroot-providers/   # Metadata about what provided what
```

When `busybox:do_compile` declares dependency on `glibc:do_install`, bitzel should:

1. Find cached artifact for `glibc:do_install:<signature>`
2. Symlink `glibc` sysroot outputs into `busybox` sandbox's `recipe-sysroot/`
3. Set `--sysroot=/work/recipe-sysroot` in `CFLAGS`/`LDFLAGS`

This ensures:
- Compiler finds the right headers
- Linker finds the right libraries
- Build is hermetic (only sees declared dependencies)

### Comparison to Current Implementation

| Aspect | Current | Proposed |
|--------|---------|----------|
| Input declaration | None | Explicit per task |
| Dependency outputs | Not available | Symlinked into sandbox |
| Sysroot | None | Assembled from dependencies |
| Hermeticity | No (sees host filesystem) | Yes (only declared inputs) |
| Caching | Signature-based | Signature + artifact cache |
| Debugging | Minimal | Full sandbox preserved |
| Parallelism | Limited | Full parallel execution |

### Next Steps

1. Extend TaskSpec with input/output declarations
2. Implement artifact cache storage
3. Implement SandboxBuilder with symlink farm
4. Update sandbox backends (Bubblewrap, etc.) to use new structure
5. Extract dependency information from BitBake recipes
6. Test with real BitBake recipes (busybox, glibc, etc.)

### Open Questions

1. **How to extract dependency info from recipes?**
   - Parse DEPENDS, RDEPENDS
   - Analyze task dependencies (do_compile[depends] = "...")
   - Determine which outputs go into sysroot

2. **How to handle native vs target sysroots?**
   - Separate recipe-sysroot (target) and recipe-sysroot-native (host)
   - Track architecture in task signatures

3. **How to handle STAGING_DIR vs recipe-sysroot?**
   - BitBake uses both
   - recipe-sysroot is per-recipe subset of STAGING_DIR

4. **How to handle postinst scripts and package splitting?**
   - do_package splits ${D} into multiple packages
   - do_package_write_* creates actual packages
   - These need proper input/output tracking

This design aligns bitzel's sandboxing with Bazel's proven model while adapting it to BitBake's specific requirements around sysroots and cross-compilation.
