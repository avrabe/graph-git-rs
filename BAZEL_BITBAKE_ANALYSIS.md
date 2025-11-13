# Bazel-Inspired BitBake Replacement - Deep Analysis & Design

## Executive Summary

This document presents a comprehensive analysis and design for a next-generation BitBake replacement that combines:
- **BitBake's recipe-based build system** for embedded Linux
- **Bazel's hermetic build approach** for reproducibility and caching
- **Modern Rust implementation** for safety, performance, and maintainability

---

## 1. Current State Analysis

### 1.1 convenient-bitbake Parser Status

**✅ Completed Capabilities:**
- **Full Recipe Parsing**: Can parse .bb, .bbappend, .inc files
- **Variable Resolution**: Handles ${VAR} expansion, overrides (:append, :prepend, :remove)
- **Task Dependencies**: Parses addtask statements and task[depends] flags
- **Recipe Graph**: ID-based graph structure (RecipeGraph) with topological sorting
- **Python Block Parsing**: Can extract and analyze Python functions
- **Include Resolution**: Handles include/require directives
- **Class Inheritance**: Understands inherit statements
- **Dependency Extraction**: DEPENDS, RDEPENDS, PROVIDES, RPROVIDES

**📊 Architecture Strengths:**
- Clean separation: lexer → parser → CST → graph
- Rowan-based resilient parsing (no panics on malformed input)
- Flat, ID-based graph (inspired by rustc/LLVM IR)
- Comprehensive test coverage

**❌ Missing for Execution:**
- No actual task executor
- No work directory management
- No sandboxing
- No caching layer
- No download/fetch implementation
- No sysroot/staging management

---

## 2. BitBake Work Directory Structure

### 2.1 Standard BitBake Directories

```
${TMPDIR}/
├── deploy/                    # Final build artifacts
│   ├── images/               # Bootable images
│   ├── ipk/                  # IPK packages
│   ├── rpm/                  # RPM packages
│   └── deb/                  # DEB packages
│
├── sysroots/                 # Cross-compilation sysroots
│   ├── x86_64-linux/        # Build host sysroot
│   └── arm-poky-linux-gnueabi/  # Target sysroot
│
├── sysroots-components/      # Individual recipe sysroot contributions
│   ├── x86_64/
│   │   ├── glibc/
│   │   └── openssl/
│   └── arm/
│       └── ...
│
├── work/                     # Per-recipe build directories
│   ├── core2-64-poky-linux/
│   │   ├── glibc/
│   │   │   └── 2.37-r0/
│   │   │       ├── temp/    # Task logs, run scripts
│   │   │       ├── image/   # ${D} - installation staging
│   │   │       ├── package/ # Split packages
│   │   │       ├── packages-split/
│   │   │       ├── git/     # ${S} - source directory
│   │   │       └── build/   # ${B} - build directory
│   │   └── linux-yocto/
│   └── x86_64-linux/        # Native recipes
│       └── make-native/
│
├── stamps/                   # Task completion markers
│   └── core2-64-poky-linux/
│       └── glibc/
│           └── 2.37-r0/
│               ├── do_fetch
│               ├── do_unpack.sigdata
│               ├── do_configure
│               └── ...
│
├── cache/                    # Signature cache, parsing cache
│   ├── bb_codeparser.dat
│   └── bb_persist_data.sqlite3
│
└── pkgdata/                  # Package metadata
    └── core2-64-poky-linux/
        └── runtime/
```

### 2.2 Key BitBake Variables

```python
# Source locations
S = "${WORKDIR}/${PN}-${PV}"           # Source directory
B = "${WORKDIR}/build"                  # Build directory
D = "${WORKDIR}/image"                  # Install destination (fake root)
WORKDIR = "${TMPDIR}/work/${MULTIMACH_TARGET_SYS}/${PN}/${EXTENDPE}${PV}-${PR}"

# Sysroot locations
STAGING_DIR = "${TMPDIR}/sysroots"
STAGING_DIR_HOST = "${STAGING_DIR}/${MACHINE}"
STAGING_DIR_NATIVE = "${STAGING_DIR}/${BUILD_ARCH}-${SDK_OS}"

# Package/Deploy
DEPLOY_DIR = "${TMPDIR}/deploy"
PKGDATA_DIR = "${TMPDIR}/pkgdata"
```

### 2.3 BitBake Task Execution Flow

1. **do_fetch**: Download sources (from SRC_URI)
2. **do_unpack**: Extract archives to ${WORKDIR}
3. **do_patch**: Apply patches
4. **do_configure**: Run ./configure or cmake
5. **do_compile**: Build software
6. **do_install**: Install to ${D}
7. **do_package**: Split into packages
8. **do_package_write_rpm**: Create RPM/DEB/IPK
9. **do_populate_sysroot**: Install headers/libs to sysroot
10. **do_deploy**: Copy artifacts to deploy dir

---

## 3. Bazel Execution Model

### 3.1 Bazel Directory Structure

```
<workspace>/
├── bazel-bin/               # Symlink to output artifacts (bazel-out/k8-fastbuild/bin)
├── bazel-out/              # All build outputs (hermetic)
│   ├── k8-fastbuild/       # Configuration-specific output
│   │   ├── bin/           # Executable outputs
│   │   ├── genfiles/      # Generated source files
│   │   └── testlogs/      # Test results
│   ├── host/              # Host tools
│   └── _tmp/              # Temp files
│
├── bazel-<workspace>/      # Symlink to execroot
├── bazel-execroot/
│   └── __main__/          # Execution root with source tree + generated files
│
└── bazel-sandbox/          # Sandboxed execution directories
    ├── <hash1>/
    │   ├── execroot/     # Isolated execution environment
    │   ├── inputs/       # Input files (symlinks/hardlinks)
    │   └── outputs/      # Output files
    └── <hash2>/
```

### 3.2 Bazel Key Concepts

#### 3.2.1 **Hermetic Builds**
- Each action runs in isolated sandbox
- Only declared inputs are visible
- Only declared outputs are collected
- No network access (except explicit "external" rules)
- No system dependencies (everything via toolchains)

#### 3.2.2 **Content-Addressable Cache**
```
Inputs: {source.c: hash1, compiler: hash2, flags: "-O2"}
  ↓
Action Key: hash(inputs + command)
  ↓
Cache Lookup: action_cache[action_key] = output_hash
  ↓
If hit: Skip execution, restore from cache
If miss: Execute and cache result
```

#### 3.2.3 **Sandboxing Strategies**

**Linux:**
- User namespaces (preferred)
- Hardlink forest + overlayFS
- Docker containers (fallback)

**Features:**
- Private mount namespace
- Private PID namespace (optional)
- Network namespace (disabled by default)
- Writable /tmp per action
- Read-only inputs

#### 3.2.4 **Remote Execution (Optional)**
- gRPC protocol (Remote Execution API v2)
- Can offload to remote workers
- Shared cache across team/CI

---

## 4. Hybrid Design: "Bitzel" (Working Name)

### 4.1 Core Philosophy

**Combine:**
1. **BitBake's Recipe Model**: Keep .bb files, variables, tasks
2. **Bazel's Execution Model**: Hermetic, cached, sandboxed
3. **Rust Implementation**: Type-safe, fast, maintainable

**Keep from BitBake:**
- Recipe syntax (.bb files)
- Variable system (DEPENDS, SRC_URI, etc.)
- Task dependencies
- Class inheritance
- Package splitting

**Adopt from Bazel:**
- Content-addressable storage
- Hermetic sandboxing
- Incremental builds via input hashing
- Remote execution capability
- Structured output tree

### 4.2 Directory Structure (Bitzel)

```
<project>/
├── bitzel-out/                      # All outputs (like bazel-out)
│   ├── cache/                       # Content-addressable cache
│   │   ├── cas/                    # Content-Addressable Storage
│   │   │   ├── sha256/
│   │   │   │   ├── ab/cd/ef.../   # Object storage by hash
│   │   │   │   └── ...
│   │   │   └── index.db           # SQLite index
│   │   ├── action-cache/           # Action → Output hash mapping
│   │   └── download-cache/         # SRC_URI downloads
│   │
│   ├── sysroot/                    # Merged sysroot (read-only)
│   │   ├── target-arm/
│   │   └── native-x86_64/
│   │
│   ├── work/                       # Per-recipe work dirs (ephemeral)
│   │   └── glibc-2.37-r0/
│   │       └── sandbox-<hash>/
│   │           ├── inputs/        # Mounted inputs (RO)
│   │           ├── work/          # Writable work area
│   │           ├── image/         # ${D} installation staging
│   │           └── outputs/       # Declared outputs
│   │
│   ├── deploy/                     # Final artifacts
│   │   ├── images/
│   │   └── packages/
│   │
│   └── logs/                       # Task logs, signatures
│       └── glibc-2.37-r0/
│           ├── do_fetch.log
│           ├── do_compile.log
│           └── do_compile.signature
│
└── bitzel-execroot/               # Execution root (like Bazel's execroot)
    ├── recipes/                   # Recipe source tree (symlinks)
    └── generated/                 # Generated files (e.g., expanded recipes)
```

### 4.3 Key Data Structures

#### 4.3.1 Task Signature (Input Hash)
```rust
#[derive(Serialize, Deserialize)]
struct TaskSignature {
    /// Task name
    task: String,

    /// Recipe identifier
    recipe: RecipeId,

    /// Input file hashes
    input_files: HashMap<PathBuf, ContentHash>,

    /// Dependency task signatures (recursive)
    dep_signatures: Vec<TaskSignature>,

    /// Environment variables
    env_vars: HashMap<String, String>,

    /// Task implementation hash (do_compile code)
    task_code_hash: ContentHash,

    /// Computed signature
    signature: ContentHash,
}
```

#### 4.3.2 Sandbox Spec
```rust
struct SandboxSpec {
    /// Read-only inputs (host path → sandbox path)
    ro_inputs: Vec<(PathBuf, PathBuf)>,

    /// Writable directories
    rw_dirs: Vec<PathBuf>,

    /// Output files/dirs to collect
    outputs: Vec<PathBuf>,

    /// Environment variables
    env: HashMap<String, String>,

    /// Command to execute
    command: Vec<String>,

    /// Working directory
    cwd: PathBuf,
}
```

#### 4.3.3 Content-Addressable Store
```rust
trait ContentStore {
    /// Store content, return hash
    fn put(&mut self, content: &[u8]) -> Result<ContentHash>;

    /// Retrieve content by hash
    fn get(&self, hash: &ContentHash) -> Result<Vec<u8>>;

    /// Check if hash exists
    fn contains(&self, hash: &ContentHash) -> bool;

    /// Store file
    fn put_file(&mut self, path: &Path) -> Result<ContentHash>;

    /// Restore file from hash
    fn get_file(&self, hash: &ContentHash, dest: &Path) -> Result<()>;
}
```

### 4.4 Task Execution Flow

```
1. Parse Recipe
   ├─ Load .bb file
   ├─ Resolve variables
   ├─ Expand ${VAR} references
   └─ Build task DAG

2. Compute Task Signature
   ├─ Hash input files (SRC_URI content)
   ├─ Hash dependency task outputs
   ├─ Hash environment vars (CC, CFLAGS, etc.)
   ├─ Hash task code (do_compile function)
   └─ signature = hash(all_above)

3. Check Cache
   ├─ lookup action_cache[signature]
   ├─ If HIT: restore outputs from CAS → skip execution
   └─ If MISS: proceed to execution

4. Prepare Sandbox
   ├─ Create sandbox dir: work/<recipe>/sandbox-<hash>
   ├─ Mount inputs (RO): source files, dependencies
   ├─ Create writable dirs: work/, image/, temp/
   ├─ Inject environment variables
   └─ Set up minimal PATH

5. Execute Task
   ├─ Run in sandbox: `bash -c "do_compile"`
   ├─ Capture stdout/stderr → logs/
   ├─ Monitor for violations (network access, /proc, etc.)
   └─ Check exit code

6. Collect Outputs
   ├─ Scan declared output paths
   ├─ Hash each output file
   ├─ Store in CAS: cas/sha256/ab/cd/...
   └─ Record action_cache[signature] = [output_hashes]

7. Update Sysroot (if do_populate_sysroot)
   ├─ Extract headers/libs from outputs
   ├─ Install to sysroot/target-<arch>/
   └─ Update sysroot index

8. Generate Stamp
   ├─ Create stamp file: stamps/<recipe>/do_task
   └─ Store signature in stamp for debugging
```

---

## 5. Implementation Phases

### Phase 1: Core Infrastructure (Week 1-2)
```rust
// convenient-bitbake/src/executor/mod.rs

pub struct TaskExecutor {
    cas: ContentAddressableStore,
    action_cache: ActionCache,
    sandbox: SandboxManager,
}

impl TaskExecutor {
    /// Execute a task with caching
    pub fn execute_task(
        &mut self,
        recipe: &Recipe,
        task: &TaskNode,
        deps: &[TaskOutput],
    ) -> Result<TaskOutput> {
        // 1. Compute signature
        let sig = self.compute_signature(recipe, task, deps)?;

        // 2. Check cache
        if let Some(cached) = self.action_cache.get(&sig) {
            return self.restore_from_cache(cached);
        }

        // 3. Prepare sandbox
        let sandbox = self.sandbox.create_sandbox(recipe, task, deps)?;

        // 4. Execute
        let result = sandbox.run_task(task)?;

        // 5. Cache result
        self.store_outputs(&sig, &result)?;

        Ok(result)
    }
}
```

### Phase 2: Sandboxing (Week 3-4)
```rust
// convenient-bitbake/src/sandbox/linux.rs

pub struct LinuxSandbox {
    root: PathBuf,
    spec: SandboxSpec,
}

impl LinuxSandbox {
    /// Create sandbox using Linux namespaces
    pub fn create(spec: SandboxSpec) -> Result<Self> {
        let root = PathBuf::from(format!("/tmp/bitzel-sandbox-{}",
            uuid::Uuid::new_v4()));

        // Create namespace with:
        // - Mount namespace (private /tmp, bind mounts)
        // - PID namespace (isolated process tree)
        // - User namespace (uid/gid mapping)
        // - Network namespace (disabled by default)

        unistd::unshare(
            CloneFlags::CLONE_NEWNS |
            CloneFlags::CLONE_NEWPID |
            CloneFlags::CLONE_NEWUSER
        )?;

        // Mount inputs read-only
        for (host_path, sandbox_path) in &spec.ro_inputs {
            mount_bind_ro(host_path, &root.join(sandbox_path))?;
        }

        // Create writable work dir
        for work_dir in &spec.rw_dirs {
            std::fs::create_dir_all(root.join(work_dir))?;
        }

        Ok(Self { root, spec })
    }

    /// Execute command in sandbox
    pub fn run(&self, cmd: &[String]) -> Result<ExitStatus> {
        // Spawn child process in namespace
        // Change root to sandbox root
        // Set environment
        // Exec command
        todo!()
    }
}
```

### Phase 3: Caching Layer (Week 5)
```rust
// convenient-bitbake/src/cache/cas.rs

pub struct ContentAddressableStore {
    root: PathBuf,
    db: sled::Db,  // Fast embedded DB for index
}

impl ContentAddressableStore {
    /// Store content and return hash
    pub fn put(&mut self, content: &[u8]) -> Result<ContentHash> {
        let hash = ContentHash::sha256(content);
        let path = self.hash_to_path(&hash);

        // Create parent dirs
        std::fs::create_dir_all(path.parent().unwrap())?;

        // Write atomically
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, &path)?;

        // Index
        self.db.insert(hash.as_bytes(), path.as_os_str().as_bytes())?;

        Ok(hash)
    }

    /// Map hash to filesystem path
    fn hash_to_path(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.to_hex();
        self.root
            .join("sha256")
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex)
    }
}
```

### Phase 4: Task Scheduler (Week 6)
- Parallel task execution
- Dependency-aware scheduling
- Resource limits (CPU, memory)
- Progress tracking

### Phase 5: Fetcher (Week 7)
- SRC_URI downloaders (http, https, git)
- Integrity checking (SRC_URI[sha256sum])
- Mirror fallback
- Download cache

### Phase 6: Bootstrap Integration (Week 8)
- Test with simple recipes (base-files, busybox)
- Integrate with Yocto/Poky
- Document migration path

---

## 6. Proof of Concept Plan

### 6.1 POC Scope
Build **2 bootstrap packages** with full sandboxing & caching:
1. **base-files** (simple, no compilation)
2. **busybox** (C compilation, cross-compilation)

### 6.2 Success Criteria
✅ Parse .bb files for both recipes
✅ Compute task signatures
✅ Execute do_fetch (download sources)
✅ Execute do_unpack in sandbox
✅ Execute do_compile in sandbox (busybox only)
✅ Execute do_install to ${D}
✅ Cache outputs in CAS
✅ Re-run builds → cache hit (< 1s rebuild)
✅ Modify source → cache miss → rebuild only affected

### 6.3 Directory Structure for POC
```
/tmp/bitzel-poc/
├── bitzel-out/
│   ├── cache/
│   │   └── cas/sha256/
│   ├── work/
│   │   ├── base-files-3.0/
│   │   └── busybox-1.36.1/
│   └── deploy/
├── recipes/
│   ├── base-files_3.0.bb
│   └── busybox_1.36.1.bb
└── logs/
```

---

## 7. Testing Strategy

### 7.1 Unit Tests
- TaskSignature computation
- CAS put/get operations
- Sandbox mount setup
- Cache hit/miss logic

### 7.2 Integration Tests
- End-to-end recipe build
- Cache persistence across runs
- Parallel task execution
- Error handling (network failures, compilation errors)

### 7.3 Real-World Validation
- Bootstrap Yocto minimal image (core-image-minimal)
- Compare output with bitbake
- Benchmark build times
- Measure cache hit rate

---

## 8. Next Steps

1. **TODAY**: Implement Phase 1 (Core Infrastructure)
   - Basic TaskExecutor struct
   - Skeleton SandboxManager
   - Simple in-memory ActionCache

2. **Test with base-files**:
   - Hardcode do_install task
   - Create sandbox, run commands
   - Verify outputs

3. **Add CAS**:
   - Implement file-based CAS
   - Store/retrieve task outputs
   - Verify cache hits

4. **Add busybox**:
   - Download busybox source
   - Cross-compile in sandbox
   - Cache compiled binaries

5. **Iterate & Refine**:
   - Add logging
   - Improve error messages
   - Optimize performance

---

## 9. Open Questions

1. **Sysroot Management**: How to handle incremental sysroot updates?
2. **Remote Execution**: Should we implement gRPC API immediately?
3. **Package Splitting**: How to integrate do_package task?
4. **Reproducibility**: Time stamps, user/group IDs in archives?
5. **BitBake Compatibility**: Should we parse existing stamps for migration?

---

## 10. References

### BitBake Documentation
- BitBake User Manual: https://docs.yoctoproject.org/bitbake/
- Yocto Project Mega Manual
- `lib/bb/` source code in poky repository

### Bazel Documentation
- Bazel Remote Execution API v2
- `src/main/java/com/google/devtools/build/lib/sandbox/`
- Remote Execution API: https://github.com/bazelbuild/remote-apis

### Implementation References
- Rust sandboxing: `nix` crate for namespaces
- Content-addressable storage: `sled` embedded DB
- Task scheduling: `tokio` async runtime
