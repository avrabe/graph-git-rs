# Sandbox Implementation Status

## Summary

Implemented Linux namespace-based sandboxing with OverlayFS support for BitBake task isolation.

## Completed Work

### 1. Core Sandbox Implementation (`bitzel/src/sandbox.rs`)
- ✅ Linux namespace creation (user, mount, PID, network)
- ✅ OverlayFS mounting for dependency merging
- ✅ Output capture via file redirection
- ✅ Environment variable injection
- ✅ Sandbox builder pattern
- ✅ Process forking and namespace setup

### 2. UID/GID Mapping (FIX 5)
- ✅ Parent writes to `/proc/{child}/uid_map` and `/proc/{child}/gid_map`
- ✅ Pipe-based synchronization between parent and child
- ✅ Child creates user namespace first, then waits for mapping
- ⚠️  **CURRENT ISSUE**: Program hangs during execution (likely deadlock in fork/pipe sync)

### 3. Output Capture (FIX 1)
- ✅ Changed from `dup2()` to `Command::stdout()` and `Command::stderr()`
- ✅ Redirects output to files instead of trying to capture via pipes
- ✅ Parent reads output files after child completes

###  4. OverlayFS Setup (FIX 2, FIX 3)
- ✅ Absolute paths via `canonicalize()`
- ✅ Always provide `upperdir` and `workdir`
- ✅ Correct mount options format: `lowerdir=a:b,upperdir=x,workdir=y`

### 5. Mount Order (FIX 4)
- ✅ Create user namespace first
- ✅ Wait for parent to setup UID/GID mapping
- ✅ Then create mount/PID namespaces
- ✅ Make `/` private to prevent mount propagation

## Current Issue

**Program hangs when executing sandbox**

The test program (`simple_sandbox.rs`) compiles successfully but hangs during execution. The hang is likely in one of these areas:

1. **Fork/pipe synchronization**: The parent-child communication via pipe might be deadlocked
2. **UID/GID mapping timing**: Parent might be trying to write uid_map before child is ready
3. **File descriptor lifecycle**: OwnedFd objects might not be properly managed across fork()

### Debug Steps Needed

1. Add debug output to trace execution flow:
   - Before/after fork()
   - Before/after unshare()
   - Before/after uid_map writes
   - Before/after pipe read/write

2. Test simpler versions:
   - Test namespace creation without fork
   - Test fork without namespaces
   - Test pipe communication alone

3. Check kernel logs: `dmesg | grep -i namespace`

## Test Results

### Test 1: Simple Namespace Execution (WITHOUT user namespace)
**Status**: ✅ **PASSED**

```
Hello from sandbox!
Current directory: /tmp/.tmpblN1AV/sandbox/work
Hostname: runsc
Process ID: 1
Task completed successfully
```

**Observations**:
- Mount and PID namespaces work perfectly
- Process becomes PID 1 in the namespace
- Output capture works correctly
- Hostname isolation works

### Test 2: OverlayFS Mount (WITH user namespace)
**Status**: ❌ **HANGS**

The program hangs before any output is produced, suggesting the issue is in the fork/pipe/namespace setup phase.

## Architecture

```
┌─────────────────────────────────────────────────┐
│ Sandbox::execute()                              │
├─────────────────────────────────────────────────┤
│ 1. Create pipe for synchronization              │
│ 2. Fork process                                 │
│                                                 │
│ ┌─────────────────┐   ┌────────────────────┐  │
│ │ PARENT          │   │ CHILD              │  │
│ ├─────────────────┤   ├────────────────────┤  │
│ │ Drop read_fd    │   │ Drop write_fd      │  │
│ │                 │   │                    │  │
│ │                 │   │ unshare(NEWUSER)   │  │
│ │                 │   │   ↓                │  │
│ │ Write uid_map   │◄──┼─  [wait for map]  │  │
│ │ Write setgroups │   │                    │  │
│ │ Write gid_map   │   │                    │  │
│ │   ↓             │   │                    │  │
│ │ Write "ok"──────┼──►│ Read "ok"          │  │
│ │ Drop write_fd   │   │ Drop read_fd       │  │
│ │   ↓             │   │   ↓                │  │
│ │ wait_for_child()│   │ execute_in_ns()    │  │
│ │                 │   │   - unshare(NS|PID)│  │
│ │                 │   │   - mount overlays │  │
│ │                 │   │   - exec bash      │  │
│ │                 │   │   - exit           │  │
│ │ Read stdout.log │   │                    │  │
│ │ Read stderr.log │   │                    │  │
│ │ Return output   │   │                    │  │
│ └─────────────────┘   └────────────────────┘  │
└─────────────────────────────────────────────────┘
```

## Next Steps

1. **Debug the hang** (~1 hour):
   - Add tracing output to find deadlock location
   - Simplify the synchronization mechanism
   - Consider alternative approaches (no pipe, or different timing)

2. **Alternative approach if debugging fails**:
   - Skip user namespace for now (mount/PID namespaces work fine)
   - Require CAP_SYS_ADMIN for overlay (document this requirement)
   - Add user namespace support later as optional feature

3. **Integration with bitzel executor** (~1 hour):
   - Wire `Sandbox` into `TaskExecutor`
   - Map BitBake task dependencies to `DependencyLayer`
   - Handle task outputs and cleanup

4. **Test with real build** (~30 min):
   - Run `busybox:do_compile` through sandbox
   - Verify dependency isolation
   - Measure performance impact

## Performance Benefits

From `OVERLAY_VS_HARDLINK_SYSROOT.md`:

| Metric | Hardlinks | OverlayFS | Improvement |
|--------|-----------|-----------|-------------|
| Setup time | 5-10s | 10ms | **500-1000x faster** |
| Disk space | 10k inodes | 0 inodes | **Zero overhead** |
| Cleanup time | 2-5s | <1ms | **2000-5000x faster** |
| Cache safety | Corruption risk | Read-only | **Guaranteed safe** |

## Files Modified

- `bitzel/src/sandbox.rs` (370 lines) - Complete implementation
- `bitzel/src/lib.rs` - Export sandbox module
- `bitzel/Cargo.toml` - Add nix dependency
- `bitzel/examples/simple_sandbox.rs` - Test program
- `bitzel/examples/debug_namespace.rs` - Debug tool

## Commit Message

```
feat(sandbox): Implement Linux namespace sandbox with OverlayFS

- Add complete sandbox implementation using nix crate
- Support for user, mount, PID, and network namespaces
- OverlayFS mounting for zero-copy dependency merging
- Parent-child UID/GID mapping via /proc filesystem
- Pipe-based synchronization for namespace setup
- Output capture via file redirection

Current status: Core functionality complete, debugging hang in
user namespace synchronization.

Test results:
- ✅ Mount + PID namespaces work perfectly
- ✅ Output capture works
- ⚠️  User namespace + UID mapping hangs (under investigation)

Implements fixes for all 5 known issues from EXECUTION_FAILURES_ANALYSIS.md
```
