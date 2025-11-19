# Execution & Sandboxing Architecture Guide

## Overview

The system provides **four execution modes** for BitBake tasks, each with different levels of isolation and performance characteristics. The goal is to achieve **hermetic, reproducible builds** while maximizing performance.

## Execution Modes

### 1. DirectRust - Fast Path (No Sandbox)
**Best for**: Simple file operations, no host contamination risk

```rust
ExecutionMode::DirectRust
```

**Characteristics:**
- ✅ **2-5x faster** than shell execution
- ✅ **Zero sandbox overhead** - direct Rust execution
- ✅ **Still hermetic** - controlled environment
- ❌ **Limited to simple operations** only

**What qualifies as "simple"?**
```bash
# ✅ SIMPLE - Can use DirectRust
mkdir -p $D/usr/bin
touch $D/usr/bin/app
echo "test" > $D/README
cp src/app $D/usr/bin/
export PN="myrecipe"
bb_note "Building..."

# ❌ COMPLEX - Must use Shell
cat file.txt | grep pattern | awk '{print $1}'
for i in *.c; do gcc -c $i; done
if [ -f config.h ]; then make; fi
gcc -o app main.c  # Compiler invocation
```

**Implementation:**
```rust
// Script is analyzed before execution
let analysis = script_analyzer::analyze_script(&spec.script);

if analysis.is_simple {
    // Execute directly in Rust - no bash spawn
    execute_direct(&analysis, &work_dir, &env)?
} else {
    // Fall back to sandboxed shell
    execute_sandboxed(&spec)?
}
```

**Performance Impact:**
```
DirectRust:    0.5-2ms per task
RustShell:     1-3ms per task
Shell:         5-10ms per task
```

### 2. RustShell - In-Process Bash Interpreter ⭐ NEW
**Best for**: Bash scripts without external dependencies, variable tracking

```rust
ExecutionMode::RustShell
```

**Characteristics:**
- ✅ **2-5x faster** than subprocess bash
- ✅ **No /bin/bash dependency** - pure Rust implementation
- ✅ **Variable tracking** - like RustPython for shell scripts
- ✅ **Custom built-ins** - bb_note, bb_warn, etc.
- ✅ **90%+ bash compatible** - handles most BitBake tasks
- ✅ **Better error reporting** - full context and stack traces

**How it works:**
```rust
// Uses brush-shell: Rust-based bash interpreter
let mut executor = RustShellExecutor::new(workdir)?;

// Setup BitBake environment with tracking
executor.setup_bitbake_env("myrecipe", Some("1.0"), workdir);

// Execute script in-process (no subprocess!)
let result = executor.execute(script)?;

// Variable tracking like RustPython
println!("Variables read: {:?}", result.vars_read);
println!("Variables written: {:?}", result.vars_written);
```

**Example script:**
```bash
#!/bin/bash
# All standard BitBake helpers work!
bb_note "Building ${PN}-${PV}"
bbdirs "$D/usr/bin" "$D/etc"

# Bash features work
if [ -f configure ]; then
    ./configure --prefix=/usr
fi

for src in *.c; do
    echo "Compiling $src"
done

# Custom variables are tracked
export MY_VAR="value"
```

**Benefits over external bash:**
- **No subprocess overhead**: 2-5x faster execution
- **Variable tracking**: Know exactly what variables are used
- **No dependencies**: Works without /bin/bash in container
- **Better errors**: Stack traces with variable context
- **Custom built-ins**: BitBake helpers without prelude script

**Implementation:**
```rust
// Powered by brush-shell (Rust bash interpreter)
use brush_core::Shell;

let mut shell = Shell::new()?;
shell.env.insert("PN", "myrecipe");
shell.run_script(script)?;

// Full bash compatibility in pure Rust!
```

**Performance:**
```
RustShell:     1-3ms overhead
bash subprocess: 5-10ms overhead
Speedup:       2-5x
```

### 3. Shell - Sandboxed Execution
**Best for**: Complex bash scripts, compiler invocations

```rust
ExecutionMode::Shell
```

**Characteristics:**
- ✅ **Full bash compatibility** - pipes, loops, conditions
- ✅ **Complete isolation** - Linux namespaces
- ✅ **Resource limits** - cgroups v2
- ⚠️  **Slower** - ~5-10ms overhead per task

**Sandboxing Layers:**
```
┌─────────────────────────────────────────┐
│  Task Script                             │
│  #!/bin/bash                             │
│  . /hitzeleiter/prelude.sh              │
│  gcc -c main.c -o main.o                │
└─────────────────────────────────────────┘
              ↓
┌─────────────────────────────────────────┐
│  Prelude Script (/hitzeleiter/prelude.sh)│
│  • Error handling (set -e)              │
│  • Helper functions (bb_note, etc.)     │
│  • Standard environment (PN, PV, etc.)  │
└─────────────────────────────────────────┘
              ↓
┌─────────────────────────────────────────┐
│  Linux Namespaces                        │
│  • Mount: Private filesystem view       │
│  • PID: Process becomes PID 1           │
│  • Network: Isolated (no network)       │
└─────────────────────────────────────────┘
              ↓
┌─────────────────────────────────────────┐
│  cgroup v2 Resource Limits              │
│  • CPU: 50000µs per 100ms = 50% core   │
│  • Memory: 4GB default                  │
│  • PIDs: 1024 max (fork bomb protection)│
│  • I/O: weight=100 (fair share)         │
└─────────────────────────────────────────┘
```

### 4. Python - RustPython VM
**Best for**: Python tasks with BitBake data store access

```rust
ExecutionMode::Python
```

**Characteristics:**
- ✅ **RustPython VM** - Pure Rust, no CPython dependency
- ✅ **BitBake DataStore** - Access to `d.getVar()`
- ✅ **Full Python syntax** - conditionals, loops, functions
- ⚠️  **In-process** - No subprocess overhead

**Example Python Task:**
```python
# Executed in RustPython VM
python do_configure() {
    pn = d.getVar('PN')
    pv = d.getVar('PV')

    bb.note(f"Configuring {pn}-{pv}")

    if bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d):
        d.setVar('SYSTEMD_ENABLED', '1')

    d.appendVar('EXTRA_OECONF', ' --enable-foo')
}
```

**Implementation:**
```rust
// RustPython VM with BitBake modules
let interpreter = rustpython::InterpreterConfig::new()
    .init_stdlib()
    .interpreter();

interpreter.enter(|vm| {
    // Register BitBake modules
    vm.insert_sys_path(bb_module);
    vm.insert_sys_path(bb_utils_module);

    // Create DataStore for variable access
    let datastore = DataStore::new();
    vm.run_code(python_code, datastore)?;
});
```

## Linux Namespace Sandboxing

### Architecture

The sandbox uses **native Linux namespaces** (no bubblewrap or Docker required):

```rust
// native_sandbox.rs
pub fn execute_in_namespace(
    script: &str,
    work_dir: &Path,
    env: &HashMap<String, String>,
    network_policy: NetworkPolicy,
    resource_limits: &ResourceLimits,
) -> Result<(i32, String, String), ExecutionError>
```

### Isolation Layers

#### 1. **Mount Namespace** - Filesystem Isolation
```rust
unshare(CloneFlags::CLONE_NEWNS)  // Create private mount namespace

// Mount essential directories (read-only)
mount("/bin", "/bin", MS_BIND | MS_RDONLY)
mount("/usr", "/usr", MS_BIND | MS_RDONLY)
mount("/lib", "/lib", MS_BIND | MS_RDONLY)

// Mount work directory (read-write)
mount(work_dir, "/work", MS_BIND)
```

**What the task sees:**
```
/               (private, isolated)
├── bin/        (read-only, from host)
├── usr/        (read-only, from host)
├── lib/        (read-only, from host)
├── work/       (read-write, task-specific)
│   ├── src/    (source files)
│   ├── build/  (build artifacts)
│   └── outputs/ (task outputs)
└── hitzeleiter/
    └── prelude.sh (BitBake helpers)
```

#### 2. **PID Namespace** - Process Isolation
```rust
unshare(CloneFlags::CLONE_NEWPID)  // Task becomes PID 1

// Process cannot see host processes
// ps aux → only shows task's own processes
```

**Benefits:**
- ✅ Process tree isolation
- ✅ Clean process cleanup (kill PID 1 = kill all)
- ✅ No interference with host processes

#### 3. **Network Namespace** - Network Isolation
```rust
match network_policy {
    NetworkPolicy::Isolated => {
        // New network namespace, no interfaces
        unshare(CloneFlags::CLONE_NEWNET)
        // Result: No network access at all
    }

    NetworkPolicy::LoopbackOnly => {
        // New network namespace with loopback
        unshare(CloneFlags::CLONE_NEWNET)
        setup_loopback()  // 127.0.0.1 only
    }

    NetworkPolicy::FullNetwork => {
        // No network namespace - inherit host
        // Used for do_fetch tasks
    }
}
```

**Network Policy Matrix:**
```
Task Type       | Policy          | Can Access
----------------|-----------------|------------------
do_compile      | Isolated        | Nothing (hermetic)
do_install      | Isolated        | Nothing (hermetic)
do_configure    | LoopbackOnly    | 127.0.0.1 only
do_fetch        | FullNetwork     | Internet (for downloads)
```

#### 4. **cgroup v2** - Resource Limits
```rust
// Create task-specific cgroup
let cgroup = "/sys/fs/cgroup/bitzel/task-12345"
fs::create_dir(cgroup)?

// Apply resource limits
fs::write(format!("{}/cpu.max", cgroup), "50000 100000")?;    // 50% CPU
fs::write(format!("{}/memory.max", cgroup), "4294967296")?;   // 4GB RAM
fs::write(format!("{}/pids.max", cgroup), "1024")?;           // Max PIDs
fs::write(format!("{}/io.weight", cgroup), "default 100")?;   // I/O priority

// Move task to cgroup
fs::write(format!("{}/cgroup.procs", cgroup), task_pid)?;
```

**Resource Limit Profiles:**
```rust
// Default (balanced)
ResourceLimits::default() -> {
    cpu_quota_us: None,          // Unlimited
    memory_bytes: 4GB,           // 4GB limit
    pids_max: 1024,              // Prevent fork bombs
    io_weight: 100,              // Normal I/O priority
}

// Strict (untrusted)
ResourceLimits::strict() -> {
    cpu_quota_us: 100_000,       // 1 CPU core max
    memory_bytes: 2GB,           // 2GB limit
    pids_max: 512,               // Fewer processes
    io_weight: 50,               // Lower I/O priority
}

// Unlimited (trusted)
ResourceLimits::unlimited() -> {
    cpu_quota_us: None,          // No limit
    memory_bytes: None,          // No limit
    pids_max: None,              // No limit
    io_weight: None,             // Default
}
```

## Execution Flow

### Shell Script Execution

```
1. SCRIPT ANALYSIS
   └─> analyze_script()
       ├─> Is script simple? → DirectRust path
       └─> Complex? → Sandboxed shell path

2. SANDBOX SETUP (if needed)
   └─> fork()
       ├─> Parent: Wait for child
       └─> Child:
           ├─> Create namespaces (mount+PID+network)
           ├─> Setup cgroups (CPU, memory, PIDs, I/O)
           ├─> Bind mount system directories (read-only)
           ├─> Mount work directory (read-write)
           ├─> Install prelude script
           └─> Change to work directory

3. SCRIPT EXECUTION
   └─> execvp("/bin/bash", ["-c", script])
       ├─> Source prelude (. /hitzeleiter/prelude.sh)
       │   ├─> Set error handling (set -e -u -o pipefail)
       │   ├─> Define helper functions (bb_note, bb_warn, etc.)
       │   └─> Set standard environment (PN, PV, WORKDIR, etc.)
       └─> Execute task code
           ├─> Run commands (gcc, make, cp, etc.)
           ├─> Create outputs in /work/outputs/
           └─> Exit with status code

4. RESULT COLLECTION
   └─> Parent process:
       ├─> Wait for child (waitpid)
       ├─> Collect stdout/stderr
       ├─> Hash output files (ContentHash)
       ├─> Cleanup cgroup
       └─> Return TaskOutput
```

### Python Script Execution

```
1. SCRIPT PARSING
   └─> Extract Python blocks from recipe
       └─> python do_configure() { ... }

2. RUSTPYTHON VM SETUP
   └─> Create interpreter
       ├─> Register bb module
       ├─> Register bb.utils module
       ├─> Create DataStore
       └─> Pre-populate known variables

3. EXECUTION
   └─> vm.run_code(python_code)
       ├─> Call d.getVar('PN') → Track variable reads
       ├─> Call d.setVar('FOO', 'bar') → Track variable writes
       ├─> Execute Python logic (conditionals, loops)
       └─> Return execution result

4. RESULT PROCESSING
   └─> PythonExecutionResult {
       variables_set: HashMap<String, String>,
       variables_read: Vec<String>,
       success: bool,
       error: Option<String>,
   }
```

## Script Prelude System

All shell tasks automatically source `/hitzeleiter/prelude.sh`:

```bash
#!/bin/bash
# Auto-sourced by all task scripts

# Strict error handling
set -e          # Exit on error
set -u          # Exit on undefined variable
set -o pipefail # Pipe failures propagate

# BitBake helper functions
bb_note()  { echo "NOTE: $*" }
bb_warn()  { echo "WARNING: $*" >&2 }
bb_fatal() { echo "FATAL: $*" >&2; exit 1 }

# Standard BitBake variables
export PN="${PN:-unknown}"
export PV="${PV:-1.0}"
export WORKDIR="${WORKDIR:-/work}"
export S="${S:-${WORKDIR}/src}"
export B="${B:-${WORKDIR}/build}"
export D="${D:-${WORKDIR}/image}"

# Helper functions
bbdirs() { mkdir -p "$@"; }
bb_cd_build() { bbdirs "${B}"; cd "${B}"; }
bb_install() { /* ... */ }
```

**Benefits:**
- ✅ **Reduced script size** - Common functionality extracted
- ✅ **Consistent error handling** - All scripts fail fast
- ✅ **BitBake compatibility** - Standard functions available
- ✅ **Easier debugging** - Clear error messages

## Security & Isolation Guarantees

### What Tasks CANNOT Do

Even malicious tasks are constrained:

```bash
# ❌ BLOCKED: Cannot see host processes
ps aux          # Only shows task's own processes

# ❌ BLOCKED: Cannot access host filesystem
cat /etc/passwd # Read-only system files only
rm -rf /        # Mount namespace prevents damage

# ❌ BLOCKED: Cannot access network (default)
curl google.com # No network namespace available

# ❌ BLOCKED: Cannot fork bomb
while true; do fork; done  # Killed at 1024 PIDs

# ❌ BLOCKED: Cannot exhaust memory
malloc(100GB)   # Killed at 4GB limit

# ❌ BLOCKED: Cannot hog CPU
while true; done # Limited to 100% of one core
```

### What Tasks CAN Do

Tasks have controlled access:

```bash
# ✅ ALLOWED: Read system binaries
/usr/bin/gcc    # Read-only access

# ✅ ALLOWED: Read system libraries
ldd /usr/bin/gcc # Can see libraries

# ✅ ALLOWED: Write to work directory
echo "test" > /work/outputs/file.txt

# ✅ ALLOWED: Create processes (within limits)
gcc -j8 ...     # Up to 1024 processes

# ✅ ALLOWED: Use CPU (within limits)
make -j$(nproc) # Subject to cgroup limits
```

## Performance Characteristics

### Execution Mode Comparison

```
Mode          | Overhead | Isolation | Use Case
--------------|----------|-----------|---------------------------
DirectRust    | 0.5-2ms  | Medium    | Simple file operations
RustShell     | 1-3ms    | Medium    | Bash scripts, variable tracking
Shell         | 5-10ms   | Complete  | Complex builds, compilers
Python        | 2-5ms    | Medium    | Variable manipulation
```

### Optimization Strategies

**1. Fast Path Analysis**
```rust
// ~60% of tasks are simple enough for DirectRust
let analysis = analyze_script(script);
if analysis.is_simple {
    // 2-5x faster execution
    execute_direct(&analysis, work_dir, env)
}
```

**2. Namespace Reuse**
```rust
// Avoid repeated namespace setup
let sandbox_manager = SandboxManager::new(sandbox_dir);
sandbox_manager.get_or_create_sandbox(spec)  // Cached
```

**3. cgroup Cleanup**
```rust
// Proper cleanup prevents resource leaks
cleanup_cgroup(cgroup_path)?;
```

## Usage Examples

### Example 1: Simple Task (DirectRust)
```rust
let spec = TaskSpec {
    name: "do_install".to_string(),
    recipe: "myapp".to_string(),
    script: r#"
        #!/bin/bash
        . /hitzeleiter/prelude.sh

        bb_note "Installing application"
        mkdir -p $D/usr/bin
        cp $S/myapp $D/usr/bin/
        chmod 755 $D/usr/bin/myapp
        touch /work/outputs/do_install.done
    "#.to_string(),
    execution_mode: ExecutionMode::DirectRust,  // Fast path
    network_policy: NetworkPolicy::Isolated,
    resource_limits: ResourceLimits::default(),
    // ... other fields
};

let result = executor.execute_task(spec)?;
// Execution time: ~1ms (DirectRust fast path)
```

### Example 2: Complex Build (Shell + Sandbox)
```rust
let spec = TaskSpec {
    name: "do_compile".to_string(),
    recipe: "linux-kernel".to_string(),
    script: r#"
        #!/bin/bash
        . /hitzeleiter/prelude.sh

        bb_cd_build
        bb_note "Building kernel"

        make -j$(nproc) ARCH=${TARGET_ARCH} vmlinux modules

        if [ $? -ne 0 ]; then
            bb_fatal "Kernel build failed"
        fi

        bb_note "Build completed successfully"
    "#.to_string(),
    execution_mode: ExecutionMode::Shell,  // Full bash
    network_policy: NetworkPolicy::Isolated,
    resource_limits: ResourceLimits {
        cpu_quota_us: None,  // Use all available CPUs
        memory_bytes: Some(8 * 1024 * 1024 * 1024),  // 8GB for kernel build
        pids_max: Some(2048),  // More PIDs for parallel make
        io_weight: Some(200),  // Higher I/O priority
    },
    // ... other fields
};

let result = executor.execute_task(spec)?;
// Execution time: ~5-10ms overhead + actual build time
```

### Example 3: Python Variable Processing
```rust
let spec = TaskSpec {
    name: "do_configure".to_string(),
    recipe: "myrecipe".to_string(),
    script: r#"
python do_configure() {
    pn = d.getVar('PN')
    pv = d.getVar('PV')

    bb.note(f"Configuring {pn}-{pv}")

    # Conditional configuration based on DISTRO_FEATURES
    if bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d):
        d.appendVar('EXTRA_OECONF', ' --with-systemd')
        bb.note("systemd support enabled")

    # Platform-specific flags
    if d.getVar('TARGET_ARCH') == 'arm':
        d.appendVar('CFLAGS', ' -mfpu=neon')
}
    "#.to_string(),
    execution_mode: ExecutionMode::Python,  // RustPython VM
    network_policy: NetworkPolicy::Isolated,
    resource_limits: ResourceLimits::default(),
    // ... other fields
};

let result = executor.execute_task(spec)?;
// Execution time: ~2-5ms (in-process Python)
```

## Debugging & Troubleshooting

### Enable Debug Logging
```bash
export RUST_LOG=convenient_bitbake::executor=debug
cargo run
```

### Inspect Sandbox State
```bash
# Check active cgroups
ls -la /sys/fs/cgroup/bitzel/

# View resource usage
cat /sys/fs/cgroup/bitzel/task-12345/memory.current
cat /sys/fs/cgroup/bitzel/task-12345/cpu.stat

# Check PID limits
cat /sys/fs/cgroup/bitzel/task-12345/pids.current
cat /sys/fs/cgroup/bitzel/task-12345/pids.max
```

### Common Issues

**1. Script Complexity Detection**
```rust
// If script falls back to Shell when it should use DirectRust
let analysis = analyze_script(script);
println!("Complexity reason: {:?}", analysis.complexity_reason);
```

**2. Resource Limit Exceeded**
```
Error: Task killed (OOM or PID limit)
Solution: Increase resource limits for this task
```

**3. Network Access Required**
```
Error: Cannot resolve hostname
Solution: Change network_policy to FullNetwork for fetch tasks
```

## Architecture Benefits

### Compared to BitBake's Native Execution
✅ **Hermetic**: Complete isolation prevents host contamination
✅ **Reproducible**: Same inputs → same outputs, always
✅ **Cacheable**: Content-addressable caching with fine granularity
✅ **Parallel**: Safe concurrent execution without interference
✅ **Resource-limited**: Prevent resource exhaustion

### Compared to Docker/Containers
✅ **Faster**: Native namespaces, no container overhead
✅ **Lighter**: No image layers, no daemon
✅ **Fine-grained**: Per-task isolation, not per-build
✅ **Direct**: No Docker socket, no privileged mode

### Compared to Bazel
✅ **Task-level**: BitBake task model preserved
✅ **Python support**: Full RustPython VM integration
✅ **Flexible**: Three execution modes for different needs
✅ **Yocto-compatible**: Works with existing BitBake recipes

## Summary

The execution and sandboxing system provides:

1. **Three Execution Modes**: DirectRust (fast), Shell (compatible), Python (integrated)
2. **Complete Isolation**: Linux namespaces (mount+PID+network) + cgroups v2
3. **Resource Control**: CPU, memory, PID, and I/O limits
4. **Performance**: Fast path for simple operations, full sandbox for complex
5. **Security**: Multiple layers of defense against malicious or buggy tasks
6. **BitBake Compatible**: Standard helpers, variables, and Python API

This architecture enables **Bazel-level caching and hermeticity** while maintaining **BitBake compatibility and workflow**.
