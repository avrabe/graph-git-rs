# Existing Rust Crates for Build Orchestration and Sandboxing

## Should We Build or Use Existing Crates?

**TL;DR**: Mix of both - use existing crates for sandboxing and low-level primitives, build our own orchestration layer.

## 1. Process Sandboxing (USE EXISTING CRATES)

### Option A: **hakoniwa** ⭐ RECOMMENDED
```toml
[dependencies]
hakoniwa = "0.1"
```

**What it provides**:
- Linux namespaces (MNT, PID, USER, NET, IPC)
- Mount namespace + pivot_root
- Seccomp filtering
- Resource limits (rlimit)
- LandLock LSM support
- **Inspired by Bubblewrap**

**Example**:
```rust
use hakoniwa::Sandbox;

let sandbox = Sandbox::new()
    .mount("/work", sandbox_dir)
    .mount("/work/deps", deps_dir)
    .bind("/dev", "/dev", BindFlags::ReadOnly)
    .tmpfs("/tmp")
    .env("WORKDIR", "/work")
    .build()?;

sandbox.run(&["bash", "-c", &task_script])?;
```

**Pros**:
- ✅ Pure Rust (no external binary dependency)
- ✅ Handles namespace creation properly
- ✅ Support for unprivileged users
- ✅ Security hardening (seccomp, landlock)

**Cons**:
- Still young (may need contributions)

### Option B: **bastille**
```toml
[dependencies]
bastille = "0.4"
```

**What it provides**:
- Process sandboxing via namespaces
- Similar API to Bubblewrap
- Clone process in new namespace

**Example**:
```rust
use bastille::{Sandbox, BindMount};

let sandbox = Sandbox::new("/work")
    .bind_mount(BindMount::new(sandbox_dir, "/work"))
    .bind_mount(BindMount::new(deps_dir, "/work/deps"))
    .spawn(&["bash", "-c", &script])?;
```

### Option C: **youki/libcontainer**
```toml
[dependencies]
libcontainer = "0.5"
```

**What it provides**:
- Full OCI runtime spec implementation
- All container features (cgroups, namespaces, seccomp)
- Compatible with podman/docker

**Example**:
```rust
use libcontainer::container::builder::ContainerBuilder;

let container = ContainerBuilder::new("task-id".to_string(), "rootfs")
    .with_rootfs(sandbox_dir)
    .with_mounts(vec![...])
    .build()?;

container.start()?;
```

**Pros**:
- ✅ Full OCI compatibility
- ✅ Battle-tested (used in production)
- ✅ Comprehensive isolation

**Cons**:
- ❌ Heavy (full container runtime)
- ❌ Requires OCI bundle format
- ❌ Overkill for task execution?

### Option D: Direct **nix** crate (MANUAL)
```toml
[dependencies]
nix = "0.29"
```

**What it provides**:
- Low-level namespace syscalls
- Manual control over everything

**Example**:
```rust
use nix::sched::{unshare, CloneFlags};
use nix::mount::{mount, MsFlags};

// Create namespace
unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID)?;

// Mount sandbox
mount(
    Some(sandbox_dir),
    "/work",
    None::<&str>,
    MsFlags::MS_BIND,
    None::<&str>,
)?;

// Execute
Command::new("bash").args(&["-c", &script]).spawn()?;
```

**Pros**:
- ✅ Full control
- ✅ Minimal dependencies

**Cons**:
- ❌ Have to implement everything ourselves
- ❌ Easy to get wrong (security)

---

## 2. Build Orchestration (BUILD OUR OWN, inspired by existing)

### Existing: **Buck2** (Meta's build system)

**Written in Rust**, but designed for general builds (not BitBake-specific).

**What we can learn from Buck2**:
- Dependency graph as key-value store
- Everything is content-addressable
- Single unified graph (no separate phases)
- Remote execution API v2 support

**Should we use it?**
- ❌ No - Buck2 is a complete build system with its own DSL (Starlark)
- ❌ Doesn't understand BitBake recipes
- ✅ But we can **learn** from its architecture

**What to adopt from Buck2**:
```rust
// Buck2's approach to dependency graph
pub trait Key {
    type Value;
    fn compute(key: &Self, deps: &dyn Graph) -> Self::Value;
}

// Everything is a key-value pair
// Recipe + Task = Key → Artifacts = Value
```

### Existing: **async-jobs** ⭐ USEFUL
```toml
[dependencies]
async-jobs = "0.3"
```

**What it provides**:
- Framework for interdependent async jobs
- Complex dependency graphs
- Designed for build systems
- Job scheduling backbone

**Example**:
```rust
use async_jobs::{Job, JobGraph};

let mut graph = JobGraph::new();

graph.add_job(Job::new("compile", vec!["fetch"])
    .with_fn(|| async {
        // Compile task
    }));

graph.add_job(Job::new("fetch", vec![])
    .with_fn(|| async {
        // Fetch sources
    }));

graph.run().await?;
```

**Pros**:
- ✅ Handles dependency resolution
- ✅ Async execution
- ✅ Designed for build systems

**Cons**:
- May not fit our exact model (signatures, caching)

### Existing: **fn_graph** ⭐ USEFUL
```toml
[dependencies]
fn_graph = "0.11"
```

**What it provides**:
- Register functions with interdependencies
- Stream of functions to execute (sequential or concurrent)
- Dependency tracking

**Example**:
```rust
use fn_graph::FnGraph;

let graph = FnGraph::new()
    .with_fn("compile", &["fetch"], |ctx| {
        // Compile task
    })
    .with_fn("fetch", &[], |ctx| {
        // Fetch sources
    });

// Execute in dependency order
for func in graph.stream() {
    func.execute()?;
}
```

**Recommendation**: We could use **fn_graph** as the foundation for task scheduling, but add:
- Task signature computation
- Artifact caching
- Wave-based parallel execution
- Remote caching integration

---

## 3. Remote Caching / Remote Execution (USE EXISTING PROTOCOLS)

### Bazel Remote Execution API v2

**Protocol**: gRPC-based, defined at https://github.com/bazelbuild/remote-apis

**Existing Rust clients**: None complete, but protobuf definitions available

**What we need to build**:
```rust
use tonic::transport::Channel;
use bazel_remote_apis::build::bazel::remote::execution::v2::*;

pub struct RemoteCache {
    client: ActionCacheClient<Channel>,
    cas_client: ContentAddressableStorageClient<Channel>,
}

impl RemoteCache {
    pub async fn get_action_result(&self, digest: &Digest) -> Option<ActionResult> {
        // Check if action was cached
    }

    pub async fn upload_blob(&self, data: &[u8]) -> Digest {
        // Upload artifact to CAS
    }
}
```

**Recommendation**: Implement thin client for Bazel Remote API v2
- **Protocol is standardized** - works with BuildBuddy, bazel-remote, BuildKit, etc.
- Only ~300 lines of code for basic client
- Enables remote caching with any compatible server

### Existing: **NativeLink** (Rust implementation of Bazel Remote Execution)

Could run our own NativeLink server for remote caching:
```bash
# Run NativeLink server
docker run -p 50051:50051 tracemachina/nativelink

# Point bitzel at it
bitzel --remote-cache=grpc://localhost:50051
```

---

## 4. Hardlink Tree Operations (BUILD OUR OWN)

**No existing crate** does exactly what BitBake needs (cp -afl with conflict detection).

**Simple implementation**:
```rust
use std::fs;
use std::os::unix::fs::MetadataExt;

pub fn copyhardlinktree(src: &Path, dst: &Path) -> Result<()> {
    // Check if same filesystem
    let src_stat = fs::metadata(src)?;
    let dst_stat = fs::metadata(dst.parent().unwrap())?;

    if src_stat.dev() == dst_stat.dev() {
        // Use cp -afl (fastest - hardlinks)
        Command::new("cp")
            .args(&["-afl", "--preserve=xattr"])
            .arg(src)
            .arg(dst)
            .output()?;
    } else {
        // Fall back to copy
        // Use rsync or custom walker
        Command::new("rsync")
            .args(&["-a", "--link-dest", src.to_str().unwrap()])
            .arg(src)
            .arg(dst)
            .output()?;
    }

    Ok(())
}
```

**Or use**:
```toml
[dependencies]
fs_extra = "1.3"  # For recursive file operations
```

---

## RECOMMENDED ARCHITECTURE

### Use Existing Crates For:

1. **Sandboxing**: `hakoniwa` or `bastille`
   - Handles namespaces properly
   - Security hardening built-in
   - Less code to maintain

2. **Task Dependency Graph**: `fn_graph` or `async-jobs`
   - Proven dependency resolution
   - Async execution support
   - Focus on BitBake-specific logic instead

3. **Low-level primitives**: `nix`
   - Mount operations
   - File metadata
   - Namespace helpers

4. **Remote Execution Protocol**: Bazel Remote API v2 protobuf definitions
   - Standardized protocol
   - Works with existing servers

### Build Our Own:

1. **BitBake-specific orchestration**
   - Recipe parsing
   - Override resolution (MACHINE/DISTRO)
   - BBappend merging
   - Task signature computation

2. **Sysroot assembly**
   - Hardlink tree merging (cp -afl)
   - File conflict detection
   - Manifest tracking

3. **Artifact caching layer**
   - Content-addressable storage
   - Signature-based retrieval
   - Remote cache integration

4. **Task execution coordinator**
   - Wave-based scheduling
   - Concurrency limiting
   - Cache checking
   - Dependency staging

---

## ISOLATION LEVELS

You mentioned "I would have gone that far to isolate each task execution" - let's discuss options:

### Level 1: Basic Namespace Isolation (MINIMUM)
```rust
use hakoniwa::Sandbox;

Sandbox::new()
    .mount("/work", sandbox_dir)          // Isolated filesystem
    .unshare_user()                        // User namespace
    .unshare_pid()                         // PID namespace
    .unshare_mount()                       // Mount namespace
    .run(&["bash", "-c", &script])?;
```

**Provides**:
- ✅ Isolated filesystem (/work is separate)
- ✅ Can't see host processes
- ✅ Can't escape to host filesystem

### Level 2: Network Isolation
```rust
Sandbox::new()
    // ... Level 1 ...
    .unshare_network()                     // Network namespace
    .run(&["bash", "-c", &script])?;
```

**Provides**:
- ✅ No network access (unless explicitly allowed)
- ✅ Can't download during build (reproducibility)

**Issue**: Some BitBake tasks need network (do_fetch)
**Solution**: Selective network access per task

### Level 3: Resource Limits
```rust
Sandbox::new()
    // ... Level 2 ...
    .cpu_quota(2.0)                        // Max 2 CPUs
    .memory_limit(4 * 1024 * 1024 * 1024)  // 4GB RAM
    .run(&["bash", "-c", &script])?;
```

**Provides**:
- ✅ Can't exhaust system resources
- ✅ Predictable performance

### Level 4: Seccomp Filtering
```rust
use hakoniwa::seccomp::SeccompFilter;

Sandbox::new()
    // ... Level 3 ...
    .seccomp(SeccompFilter::new()
        .allow_syscall("read")
        .allow_syscall("write")
        .allow_syscall("open")
        // ... whitelist specific syscalls ...
        .deny_default())
    .run(&["bash", "-c", &script])?;
```

**Provides**:
- ✅ Can only make whitelisted syscalls
- ✅ Defense in depth

**Issue**: Need to profile tasks to know which syscalls they need

### Level 5: Full Container (OCI Runtime)
```rust
use libcontainer::container::builder::ContainerBuilder;

ContainerBuilder::new("task-id", rootfs_path)
    .with_cgroups(...)
    .with_namespaces(...)
    .with_seccomp(...)
    .with_capabilities(...)  // Drop all capabilities
    .build()?;
```

**Provides**:
- ✅ Maximum isolation
- ✅ OCI compatibility

**Issue**: More complex, slower startup

---

## RECOMMENDED APPROACH

### Phase 1: Get It Working (Now)
```toml
[dependencies]
hakoniwa = "0.1"  # For sandboxing
```

```rust
// Simple but isolated
use hakoniwa::Sandbox;

pub fn execute_task(task: &Task, sandbox_dir: &Path) -> Result<()> {
    Sandbox::new()
        .mount("/work", sandbox_dir)
        .unshare_user()
        .unshare_pid()
        .unshare_mount()
        .env("WORKDIR", "/work")
        .run(&["bash", "-c", &task.script])?;

    Ok(())
}
```

**Benefits**:
- ✅ Real isolation immediately
- ✅ 10 lines of code
- ✅ No bubblewrap binary dependency
- ✅ Works for unprivileged users

### Phase 2: Add Resource Limits
```rust
Sandbox::new()
    .mount("/work", sandbox_dir)
    .mount("/work/recipe-sysroot", sysroot_dir)  // Dependencies
    .unshare_user()
    .unshare_pid()
    .unshare_mount()
    .cpu_quota(max_cpus as f64)
    .memory_limit(max_memory)
    .run(&["bash", "-c", &task.script])?;
```

### Phase 3: Add Task Orchestration
```toml
[dependencies]
fn_graph = "0.11"  # Or async-jobs
```

```rust
use fn_graph::FnGraph;

let mut graph = FnGraph::new();

for (task_id, task) in &task_graph.tasks {
    graph.add_fn(
        task_id.clone(),
        task.depends_on.clone(),
        move |_ctx| {
            // Check cache
            if let Some(cached) = artifact_cache.get(&task.signature) {
                return Ok(cached);
            }

            // Execute in sandbox
            execute_task_in_sandbox(&task)?;

            // Collect outputs
            let artifact = collect_outputs(&task)?;
            artifact_cache.put(&task.signature, artifact);

            Ok(())
        },
    );
}

// Execute with concurrency limit
graph.run_parallel(max_parallel)?;
```

### Phase 4: Add Remote Caching
```rust
// Check remote cache first
if let Some(artifact) = remote_cache.get_action_result(&task.signature).await? {
    // Download from remote
    download_artifact(&artifact).await?;
    return Ok(CacheHit);
}

// Execute task
execute_task_in_sandbox(&task)?;

// Upload to remote cache
let artifact = collect_outputs(&task)?;
remote_cache.upload_action_result(&task.signature, &artifact).await?;
```

---

## CODE SIZE ESTIMATE

Using existing crates vs building everything:

| Component | Build Own | Use Crate | Lines Saved |
|-----------|-----------|-----------|-------------|
| Sandboxing | ~500 lines | ~10 lines (hakoniwa) | **490 lines** |
| Task Graph | ~300 lines | ~50 lines (fn_graph) | **250 lines** |
| Remote Cache Client | ~300 lines | ~300 lines (have to build) | 0 |
| Sysroot Assembly | ~200 lines | ~200 lines (have to build) | 0 |
| **TOTAL** | ~1300 lines | ~560 lines | **740 lines** |

**Recommendation**: Use existing crates for **sandboxing** and **task orchestration**, build **BitBake-specific** components ourselves.

---

## SUMMARY

### ✅ USE EXISTING CRATES:
1. **hakoniwa** or **bastille** - Process sandboxing
2. **fn_graph** or **async-jobs** - Task dependency graph
3. **nix** - Low-level primitives
4. **tonic** + Bazel Remote API protos - Remote caching protocol

### 🔨 BUILD OURSELVES:
1. BitBake recipe parsing & override resolution
2. Sysroot assembly with hardlinks
3. Task signature computation
4. Artifact collection & caching layer
5. Wave-based execution coordinator

### 🎯 ISOLATION STRATEGY:
Start with **Level 2** (namespaces + network isolation):
- Filesystem isolation via mount namespace
- Process isolation via PID namespace
- User namespace for unprivileged execution
- Network namespace (selectively enabled for do_fetch)

**This gives 90% of isolation benefits with minimal complexity.**

Later add **Level 3** (resource limits) when needed for production.

**Level 4** (seccomp) and **Level 5** (full OCI) are optional for extra hardening.
