# Sandboxing in Bitzel

Bitzel provides secure task execution through platform-specific sandboxing mechanisms that provide maximum isolation without requiring root privileges.

## Overview

The sandboxing system is implemented in `convenient-bitbake/src/executor/sandbox_backend.rs` and provides three backends:

1. **Bubblewrap** (Linux) - RECOMMENDED ✅
2. **sandbox-exec** (macOS) - Built-in ✅
3. **Basic** (Fallback) - ⚠️ Development only

## Sandboxing Backends

### 1. Bubblewrap (Linux)

**Status**: ✅ RECOMMENDED - Production-ready

**Installation**:
```bash
# Debian/Ubuntu
sudo apt install bubblewrap

# Fedora/RHEL
sudo dnf install bubblewrap

# Arch Linux
sudo pacman -S bubblewrap
```

**Features**:
- ✅ User namespaces (no root required)
- ✅ PID namespace isolation
- ✅ Mount namespace isolation
- ✅ Network namespace isolation (optional)
- ✅ IPC namespace isolation
- ✅ UTS namespace isolation
- ✅ Read-only root filesystem
- ✅ Private /tmp and /dev
- ✅ Process isolation

**Security Model**:
```
┌──────────────────────────────────────┐
│  Host System (Unmodified)           │
│                                      │
│  ┌────────────────────────────────┐ │
│  │  Bubblewrap Container          │ │
│  │                                │ │
│  │  /usr, /bin, /lib (read-only) │ │
│  │  /tmp (private tmpfs)          │ │
│  │  /dev (minimal)                │ │
│  │  /work (task workspace)        │ │
│  │                                │ │
│  │  Task Process (isolated)       │ │
│  └────────────────────────────────┘ │
└──────────────────────────────────────┘
```

**Example Bubblewrap Command**:
```bash
bwrap \
  --unshare-all \       # Unshare all namespaces
  --share-net \         # Allow network
  --die-with-parent \   # Kill on parent exit
  --ro-bind /usr /usr \ # Read-only system dirs
  --ro-bind /bin /bin \
  --tmpfs /tmp \        # Private /tmp
  --dev /dev \          # Minimal /dev
  --proc /proc \        # /proc filesystem
  --bind /work /work \  # Task workspace
  --chdir /work \       # Working directory
  --clearenv \          # Clear environment
  --setenv PATH /usr/bin:/bin \
  -- bash -c "echo 'Sandboxed!'"
```

### 2. sandbox-exec (macOS)

**Status**: ✅ Built-in on macOS

**Installation**: None required - built into macOS

**Features**:
- ✅ File system access control
- ✅ Network restrictions
- ✅ IPC restrictions
- ✅ Process restrictions
- ✅ Profile-based security

**Security Model**:
Uses Apple's TrustedBSD Mandatory Access Control (MAC) framework to restrict process capabilities.

**Example Profile**:
```scheme
(version 1)
(debug deny)

; Deny everything by default
(deny default)

; Allow reading system files
(allow file-read*
    (subpath "/usr/lib")
    (subpath "/usr/bin")
    (subpath "/System/Library"))

; Allow work directory access
(allow file-read* file-write*
    (subpath "/path/to/work"))

; Allow basic process operations
(allow process-exec
    (subpath "/usr/bin")
    (subpath "/bin"))

; Allow networking
(allow network-outbound)
```

**Example Usage**:
```bash
sandbox-exec -f profile.sb bash -c "echo 'Sandboxed!'"
```

### 3. Basic (Fallback)

**Status**: ⚠️ **NO REAL ISOLATION** - Development only

**Features**:
- Directory-based workspace
- Environment variable isolation
- **NO** process isolation
- **NO** filesystem isolation
- **NO** network isolation

**Warning**: This backend provides no real security and should only be used for development and testing.

## File Generation and Preparation

Before task execution, Bitzel generates all necessary files:

### 1. Task Scripts

Each task gets a generated shell script with:
- BitBake environment variables (PN, WORKDIR, S, B, D)
- Actual task implementation code (from do_compile, do_install, etc.)
- Output marker files for verification

Example generated script:
```bash
#!/bin/bash
set -e

# BitBake environment
export PN="busybox"
export WORKDIR="/work"
export S="/work/src"
export B="/work/build"
export D="/work/outputs"

# Task implementation (from recipe)
cd ${S}
make ${PARALLEL_MAKE}

# Mark task complete
mkdir -p /work/outputs
echo 'completed' > /work/outputs/compile.done
```

### 2. Sandbox Workspace

For each task execution, Bitzel creates:

```
/sandbox-root/
  ├── work/           # Writable workspace
  │   ├── src/        # Source files
  │   ├── build/      # Build artifacts
  │   └── outputs/    # Task outputs
  └── sandbox.sb      # Sandbox profile (macOS only)
```

### 3. Input Files

Input files are mounted/copied into the sandbox:
- **Read-only inputs**: Symlinked when possible (fast)
- **Writable inputs**: Copied into workspace
- **System libraries**: Read-only bind mounts (Linux)

## Usage

### Automatic Backend Detection

```rust
use convenient_bitbake::{SandboxManager, SandboxSpec};

// Auto-detects best available backend
let manager = SandboxManager::new("/path/to/sandboxes")?;

let mut spec = SandboxSpec::new(vec!["echo 'Hello'".to_string()]);
spec.cwd = PathBuf::from("/work");
spec.rw_dirs.push(PathBuf::from("/work"));

let sandbox = manager.create_sandbox(spec)?;
let result = sandbox.execute()?;

assert!(result.success());
```

### Explicit Backend Selection

```rust
use convenient_bitbake::{SandboxManager, SandboxBackend};

// Force specific backend
let backend = SandboxBackend::Bubblewrap;
let manager = SandboxManager::with_backend("/path/to/sandboxes", backend)?;
```

## Security Guarantees

### Bubblewrap (Linux)

✅ **Process Isolation**: Tasks cannot see or affect other processes
✅ **Filesystem Isolation**: Read-only root, private writable workspace
✅ **Network Isolation**: Optional network namespace (can be disabled)
✅ **Resource Limits**: Can be combined with cgroups
✅ **No Privilege Escalation**: User namespaces, no SUID

### sandbox-exec (macOS)

✅ **Filesystem Restrictions**: Profile-based file access control
✅ **Network Restrictions**: Can limit outbound/inbound connections
✅ **Process Restrictions**: Limited process spawning
⚠️ **Weaker than Bubblewrap**: Less isolation than Linux namespaces

### Basic (Fallback)

❌ **No Process Isolation**: Can see and affect other processes
❌ **No Filesystem Isolation**: Full access to host filesystem
❌ **No Network Isolation**: Full network access
❌ **No Resource Limits**: No restrictions

## Testing

### Test on Linux with Bubblewrap

```bash
# Install bubblewrap
sudo apt install bubblewrap

# Run tests
cargo test --package convenient-bitbake sandbox_backend

# Test actual sandboxing
cargo test --package convenient-bitbake test_bubblewrap_available
```

### Test on macOS with sandbox-exec

```bash
# No installation needed

# Run tests
cargo test --package convenient-bitbake sandbox_backend

# Test actual sandboxing
cargo test --package convenient-bitbake test_sandbox_exec_available
```

## Performance

### Bubblewrap

- **Overhead**: ~10-50ms per task
- **Scalability**: Excellent (user namespaces)
- **No root required**: ✅

### sandbox-exec

- **Overhead**: ~5-20ms per task
- **Scalability**: Good
- **No root required**: ✅

### Basic

- **Overhead**: <1ms per task
- **Scalability**: Excellent
- **No root required**: ✅
- **Security**: ❌ None

## Troubleshooting

### Bubblewrap Not Found

```
Warning: Bubblewrap not found, falling back to basic sandbox
Install bubblewrap: apt install bubblewrap
```

**Solution**: Install bubblewrap for your distribution.

### User Namespaces Disabled

```
Error: Failed to execute bubblewrap: user namespaces are disabled
```

**Solution**: Enable user namespaces:
```bash
sudo sysctl -w kernel.unprivileged_userns_clone=1
```

Make permanent in `/etc/sysctl.d/99-userns.conf`:
```
kernel.unprivileged_userns_clone=1
```

### macOS Sandbox Profile Errors

```
Error: Sandbox profile denied operation
```

**Solution**: Check the sandbox profile in `.bitzel-cache/sandboxes/*/sandbox.sb` and adjust as needed.

## Future Enhancements

- [ ] Docker/Podman backend for maximum isolation
- [ ] cgroups integration for resource limits
- [ ] seccomp-bpf for syscall filtering
- [ ] Network namespace isolation (Linux)
- [ ] GPU isolation for CUDA tasks
- [ ] Remote execution (Bazel Remote Execution API)

## References

- [Bubblewrap Documentation](https://github.com/containers/bubblewrap)
- [Apple Sandbox Guide](https://reverse.put.as/wp-content/uploads/2011/09/Apple-Sandbox-Guide-v1.0.pdf)
- [Linux Namespaces](https://man7.org/linux/man-pages/man7/namespaces.7.html)
- [Bazel Remote Execution API](https://github.com/bazelbuild/remote-apis)
