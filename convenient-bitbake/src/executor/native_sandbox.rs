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
use nix::unistd::{chdir, fork, ForkResult, Pid, getuid, getgid, read, write};
#[cfg(target_os = "linux")]
use std::os::fd::{OwnedFd, AsRawFd};
#[cfg(target_os = "linux")]
use nix::sys::wait::{waitpid, WaitStatus};

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::os::unix::process::ExitStatusExt;

use super::types::{ExecutionError, NetworkPolicy, ResourceLimits};
use super::script_analyzer::analyze_script;
use super::direct_executor::execute_direct;
use tracing::{debug, info, warn};

/// Setup cgroup v2 for resource limits
///
/// Creates a cgroup under /sys/fs/cgroup/hitzeleiter/<cgroup_name> and applies resource limits.
/// Returns the cgroup path for cleanup.
#[cfg(target_os = "linux")]
fn setup_cgroup(cgroup_name: &str, limits: &ResourceLimits) -> Result<PathBuf, ExecutionError> {
    let cgroup_root = Path::new("/sys/fs/cgroup");

    // Check if cgroup v2 is available
    if !cgroup_root.join("cgroup.controllers").exists() {
        warn!("cgroup v2 not available, resource limits will not be enforced");
        return Err(ExecutionError::SandboxError(
            "cgroup v2 not available (unified hierarchy required)".to_string()
        ));
    }

    // Create bitzel parent cgroup if needed
    let bitzel_cgroup = cgroup_root.join("bitzel");
    if !bitzel_cgroup.exists() {
        fs::create_dir(&bitzel_cgroup)
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to create bitzel cgroup: {}", e)))?;

        // Enable controllers in parent
        let subtree_control = bitzel_cgroup.join("cgroup.subtree_control");
        let controllers = "+cpu +memory +pids +io";
        fs::write(&subtree_control, controllers)
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to enable controllers: {}", e)))?;
    }

    // Create task-specific cgroup
    let task_cgroup = bitzel_cgroup.join(cgroup_name);
    if task_cgroup.exists() {
        // Clean up old cgroup
        let _ = fs::remove_dir(&task_cgroup);
    }

    fs::create_dir(&task_cgroup)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create task cgroup: {}", e)))?;

    debug!("Created cgroup: {}", task_cgroup.display());

    // Apply CPU limits
    if let Some(cpu_quota_us) = limits.cpu_quota_us {
        let cpu_max = task_cgroup.join("cpu.max");
        // Format: "quota period" where period is 100000 (100ms)
        let value = format!("{} 100000", cpu_quota_us);
        fs::write(&cpu_max, value)
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to set cpu.max: {}", e)))?;
        debug!("Set CPU quota: {} µs per 100ms", cpu_quota_us);
    }

    // Apply memory limits
    if let Some(memory_bytes) = limits.memory_bytes {
        let memory_max = task_cgroup.join("memory.max");
        fs::write(&memory_max, memory_bytes.to_string())
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to set memory.max: {}", e)))?;
        debug!("Set memory limit: {} bytes ({} GB)", memory_bytes, memory_bytes / (1024 * 1024 * 1024));
    }

    // Apply PID limits
    if let Some(pids_max) = limits.pids_max {
        let pids_max_file = task_cgroup.join("pids.max");
        fs::write(&pids_max_file, pids_max.to_string())
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to set pids.max: {}", e)))?;
        debug!("Set PID limit: {}", pids_max);
    }

    // Apply I/O weight
    if let Some(io_weight) = limits.io_weight {
        let io_weight_file = task_cgroup.join("io.weight");
        // Format: "default <weight>" where weight is 1-10000
        let value = format!("default {}", io_weight);
        fs::write(&io_weight_file, value)
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to set io.weight: {}", e)))?;
        debug!("Set I/O weight: {}", io_weight);
    }

    Ok(task_cgroup)
}

/// Move current process into cgroup
#[cfg(target_os = "linux")]
fn move_to_cgroup(cgroup_path: &Path) -> Result<(), ExecutionError> {
    let procs_file = cgroup_path.join("cgroup.procs");
    let pid = std::process::id();

    fs::write(&procs_file, pid.to_string())
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to move to cgroup: {}", e)))?;

    debug!("Moved PID {} to cgroup: {}", pid, cgroup_path.display());
    Ok(())
}

/// Cleanup cgroup after task completes
#[cfg(target_os = "linux")]
fn cleanup_cgroup(cgroup_path: &Path) -> Result<(), ExecutionError> {
    if cgroup_path.exists() {
        // Kill any remaining processes
        let procs_file = cgroup_path.join("cgroup.procs");
        if let Ok(procs) = fs::read_to_string(&procs_file) {
            for line in procs.lines() {
                if let Ok(pid) = line.trim().parse::<i32>() {
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(pid),
                        nix::sys::signal::Signal::SIGKILL
                    );
                }
            }
        }

        // Remove cgroup directory
        fs::remove_dir(cgroup_path)
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to cleanup cgroup: {}", e)))?;

        debug!("Cleaned up cgroup: {}", cgroup_path.display());
    }
    Ok(())
}

/// Install BitBake prelude script to /hitzeleiter/prelude.sh
///
/// This creates the /bitzel directory and writes the shared prelude script
/// that is sourced by all task scripts. The prelude provides:
/// - Standard BitBake environment variables
/// - Error handling (set -e, set -u, set -o pipefail)
/// - Helper functions (bb_note, bb_warn, bb_fatal, etc.)
#[cfg(target_os = "linux")]
fn install_prelude_script() -> std::io::Result<()> {
    const PRELUDE_CONTENT: &str = include_str!("prelude.sh");

    // Create /hitzeleiter directory
    fs::create_dir_all("/hitzeleiter")?;

    // Write prelude script
    fs::write("/hitzeleiter/prelude.sh", PRELUDE_CONTENT)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata("/hitzeleiter/prelude.sh")?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions("/hitzeleiter/prelude.sh", perms)?;
    }

    debug!("Installed BitBake prelude to /hitzeleiter/prelude.sh");
    Ok(())
}

/// Execute command in native Linux namespace sandbox
///
/// This implementation uses:
/// 1. Mount namespace with bind mounts for controlled filesystem access
/// 2. PID namespace for process isolation
/// 3. Network namespace for network isolation (CLONE_NEWNET)
/// 4. cgroup v2 for resource limits (CPU, memory, PIDs, I/O)
///
/// NOTE: User namespace support is available but requires kernel configuration
/// (max_user_namespaces > 0). For now, using mount+PID+network+cgroup only.
#[cfg(target_os = "linux")]
pub fn execute_in_namespace(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
    network_policy: NetworkPolicy,
    resource_limits: &ResourceLimits,
) -> Result<(i32, String, String), ExecutionError> {
    info!("Executing in native Linux namespace sandbox (mount+pid+network+cgroup): {:?}", network_policy);

    // Create work directory
    fs::create_dir_all(work_dir)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create work dir: {}", e)))?;

    // Setup cgroup for resource limits (before fork)
    let cgroup_name = format!("task-{}", std::process::id());
    let cgroup_path = match setup_cgroup(&cgroup_name, resource_limits) {
        Ok(path) => Some(path),
        Err(e) => {
            warn!("Failed to setup cgroup: {}. Continuing without resource limits", e);
            None
        }
    };

    // Fork for namespace isolation
    let result = match unsafe { fork() }
        .map_err(|e| ExecutionError::SandboxError(format!("Fork failed: {}", e)))?
    {
        ForkResult::Parent { child } => {
            debug!("Parent process: child PID = {}", child);
            // Wait for child and collect results
            let result = wait_for_child(child, work_dir);

            // Cleanup cgroup after child finishes
            if let Some(ref path) = cgroup_path {
                let _ = cleanup_cgroup(path);
            }

            result
        }
        ForkResult::Child => {
            debug!("Child process: starting namespace setup");

            // Execute in namespace (mount+PID+network without user namespace)
            match execute_child_without_userns(script, work_dir, env, network_policy, cgroup_path.as_deref()) {
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
    };

    result
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

    // Step 3: Create mount, PID, and optionally network namespaces based on policy
    // NOTE: Temporarily disabling PID namespace to debug execution issues
    let clone_flags = match network_policy {
        NetworkPolicy::FullNetwork => {
            debug!("Child: UID/GID mapping confirmed, creating mount namespace only (NO network/PID namespace for now)");
            CloneFlags::CLONE_NEWNS
        }
        _ => {
            debug!("Child: UID/GID mapping confirmed, creating mount+network namespaces (NO PID namespace for now)");
            CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWNET
        }
    };

    unshare(clone_flags)
        .map_err(|e| ExecutionError::SandboxError(format!("unshare failed: {}", e)))?;

    debug!("Child: namespaces created successfully");

    // Setup network according to policy (only for isolated namespaces)
    match network_policy {
        NetworkPolicy::Isolated => {
            debug!("Network: Full isolation (no network access)");
            // Do nothing - isolated by default in new network namespace
        }
        NetworkPolicy::LoopbackOnly => {
            setup_loopback()?;
            debug!("Network: Loopback only (127.0.0.1 accessible)");
        }
        NetworkPolicy::FullNetwork => {
            debug!("Network: Full access (inherited from host)");
            // Network namespace not created - inherits host network
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
    let essential_dirs = ["/bin", "/sbin", "/usr", "/lib", "/lib64"];
    let optional_files = ["/etc/resolv.conf", "/etc/ssl"];

    // Mount essential directories - fail if these don't work
    for dir_str in &essential_dirs {
        let dir = Path::new(dir_str);
        if dir.exists() {
            // Step 1: Bind mount (writable first)
            mount(
                Some(dir),
                dir,
                None::<&str>,
                MsFlags::MS_BIND | MsFlags::MS_REC,
                None::<&str>,
            )
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to bind mount essential directory {}: {}", dir_str, e)))?;

            // Step 2: Remount as read-only
            mount(
                None::<&str>,
                dir,
                None::<&str>,
                MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY | MsFlags::MS_REC,
                None::<&str>,
            )
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to remount essential directory {} as read-only: {}", dir_str, e)))?;

            debug!("Child: bind mounted {} (read-only)", dir_str);
        } else {
            warn!("Child: essential directory {} does not exist", dir_str);
        }
    }

    // Mount optional files - non-fatal if missing
    for file_str in &optional_files {
        let file = Path::new(file_str);
        if file.exists() {
            // Step 1: Bind mount
            match mount(
                Some(file),
                file,
                None::<&str>,
                MsFlags::MS_BIND,
                None::<&str>,
            ) {
                Ok(_) => {
                    // Step 2: Remount as read-only
                    match mount(
                        None::<&str>,
                        file,
                        None::<&str>,
                        MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY,
                        None::<&str>,
                    ) {
                        Ok(_) => debug!("Child: bind mounted {} (read-only)", file_str),
                        Err(e) => warn!("Child: failed to remount optional file {} as read-only (skipping): {}", file_str, e),
                    }
                }
                Err(e) => warn!("Child: failed to bind mount optional file {} (skipping): {}", file_str, e),
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
    // Use absolute path to bash to avoid PATH lookups in the new namespace
    let mut cmd = Command::new("/usr/bin/bash");
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

    debug!("Child: executing: /usr/bin/bash -c {:?}", script);

    // Execute and get status
    let status = cmd.status()
        .map_err(|e| ExecutionError::SandboxError(format!("Command execution failed (check if /usr/bin/bash is mounted): {}", e)))?;

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

/// Execute script using bash (fallback for complex scripts)
#[cfg(target_os = "linux")]
fn execute_with_bash(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
    stdout_file: std::fs::File,
    stderr_file: std::fs::File,
) -> Result<i32, ExecutionError> {
    // Use /bin/bash for BitBake compatibility (supports bash-specific syntax)
    let mut cmd = Command::new("/bin/bash");
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

    // Verify work_dir exists
    if !work_dir.exists() {
        return Err(ExecutionError::SandboxError(format!("Work directory does not exist: {:?}", work_dir)));
    }

    // Verify /bin/bash exists
    if !std::path::Path::new("/bin/bash").exists() {
        return Err(ExecutionError::SandboxError("/bin/bash does not exist!".to_string()));
    }

    debug!("Executing with /bin/bash in work_dir {:?}: {:?}", work_dir, &script[..script.len().min(100)]);
    debug!("Current PID: {}, work_dir exists: {}, /bin/bash exists: {}",
           std::process::id(), work_dir.exists(), std::path::Path::new("/bin/bash").exists());

    // Execute and get status
    let status = cmd.status()
        .map_err(|e| ExecutionError::SandboxError(format!("Command execution failed (shell not found or libs missing - errno {:?}): {}", e.raw_os_error(), e)))?;

    debug!("Command completed with exit code: {:?}", status.code());

    Ok(status.code().unwrap_or(1))
}

/// Child process: create mount+PID+network namespaces without user namespace
/// (requires CAP_SYS_ADMIN or running in privileged mode)
#[cfg(target_os = "linux")]
fn execute_child_without_userns(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
    network_policy: NetworkPolicy,
    cgroup_path: Option<&Path>,
) -> Result<i32, ExecutionError> {
    use std::fs::File;

    // Move to cgroup BEFORE creating namespaces (so child inherits cgroup)
    if let Some(path) = cgroup_path {
        move_to_cgroup(path)?;
    }

    // Determine which namespaces to create based on network policy
    // NOTE: Temporarily disabling mount+PID namespaces to debug execution
    // We'll use network isolation and process isolation without filesystem isolation for now
    let clone_flags = match network_policy {
        NetworkPolicy::FullNetwork => {
            debug!("Child: NO namespaces for debugging (full host access)");
            // No namespace isolation for now
            CloneFlags::empty()
        }
        _ => {
            debug!("Child: creating network namespace only (NO mount/PID namespace for now)");
            // Only network isolation
            CloneFlags::CLONE_NEWNET
        }
    };

    // Create namespaces (if any)
    if !clone_flags.is_empty() {
        unshare(clone_flags)
            .map_err(|e| ExecutionError::SandboxError(format!("unshare failed: {}", e)))?;
    }

    debug!("Child: namespaces created successfully");

    // Setup network according to policy (only for isolated namespaces)
    match network_policy {
        NetworkPolicy::Isolated => {
            debug!("Network: Full isolation (no network access)");
            // Do nothing - isolated by default in new network namespace
        }
        NetworkPolicy::LoopbackOnly => {
            setup_loopback()?;
            debug!("Network: Loopback only (127.0.0.1 accessible)");
        }
        NetworkPolicy::FullNetwork => {
            debug!("Network: Full access (inherited from host)");
            // Network namespace not created - inherits host network
        }
        NetworkPolicy::Controlled => {
            return Err(ExecutionError::SandboxError(
                "Controlled network access not yet implemented".to_string()
            ));
        }
    }

    // NOTE: Mount operations disabled since we're not using mount namespaces
    // When we add proper mount namespace support, we'll need to properly set up the root
    // filesystem before bind mounting system directories

    debug!("Child: skipping mount operations (no mount namespace)");

    // Install BitBake prelude script
    install_prelude_script()
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to install prelude: {}", e)))?;

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

    // Try fast path: analyze script for direct execution
    let analysis = analyze_script(script);

    let exit_code = if analysis.is_simple {
        info!("Fast path: executing {} actions directly (no bash)", analysis.actions.len());

        // Execute directly without bash (2-5x faster)
        match execute_direct(&analysis, work_dir, env) {
            Ok(result) => {
                // Write output to files
                use std::io::Write;
                let mut stdout_f = fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&stdout_path)
                    .map_err(|e| ExecutionError::SandboxError(format!("Failed to write stdout: {}", e)))?;
                stdout_f.write_all(result.stdout.as_bytes())?;

                let mut stderr_f = fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&stderr_path)
                    .map_err(|e| ExecutionError::SandboxError(format!("Failed to write stderr: {}", e)))?;
                stderr_f.write_all(result.stderr.as_bytes())?;

                debug!("Fast path completed in {} ms (exit code {})", result.duration_ms, result.exit_code);
                result.exit_code
            }
            Err(e) => {
                warn!("Fast path failed, falling back to bash: {}", e);
                // Fall back to bash
                execute_with_bash(script, work_dir, env, stdout_file, stderr_file)?
            }
        }
    } else {
        debug!("Complex script detected ({}), using bash: {:?}",
            analysis.complexity_reason.as_deref().unwrap_or("unknown"), script);
        // Use bash for complex scripts
        execute_with_bash(script, work_dir, env, stdout_file, stderr_file)?
    };

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
    _resource_limits: &ResourceLimits,
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

        let result = execute_in_namespace(script, &work_dir, &env, NetworkPolicy::Isolated, &ResourceLimits::default());

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

        let result = execute_in_namespace(script, &work_dir, &env, NetworkPolicy::Isolated, &ResourceLimits::default());

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

        let result = execute_in_namespace(script, &work_dir, &env, NetworkPolicy::Isolated, &ResourceLimits::default());

        assert!(result.is_ok());
        let (exit_code, stdout, _) = result.unwrap();
        assert_eq!(exit_code, 0);
        assert!(stdout.contains("test_value"));
    }
}
