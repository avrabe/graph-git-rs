//! Native Linux namespace sandbox implementation
//!
//! Provides process isolation using Linux namespaces directly via the nix crate.
//! No external dependencies like bubblewrap needed.

#[cfg(target_os = "linux")]
use nix::mount::{mount, MsFlags};
#[cfg(target_os = "linux")]
use nix::sched::{unshare, CloneFlags};
#[cfg(target_os = "linux")]
use nix::unistd::{chdir, fork, ForkResult, Pid};
#[cfg(target_os = "linux")]
use nix::sys::wait::{waitpid, WaitStatus};

use std::fs;
use std::path::Path;
use std::process::Command;
use std::os::unix::process::ExitStatusExt;

use super::types::ExecutionError;
use tracing::{debug, info};

/// Execute command in native Linux namespace sandbox
#[cfg(target_os = "linux")]
pub fn execute_in_namespace(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
) -> Result<(i32, String, String), ExecutionError> {
    info!("Executing in native Linux namespace sandbox");

    // Create work directory
    fs::create_dir_all(work_dir)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create work dir: {}", e)))?;

    // Fork for namespace isolation
    match unsafe { fork() }
        .map_err(|e| ExecutionError::SandboxError(format!("Fork failed: {}", e)))?
    {
        ForkResult::Parent { child } => {
            // Parent: Wait for child and collect results
            wait_for_child(child, work_dir)
        }
        ForkResult::Child => {
            // Child: Create namespaces and execute
            match execute_child(script, work_dir, env) {
                Ok(exit_code) => {
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

/// Child process: create namespaces and execute command
#[cfg(target_os = "linux")]
fn execute_child(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
) -> Result<i32, ExecutionError> {
    use std::fs::File;

    // Create new mount and PID namespaces
    unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID)
        .map_err(|e| ExecutionError::SandboxError(format!("unshare failed: {}", e)))?;

    // Make / private to prevent mount propagation
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_PRIVATE | MsFlags::MS_REC,
        None::<&str>,
    ).map_err(|e| ExecutionError::SandboxError(format!("mount --make-private failed: {}", e)))?;

    // Change to work directory
    chdir(work_dir)
        .map_err(|e| ExecutionError::SandboxError(format!("chdir failed: {}", e)))?;

    // Setup stdout/stderr capture
    let stdout_path = work_dir.join("../stdout.log");
    let stderr_path = work_dir.join("../stderr.log");

    let stdout_file = File::create(&stdout_path)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create stdout.log: {}", e)))?;
    let stderr_file = File::create(&stderr_path)
        .map_err(|e| ExecutionError::SandboxError(format!("Failed to create stderr.log: {}", e)))?;

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
    cmd.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    cmd.env("SHELL", "/bin/bash");

    debug!("Executing: bash -c {:?}", script);

    // Execute and get status
    let status = cmd.status()
        .map_err(|e| ExecutionError::SandboxError(format!("Command execution failed: {}", e)))?;

    Ok(status.code().unwrap_or(1))
}

/// Parent process: wait for child and collect output
#[cfg(target_os = "linux")]
fn wait_for_child(
    child: Pid,
    work_dir: &Path,
) -> Result<(i32, String, String), ExecutionError> {
    match waitpid(child, None)
        .map_err(|e| ExecutionError::SandboxError(format!("waitpid failed: {}", e)))?
    {
        WaitStatus::Exited(_pid, code) => {
            // Read captured output
            let stdout_path = work_dir.join("../stdout.log");
            let stderr_path = work_dir.join("../stderr.log");

            let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
            let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();

            debug!("Child exited with code: {}", code);

            Ok((code, stdout, stderr))
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
) -> Result<(i32, String, String), ExecutionError> {
    Err(ExecutionError::SandboxError(
        "Native namespace sandbox only available on Linux".to_string()
    ))
}
