# Bitzel Real Build Execution - Failure Analysis

## Executive Summary

Successfully ran bitzel build orchestrator up through task graph generation and signature computation. Execution **fails immediately** on the first task due to sandbox path mismatches and missing infrastructure.

## Build Statistics (What Worked)

✅ **Repository Fetching**: 1 repo cloned (poky)
✅ **Layer Discovery**: 3 layers loaded with priorities
✅ **Recipe Parsing**: 884 recipes parsed successfully in parallel
✅ **Task Extraction**: 667 task implementations from 416 recipes
✅ **Dependency Graph**: 884 recipes, 7990 tasks, 1048 dependencies
✅ **Task Graph**: 7990 tasks with proper dependency structure
✅ **Signature Computation**: 7963 task signatures (content-addressable)
✅ **Task Specifications**: 7963 task specs created
  - 11 tasks with real implementations
  - 7979 tasks with placeholders

## First Failure: libmnl:unpack

```
Task: libmnl:unpack
Error: Missing output: /work/outputs/unpack.done
Sandbox: build/bitzel-cache/sandboxes/c74174de-fa42-436f-97af-3c55e875efa0
Duration: 0.05s
```

## Root Cause Analysis

### 1. **Sandbox Path Mismatch**

**Problem**: Scripts expect `/work/outputs/` but sandbox is at different path

**Evidence**:
```rust
// From bitzel/src/main.rs:446
let output_file = format!("{}.done", task.task_name);
full_script.push_str(&format!(
    "# Mark task complete\nmkdir -p /work/outputs\necho 'completed' > /work/outputs/{}\n",
    output_file
));

// But spec expects:
TaskSpec {
    workdir: task_workdir,  // build/tmp/libmnl/unpack
    outputs: vec![PathBuf::from(&output_file)],  // "unpack.done" (relative?)
}
```

**Issue**:
- Script writes to **hardcoded** `/work/outputs/unpack.done`
- TaskSpec expects output at workdir (likely `build/tmp/libmnl/unpack/unpack.done`)
- Sandbox is at `build/bitzel-cache/sandboxes/<uuid>/`
- **No path mapping** between these locations

### 2. **Missing Sandbox Isolation**

**Warning**:
```
⚠️  Bubblewrap not found, falling back to basic sandbox
⚠️  Using basic sandbox - NO REAL ISOLATION
```

**Problem**: Basic sandbox doesn't provide:
- Proper filesystem isolation
- Mount namespace for /work mapping
- Bind mounts for dependencies
- Process isolation

**What's needed**:
```bash
# Bubblewrap command that should be executed:
bwrap \
  --unshare-all \
  --new-session \
  --die-with-parent \
  --bind /sandbox/uuid /work \              # Map sandbox to /work
  --bind /artifact-cache/deps /work/deps \  # Mount dependencies
  --dev /dev \
  --proc /proc \
  --tmpfs /tmp \
  bash -c "script.sh"
```

### 3. **Environment Variable Mismatch**

**Scripts expect BitBake environment**:
```bash
export PN="libmnl"
export WORKDIR="${WORKDIR:-/work}"    # Defaults to /work
export S="${S:-${WORKDIR}/src}"       # /work/src
export B="${B:-${WORKDIR}/build}"     # /work/build
export D="${D:-${WORKDIR}/outputs}"   # /work/outputs
```

**But executor runs at**:
```
build/bitzel-cache/sandboxes/c74174de-fa42-436f-97af-3c55e875efa0
```

**Without environment setup**, tasks will fail or write to wrong locations.

### 4. **Output Verification Logic**

**Executor checks**:
```rust
// After task execution, executor checks:
if outputs.exists() {
    // Success
} else {
    return Err("Missing output: /work/outputs/unpack.done")
}
```

**Problem**: Executor checks for file at path from script, but doesn't know where sandbox actually wrote it.

## Missing Infrastructure

### 1. **Proper Sandbox Implementation**

Current state:
```rust
// convenient-bitbake/src/executor/sandbox_backend.rs
SandboxBackend::Basic  // No isolation, no mount namespace
```

**Needs**:
```rust
pub enum SandboxBackend {
    Bubblewrap {
        work_mount: PathBuf,      // /work -> sandbox_dir
        dep_mounts: Vec<PathBuf>, // Dependencies from artifact cache
        isolation: IsolationLevel,
    },
    Docker {
        image: String,
        volumes: Vec<VolumeMount>,
    },
}

pub struct SandboxEnvironment {
    /// Physical location of sandbox
    sandbox_dir: PathBuf,  // build/bitzel-cache/sandboxes/<uuid>

    /// Virtual mount point inside sandbox
    work_dir: PathBuf,     // /work (what task sees)

    /// Bind mounts for dependencies
    sysroot_mounts: Vec<(PathBuf, PathBuf)>,  // (artifact_cache_path, /work/recipe-sysroot)
}
```

### 2. **Sysroot Assembly (Hardlink-Based)**

From HARDLINK_SYSROOT_DESIGN.md, tasks need:
```
/work/recipe-sysroot/        # Target sysroot (glibc, libs)
/work/recipe-sysroot-native/ # Native tools (gcc, make)
/work/src/                   # Source files
/work/build/                 # Build outputs
/work/outputs/               # Package outputs
```

**Implementation needed**:
```rust
pub struct SysrootAssembler {
    artifact_cache: PathBuf,
    dup_whitelist: Vec<PathBuf>,
}

impl SysrootAssembler {
    /// Assemble task sysroot from dependencies using hardlinks
    pub fn assemble_for_task(
        &self,
        task: &Task,
        sandbox_dir: &Path,
    ) -> Result<()> {
        let sysroot = sandbox_dir.join("work/recipe-sysroot");

        for dep in &task.depends_on {
            let dep_artifact = self.artifact_cache.join(&dep.recipe)
                .join(format!("{}-{}", dep.task, dep.signature));

            // Hardlink dep outputs into sysroot
            copyhardlinktree(&dep_artifact.join("sysroot"), &sysroot)?;
        }

        Ok(())
    }
}
```

### 3. **Path Translation Layer**

**Problem**: Need to translate between three path spaces:
1. **Host paths**: `build/bitzel-cache/sandboxes/<uuid>/`
2. **Sandbox virtual paths**: `/work/`
3. **BitBake expected paths**: `${WORKDIR}`, `${S}`, `${B}`, `${D}`

**Solution**:
```rust
pub struct PathTranslator {
    sandbox_dir: PathBuf,      // Host path
    work_mount: PathBuf,       // Virtual path inside sandbox
}

impl PathTranslator {
    /// Translate output specification to actual file location
    pub fn translate_output(&self, spec_output: &Path) -> PathBuf {
        // spec_output might be:
        // - "unpack.done" (relative to workdir)
        // - "/work/outputs/unpack.done" (absolute sandbox path)

        if spec_output.is_absolute() {
            // Strip /work prefix, map to sandbox_dir
            let rel = spec_output.strip_prefix("/work").unwrap_or(spec_output);
            self.sandbox_dir.join("work").join(rel)
        } else {
            // Relative to workdir
            self.sandbox_dir.join("work/outputs").join(spec_output)
        }
    }
}
```

### 4. **Artifact Collection**

After task execution, need to collect outputs to artifact cache:
```rust
pub struct ArtifactCollector {
    artifact_cache: PathBuf,
}

impl ArtifactCollector {
    pub fn collect_task_outputs(
        &self,
        task: &Task,
        signature: &str,
        sandbox_dir: &Path,
    ) -> Result<()> {
        let task_artifact_dir = self.artifact_cache
            .join(&task.recipe_name)
            .join(format!("{}-{}", task.task_name, signature));

        fs::create_dir_all(&task_artifact_dir)?;

        // Collect outputs
        // 1. Copy /work/outputs/* to task_artifact_dir/outputs/
        // 2. Create sysroot manifest
        // 3. Save execution logs

        Ok(())
    }
}
```

### 5. **Dependency Input Staging**

Before task execution, stage dependency outputs:
```rust
pub struct DependencyStager {
    artifact_cache: PathBuf,
}

impl DependencyStager {
    pub fn stage_dependencies(
        &self,
        task: &Task,
        sandbox_dir: &Path,
    ) -> Result<()> {
        // For each dependency:
        // 1. Locate artifact in cache
        // 2. Hardlink artifact outputs into sandbox

        for dep_id in &task.depends_on {
            let dep_task = task_graph.get_task(dep_id);
            let dep_sig = sig_cache.get_signature(&dep_task.recipe_name, &dep_task.task_name);

            let dep_artifact = self.artifact_cache
                .join(&dep_task.recipe_name)
                .join(format!("{}-{}", dep_task.task_name, dep_sig));

            if !dep_artifact.exists() {
                return Err(format!("Missing dependency artifact: {:?}", dep_artifact).into());
            }

            // Hardlink into sandbox
            copyhardlinktree(&dep_artifact, &sandbox_dir.join("work/deps"))?;
        }

        Ok(())
    }
}
```

## Execution Flow That Would Work

```rust
// 1. Get ready tasks
let ready_tasks = task_graph.get_ready_tasks(&completed);

// 2. For each batch of ready tasks (limited by max_parallel)
for task_id in ready_tasks.take(max_parallel) {
    let task = task_graph.get_task(task_id);
    let sig = sig_cache.get_signature(&task.recipe_name, &task.task_name);

    // 3. Check artifact cache
    let artifact_path = artifact_cache.path(&task.recipe_name, &task.task_name, sig);
    if artifact_path.exists() {
        // Cache HIT - skip execution
        completed.insert(task_id);
        continue;
    }

    // 4. Create sandbox
    let sandbox_dir = create_sandbox_dir();

    // 5. Stage dependencies (hardlink from artifact cache)
    dependency_stager.stage_dependencies(task, &sandbox_dir)?;

    // 6. Assemble sysroots (hardlink from dependency artifacts)
    sysroot_assembler.assemble_for_task(task, &sandbox_dir)?;

    // 7. Execute with bubblewrap
    bubblewrap_execute(
        task,
        &sandbox_dir,
        bind_mounts: [
            (sandbox_dir, "/work"),
        ],
    )?;

    // 8. Collect outputs to artifact cache
    artifact_collector.collect_task_outputs(task, sig, &sandbox_dir)?;

    // 9. Clean up sandbox
    fs::remove_dir_all(&sandbox_dir)?;

    completed.insert(task_id);
}
```

## What Needs to Be Implemented

### Critical Path (Must Have)

1. **Bubblewrap Sandbox Backend** (`convenient-bitbake/src/executor/sandbox_backend.rs`)
   - Install bubblewrap: `apt install bubblewrap`
   - Implement mount namespace setup
   - Bind mount sandbox_dir to /work
   - Environment variable injection

2. **Path Translator** (new file)
   - Translate between host, sandbox, and BitBake paths
   - Handle absolute vs relative paths in output specs

3. **Dependency Stager** (new file)
   - Locate dependency artifacts by signature
   - Hardlink dependency outputs into sandbox
   - Verify all dependencies present before execution

4. **Sysroot Assembler** (implements HARDLINK_SYSROOT_DESIGN.md)
   - Extract do_populate_sysroot outputs from dependencies
   - Hardlink into recipe-sysroot using `cp -afl`
   - File conflict detection with manifests

5. **Artifact Collector** (new file)
   - Collect task outputs after execution
   - Save to artifact cache by signature
   - Create sysroot manifest
   - Save execution logs

### Nice to Have

6. **Better Task Implementations**
   - Extract more real task implementations (currently 11/7990)
   - Parse task functions from recipes
   - Handle Python tasks

7. **Environment Variable Extraction**
   - Extract recipe variables (WORKDIR, S, B, D, etc.)
   - Pass to task execution
   - Override resolution (MACHINE, DISTRO)

8. **Remote Caching**
   - Bazel Remote API v2 client
   - Upload artifacts after execution
   - Download artifacts before execution (cache check)

## Quick Wins

### Fix 1: Install Bubblewrap
```bash
apt install bubblewrap
```

This enables real sandbox isolation.

### Fix 2: Fix Path in Output Verification

```rust
// bitzel/src/main.rs:463
let output_file = format!("{}.done", task.task_name);
let spec = TaskSpec {
    workdir: task_workdir,
    outputs: vec![PathBuf::from("/work/outputs").join(&output_file)],  // Absolute path!
    //                           ^^^^ Make this match what script writes
};
```

### Fix 3: Create Sandbox Work Directory Structure

```rust
// Before task execution
let sandbox_work = sandbox_dir.join("work");
fs::create_dir_all(&sandbox_work.join("outputs"))?;
fs::create_dir_all(&sandbox_work.join("src"))?;
fs::create_dir_all(&sandbox_work.join("build"))?;
fs::create_dir_all(&sandbox_work.join("recipe-sysroot"))?;
fs::create_dir_all(&sandbox_work.join("recipe-sysroot-native"))?;
```

### Fix 4: Use Bubblewrap to Mount Sandbox

```rust
// Execute with path mapping
Command::new("bwrap")
    .args(&[
        "--unshare-all",
        "--new-session",
        "--die-with-parent",
        "--bind", &sandbox_dir.join("work").display().to_string(), "/work",
        "--dev", "/dev",
        "--proc", "/proc",
        "--tmpfs", "/tmp",
        "bash", "-c", &task_script,
    ])
    .output()?;
```

## Summary

**Build infrastructure works up to execution**, but fails immediately due to:
1. ❌ Missing sandbox isolation (no bubblewrap)
2. ❌ Path mismatch (script writes /work/outputs/, executor checks elsewhere)
3. ❌ No dependency staging
4. ❌ No sysroot assembly
5. ❌ No artifact collection

**To get first task working**:
1. Install bubblewrap
2. Fix output path in TaskSpec
3. Create sandbox /work directory structure
4. Use bwrap to mount sandbox at /work
5. Fix output verification to check correct path

**To get full build working**:
- Implement all 5 critical infrastructure components above
- Extract more real task implementations
- Handle dependency chains properly
