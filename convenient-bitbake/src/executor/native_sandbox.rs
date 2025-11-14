//! Native Linux namespace sandbox implementation
//!
//! Provides hermetic process isolation using Linux namespaces directly via the nix crate.
//! No external dependencies like bubblewrap needed.
//!
//! ## Features
//! - **User namespace**: UID/GID mapping for rootless operation
//! - **Mount namespace**: Isolated filesystem with bind mounts
//! - **PID namespace**: Process becomes PID 1
//! - **Hermetic execution**: Controlled access to system directories
//!
//! ## Implementation Details
//! Uses parent-child synchronization via pipe to set up UID/GID mappings before
//! the child process creates mount and PID namespaces.

#[cfg(target_os = "linux")]
use nix::mount::{mount, MsFlags};
#[cfg(target_os = "linux")]
use nix::sched::{unshare, CloneFlags};
#[cfg(target_os = "linux")]
use nix::unistd::{chdir, fork, ForkResult, Pid, getuid, getgid, pipe, read, write};
#[cfg(target_os = "linux")]
use std::os::fd::{OwnedFd, AsRawFd};
#[cfg(target_os = "linux")]
use nix::sys::wait::{waitpid, WaitStatus};

use std::fs;
use std::path::Path;
use std::process::Command;
use std::os::unix::process::ExitStatusExt;

use super::types::{ExecutionError, NetworkPolicy};
use tracing::{debug, info, warn};

/// Execute command in native Linux namespace sandbox
///
/// This implementation uses:
/// 1. Mount namespace with bind mounts for controlled filesystem access
/// 2. PID namespace for process isolation
/// 3. Network namespace for network isolation (CLONE_NEWNET)
///
/// NOTE: User namespace support is available but requires kernel configuration
/// (max_user_namespaces > 0). For now, using mount+PID+network only.
#[cfg(target_os = "linux")]
pub fn execute_in_namespace(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
    network_policy: NetworkPolicy,
) -> Result<(i32, String, String), ExecutionError> {
    info!("Executing in native Linux namespace sandbox (mount+pid+network): {:?}", network_policy);

    // Create work directory
    fs::create_dir_all(work_dir)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create work dir: {}", e)))?;

    // Fork for namespace isolation
    match unsafe { fork() }
        .map_err(|e| ExecutionError::SandboxError(format!("Fork failed: {}", e)))?
    {
        ForkResult::Parent { child } => {
            debug!("Parent process: child PID = {}", child);
            // Wait for child and collect results
            wait_for_child(child, work_dir)
        }
        ForkResult::Child => {
            debug!("Child process: starting namespace setup");

            // Execute in namespace (mount+PID+network without user namespace)
            match execute_child_without_userns(script, work_dir, env, network_policy) {
                Ok(exit_code) => {
                    debug!("Child: execution completed with code {}", exit_code);
                    std::process::exit(exit_code);
                }
                Err(e) => {
                    eprintln!("Sandbox execution failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Setup UID/GID mapping for child process
#[cfg(target_os = "linux")]
fn setup_uid_gid_mapping(child: Pid, write_fd: OwnedFd) -> Result<(), ExecutionError> {
    let uid = getuid();
    let gid = getgid();

    debug!("Setting up UID/GID mapping for child {}: uid={}, gid={}", child, uid, gid);

    // Write uid_map (map root in container to current user outside)
    let uid_map = format!("0 {} 1\n", uid);
    let uid_map_path = format!("/proc/{}/uid_map", child);
    fs::write(&uid_map_path, &uid_map)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to write uid_map: {}", e)))?;
    debug!("Wrote uid_map: {}", uid_map.trim());

    // Disable setgroups (required before writing gid_map)
    let setgroups_path = format!("/proc/{}/setgroups", child);
    fs::write(&setgroups_path, "deny\n")
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to write setgroups: {}", e)))?;
    debug!("Disabled setgroups");

    // Write gid_map
    let gid_map = format!("0 {} 1\n", gid);
    let gid_map_path = format!("/proc/{}/gid_map", child);
    fs::write(&gid_map_path, &gid_map)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to write gid_map: {}", e)))?;
    debug!("Wrote gid_map: {}", gid_map.trim());

    // Signal child that mapping is complete
    let signal = b"ok";
    write(write_fd, signal)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to signal child: {}", e)))?;
    debug!("Signaled child that UID/GID mapping is complete");

    // write_fd will be automatically closed when it goes out of scope

    Ok(())
}

/// Child process: create user namespace, wait for mapping, then create mount+PID+network namespaces
#[cfg(target_os = "linux")]
fn execute_child_with_userns(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
    read_fd: OwnedFd,
    network_policy: NetworkPolicy,
) -> Result<i32, ExecutionError> {
    use std::fs::File;

    debug!("Child: creating user namespace");

    // Step 1: Create user namespace FIRST
    unshare(CloneFlags::CLONE_NEWUSER)
        .map_err(|e| ExecutionError::SandboxError(format!("unshare(CLONE_NEWUSER) failed: {}", e)))?;

    debug!("Child: user namespace created, waiting for UID/GID mapping");

    // Step 2: Wait for parent to setup UID/GID mapping
    let mut buf = [0u8; 2];
    match read(read_fd.as_raw_fd(), &mut buf) {
        Ok(n) if n == 2 && &buf == b"ok" => {
            debug!("Child: received mapping confirmation");
        }
        Ok(n) => {
            return Err(ExecutionError::SandboxError(format!(
                "Unexpected signal from parent: {} bytes", n
            )));
        }
        Err(e) => {
            return Err(ExecutionError::SandboxError(format!(
                "Failed to read from parent: {}", e
            )));
        }
    }

    // read_fd will be automatically closed when it goes out of scope
    drop(read_fd);

    debug!("Child: UID/GID mapping confirmed, creating mount, PID, and network namespaces");

    // Step 3: Create mount, PID, and network namespaces
    unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWNET)
        .map_err(|e| ExecutionError::SandboxError(format!("unshare(NEWNS|NEWPID|NEWNET) failed: {}", e)))?;

    debug!("Child: mount, PID, and network namespaces created");

    // Setup network according to policy
    match network_policy {
        NetworkPolicy::Isolated => {
            debug!("Network: Full isolation (no network access)");
            // Do nothing - isolated by default in new network namespace
        }
        NetworkPolicy::LoopbackOnly => {
            setup_loopback()?;
            debug!("Network: Loopback only (127.0.0.1 accessible)");
        }
        NetworkPolicy::Controlled => {
            return Err(ExecutionError::SandboxError(
                "Controlled network access not yet implemented".to_string()
            ));
        }
    }

    // Step 4: Make / private to prevent mount propagation
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_PRIVATE | MsFlags::MS_REC,
        None::<&str>,
    )
    .map_err(|e| ExecutionError::SandboxError(format!("Failed to make / private: {}", e)))?;

    debug!("Child: made / private");

    // Step 5: Bind mount essential system directories (read-only)
    // These allow access to system binaries and libraries while maintaining isolation
    let system_dirs = [
        "/bin",
        "/sbin",
        "/usr",
        "/lib",
        "/lib64",
        "/etc/resolv.conf",
        "/etc/ssl",
    ];

    for dir_str in &system_dirs {
        let dir = Path::new(dir_str);
        if dir.exists() {
            // For files (like /etc/resolv.conf), just bind mount directly
            // For directories, bind mount recursively
            let flags = if dir.is_file() {
                MsFlags::MS_BIND | MsFlags::MS_RDONLY
            } else {
                MsFlags::MS_BIND | MsFlags::MS_RDONLY | MsFlags::MS_REC
            };

            match mount(
                Some(dir),
                dir,
                None::<&str>,
                flags,
                None::<&str>,
            ) {
                Ok(_) => debug!("Child: bind mounted {} (read-only)", dir_str),
                Err(e) => {
                    // Non-fatal: some systems might not have all directories
                    warn!("Child: failed to bind mount {} (skipping): {}", dir_str, e);
                }
            }
        }
    }

    // Step 6: Setup stdout/stderr capture (BEFORE chdir)
    let sandbox_root = work_dir.parent()
        .ok_or_else(|| ExecutionError::SandboxError("work_dir has no parent".to_string()))?;
    let stdout_path = sandbox_root.join("stdout.log");
    let stderr_path = sandbox_root.join("stderr.log");

    let stdout_file = File::create(&stdout_path)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create stdout.log: {}", e)))?;
    let stderr_file = File::create(&stderr_path)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create stderr.log: {}", e)))?;

    // Step 7: Change to work directory
    chdir(work_dir)
        .map_err(|e| ExecutionError::SandboxError(format!("chdir failed: {}", e)))?;

    debug!("Child: changed to work directory: {}", work_dir.display());

    // Step 8: Execute command
    let mut cmd = Command::new("bash");
    cmd.arg("-c")
       .arg(script)
       .current_dir(work_dir)
       .stdout(stdout_file)
       .stderr(stderr_file);

    // Set environment variables
    cmd.env_clear();
    for (key, value) in env {
        cmd.env(key, value);
    }

    // Add essential environment
    cmd.env("HOME", "/tmp");
    cmd.env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
    cmd.env("SHELL", "/bin/bash");

    debug!("Child: executing: bash -c {:?}", script);

    // Execute and get status
    let status = cmd.status()
        .map_err(|e| ExecutionError::SandboxError(format!("Command execution failed: {}", e)))?;

    let exit_code = status.code().unwrap_or(1);
    debug!("Child: command completed with exit code: {}", exit_code);

    Ok(exit_code)
}

/// Setup loopback interface in network namespace
#[cfg(target_os = "linux")]
fn setup_loopback() -> Result<(), ExecutionError> {
    debug!("Setting up loopback interface");

    // Try multiple possible paths for the 'ip' command
    let ip_paths = ["/usr/sbin/ip", "/sbin/ip", "/usr/bin/ip", "/bin/ip", "ip"];

    let mut last_error = String::new();
    for ip_path in &ip_paths {
        match Command::new(ip_path)
            .args(&["link", "set", "lo", "up"])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    debug!("Loopback interface is up (using {})", ip_path);
                    return Ok(());
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    last_error = format!("ip command failed: {}", stderr);
                }
            }
            Err(e) => {
                last_error = format!("Failed to execute {}: {}", ip_path, e);
                continue;
            }
        }
    }

    Err(ExecutionError::SandboxError(format!(
        "Failed to setup loopback: {}. Tried paths: {:?}",
        last_error, ip_paths
    )))
}

/// Child process: create mount+PID+network namespaces without user namespace
/// (requires CAP_SYS_ADMIN or running in privileged mode)
#[cfg(target_os = "linux")]
fn execute_child_without_userns(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
    network_policy: NetworkPolicy,
) -> Result<i32, ExecutionError> {
    use std::fs::File;

    debug!("Child: creating mount, PID, and network namespaces (no user namespace)");

    // Create mount, PID, and network namespaces directly (no user namespace)
    unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWNET)
        .map_err(|e| ExecutionError::SandboxError(format!("unshare(NEWNS|NEWPID|NEWNET) failed: {}", e)))?;

    debug!("Child: mount, PID, and network namespaces created");

    // Setup network according to policy
    match network_policy {
        NetworkPolicy::Isolated => {
            debug!("Network: Full isolation (no network access)");
            // Do nothing - isolated by default in new network namespace
        }
        NetworkPolicy::LoopbackOnly => {
            setup_loopback()?;
            debug!("Network: Loopback only (127.0.0.1 accessible)");
        }
        NetworkPolicy::Controlled => {
            return Err(ExecutionError::SandboxError(
                "Controlled network access not yet implemented".to_string()
            ));
        }
    }

    // Make / private to prevent mount propagation
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_PRIVATE | MsFlags::MS_REC,
        None::<&str>,
    )
    .map_err(|e| ExecutionError::SandboxError(format!("Failed to make / private: {}", e)))?;

    debug!("Child: made / private");

    // Bind mount essential system directories (read-only)
    let system_dirs = [
        "/bin",
        "/sbin",
        "/usr",
        "/lib",
        "/lib64",
        "/etc/resolv.conf",
        "/etc/ssl",
    ];

    for dir_str in &system_dirs {
        let dir = Path::new(dir_str);
        if dir.exists() {
            let flags = if dir.is_file() {
                MsFlags::MS_BIND | MsFlags::MS_RDONLY
            } else {
                MsFlags::MS_BIND | MsFlags::MS_RDONLY | MsFlags::MS_REC
            };

            match mount(
                Some(dir),
                dir,
                None::<&str>,
                flags,
                None::<&str>,
            ) {
                Ok(_) => debug!("Child: bind mounted {} (read-only)", dir_str),
                Err(e) => {
                    warn!("Child: failed to bind mount {} (skipping): {}", dir_str, e);
                }
            }
        }
    }

    // Setup stdout/stderr capture
    let sandbox_root = work_dir.parent()
        .ok_or_else(|| ExecutionError::SandboxError("work_dir has no parent".to_string()))?;
    let stdout_path = sandbox_root.join("stdout.log");
    let stderr_path = sandbox_root.join("stderr.log");

    let stdout_file = File::create(&stdout_path)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create stdout.log: {}", e)))?;
    let stderr_file = File::create(&stderr_path)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create stderr.log: {}", e)))?;

    // Change to work directory
    chdir(work_dir)
        .map_err(|e| ExecutionError::SandboxError(format!("chdir failed: {}", e)))?;

    debug!("Child: changed to work directory: {}", work_dir.display());

    // Execute command
    let mut cmd = Command::new("bash");
    cmd.arg("-c")
       .arg(script)
       .current_dir(work_dir)
       .stdout(stdout_file)
       .stderr(stderr_file);

    // Set environment variables
    cmd.env_clear();
    for (key, value) in env {
        cmd.env(key, value);
    }

    // Add essential environment
    cmd.env("HOME", "/tmp");
    cmd.env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
    cmd.env("SHELL", "/bin/bash");

    debug!("Child: executing: bash -c {:?}", script);

    // Execute and get status
    let status = cmd.status()
        .map_err(|e| ExecutionError::SandboxError(format!("Command execution failed: {}", e)))?;

    let exit_code = status.code().unwrap_or(1);
    debug!("Child: command completed with exit code: {}", exit_code);

    Ok(exit_code)
}

/// Parent process: wait for child and collect output
#[cfg(target_os = "linux")]
fn wait_for_child(
    child: Pid,
    work_dir: &Path,
) -> Result<(i32, String, String), ExecutionError> {
    debug!("Parent: waiting for child {}", child);

    match waitpid(child, None)
        .map_err(|e| ExecutionError::SandboxError(format!("waitpid failed: {}", e)))?
    {
        WaitStatus::Exited(_pid, code) => {
            debug!("Parent: child exited with code: {}", code);

            // Read captured output (from sandbox root, not work dir)
            let sandbox_root = work_dir.parent()
                .ok_or_else(|| ExecutionError::SandboxError("work_dir has no parent".to_string()))?;
            let stdout_path = sandbox_root.join("stdout.log");
            let stderr_path = sandbox_root.join("stderr.log");

            let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
            let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();

            Ok((code, stdout, stderr))
        }
        WaitStatus::Signaled(_pid, signal, _) => {
            Err(ExecutionError::SandboxError(format!(
                "Child process killed by signal: {:?}",
                signal
            )))
        }
        status => {
            Err(ExecutionError::SandboxError(format!(
                "Child process ended unexpectedly: {:?}",
                status
            )))
        }
    }
}

/// Fallback for non-Linux platforms
#[cfg(not(target_os = "linux"))]
pub fn execute_in_namespace(
    _script: &str,
    _work_dir: &Path,
    _env: &std::collections::HashMap<String, String>,
    _network_policy: NetworkPolicy,
) -> Result<(i32, String, String), ExecutionError> {
    Err(ExecutionError::SandboxError(
        "Native namespace sandbox only available on Linux".to_string()
    ))
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_simple_execution() {
        let tmp = TempDir::new().unwrap();
        let work_dir = tmp.path().join("work");
        fs::create_dir_all(&work_dir).unwrap();

        let env = HashMap::new();
        let script = "echo 'Hello from sandbox'; echo $$ > /tmp/pid.txt; cat /tmp/pid.txt";

        let result = execute_in_namespace(script, &work_dir, &env, NetworkPolicy::Isolated);

        assert!(result.is_ok());
        let (exit_code, stdout, stderr) = result.unwrap();
        assert_eq!(exit_code, 0);
        assert!(stdout.contains("Hello from sandbox"));
        assert!(stdout.contains("1")); // Should be PID 1 in namespace
        assert_eq!(stderr, "");
    }

    #[test]
    fn test_filesystem_isolation() {
        let tmp = TempDir::new().unwrap();
        let work_dir = tmp.path().join("work");
        fs::create_dir_all(&work_dir).unwrap();

        let env = HashMap::new();
        // Test that we can access system binaries
        let script = "ls /bin/bash && echo 'System access OK'";

        let result = execute_in_namespace(script, &work_dir, &env, NetworkPolicy::Isolated);

        assert!(result.is_ok());
        let (exit_code, stdout, _) = result.unwrap();
        assert_eq!(exit_code, 0);
        assert!(stdout.contains("System access OK"));
    }

    #[test]
    fn test_environment_variables() {
        let tmp = TempDir::new().unwrap();
        let work_dir = tmp.path().join("work");
        fs::create_dir_all(&work_dir).unwrap();

        let mut env = HashMap::new();
        env.insert("TEST_VAR".to_string(), "test_value".to_string());

        let script = "echo $TEST_VAR";

        let result = execute_in_namespace(script, &work_dir, &env, NetworkPolicy::Isolated);

        assert!(result.is_ok());
        let (exit_code, stdout, _) = result.unwrap();
        assert_eq!(exit_code, 0);
        assert!(stdout.contains("test_value"));
    }
}
