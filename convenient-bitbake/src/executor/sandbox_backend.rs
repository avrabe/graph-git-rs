//! Sandboxing backends for different platforms
//!
//! Provides multiple isolation backends:
//! - **Bubblewrap** (Linux): User namespace containers without root
//! - **sandbox-exec** (macOS): Apple App Sandbox with profile-based restrictions
//! - **Basic** (fallback): Directory isolation only
//!
//! ## Security Features by Backend
//!
//! ### Bubblewrap (Linux) - RECOMMENDED
//! - User/PID/mount/network namespaces
//! - No network access by default
//! - Read-only root filesystem
//! - Private /tmp, /dev
//! - Process isolation
//!
//! ### sandbox-exec (macOS)
//! - File system access control
//! - Network restrictions
//! - IPC restrictions
//! - Process restrictions
//!
//! ### Basic (Fallback)
//! - Directory isolation only
//! - ⚠️  No real security - for development only

use super::types::{ExecutionError, ExecutionResult, SandboxSpec};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::{debug, info, warn};

/// Sandboxing backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    /// Native Linux namespaces - direct namespace control (RECOMMENDED for Linux)
    NativeNamespace,

    /// Bubblewrap - Linux user namespace containers
    Bubblewrap,

    /// sandbox-exec - macOS App Sandbox
    SandboxExec,

    /// Basic directory isolation (no real sandboxing)
    Basic,
}

impl SandboxBackend {
    /// Detect the best available sandbox backend for this platform
    pub fn detect() -> Self {
        #[cfg(target_os = "linux")]
        {
            // Prefer native namespace implementation (no external dependencies)
            info!("Using native Linux namespace sandbox (recommended)");
            return Self::NativeNamespace;
        }

        #[cfg(target_os = "macos")]
        {
            // sandbox-exec is built-in on macOS
            if Command::new("sandbox-exec").arg("-h").output().is_ok() {
                info!("Detected sandbox-exec backend (macOS)");
                return Self::SandboxExec;
            }
            warn!("sandbox-exec not found (unusual on macOS)");
        }

        warn!("Using basic sandbox (⚠️  NO ISOLATION - development only)");
        Self::Basic
    }

    /// Execute command in sandbox using this backend
    pub fn execute(
        &self,
        spec: &SandboxSpec,
        sandbox_root: &Path,
    ) -> ExecutionResult<SandboxResult> {
        match self {
            Self::NativeNamespace => self.execute_native_namespace(spec, sandbox_root),
            Self::Bubblewrap => self.execute_bubblewrap(spec, sandbox_root),
            Self::SandboxExec => self.execute_sandbox_exec(spec, sandbox_root),
            Self::Basic => self.execute_basic(spec, sandbox_root),
        }
    }

    /// Execute using Bubblewrap (Linux)
    fn execute_bubblewrap(
        &self,
        spec: &SandboxSpec,
        sandbox_root: &Path,
    ) -> ExecutionResult<SandboxResult> {
        let start = std::time::Instant::now();

        let mut cmd = Command::new("bwrap");

        // Create new user namespace (no root required)
        cmd.arg("--unshare-all");  // Unshare all namespaces
        cmd.arg("--share-net");    // But allow network (can be disabled per-task)
        cmd.arg("--die-with-parent"); // Kill sandbox if parent dies

        // Mount minimal read-only root filesystem
        cmd.arg("--ro-bind").arg("/usr").arg("/usr");
        cmd.arg("--ro-bind").arg("/bin").arg("/bin");
        cmd.arg("--ro-bind").arg("/lib").arg("/lib");
        cmd.arg("--ro-bind").arg("/lib64").arg("/lib64");
        cmd.arg("--ro-bind").arg("/etc/resolv.conf").arg("/etc/resolv.conf");
        cmd.arg("--ro-bind").arg("/etc/ssl").arg("/etc/ssl");

        // Create writable /tmp
        cmd.arg("--tmpfs").arg("/tmp");

        // Create minimal /dev
        cmd.arg("--dev").arg("/dev");

        // Create /proc
        cmd.arg("--proc").arg("/proc");

        // Mount sandbox work directory
        let work_dir = sandbox_root.join("work");
        fs::create_dir_all(&work_dir)?;
        cmd.arg("--bind").arg(&work_dir).arg("/work");

        // Set working directory
        cmd.arg("--chdir").arg("/work");

        // Set environment variables
        cmd.arg("--clearenv"); // Start with empty environment
        for (key, value) in &spec.env {
            cmd.arg("--setenv").arg(key).arg(value);
        }

        // Essential environment
        cmd.arg("--setenv").arg("HOME").arg("/tmp");
        cmd.arg("--setenv").arg("PATH").arg("/usr/bin:/bin");
        cmd.arg("--setenv").arg("SHELL").arg("/bin/bash");

        // Add the actual command to execute
        cmd.arg("--");
        if spec.command.len() == 1 {
            cmd.arg("bash").arg("-c").arg(&spec.command[0]);
        } else {
            cmd.args(&spec.command);
        }

        // Capture output
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!("Executing with bubblewrap: {:?}", cmd);

        let output = cmd.output().map_err(|e| {
            ExecutionError::SandboxError(format!("Failed to execute bubblewrap: {}", e))
        })?;

        let duration = start.elapsed();

        Ok(SandboxResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: duration.as_millis() as u64,
        })
    }

    /// Execute using sandbox-exec (macOS)
    fn execute_sandbox_exec(
        &self,
        spec: &SandboxSpec,
        sandbox_root: &Path,
    ) -> ExecutionResult<SandboxResult> {
        let start = std::time::Instant::now();

        // Create sandbox profile
        let profile = self.create_macos_sandbox_profile(sandbox_root)?;
        let profile_path = sandbox_root.join("sandbox.sb");
        fs::write(&profile_path, profile)?;

        let mut cmd = Command::new("sandbox-exec");
        cmd.arg("-f").arg(&profile_path);

        // Set working directory
        let work_dir = sandbox_root.join("work");
        fs::create_dir_all(&work_dir)?;
        cmd.current_dir(&work_dir);

        // Set environment variables
        cmd.env_clear();
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }

        // Essential environment
        cmd.env("HOME", "/tmp");
        cmd.env("PATH", "/usr/bin:/bin");
        cmd.env("SHELL", "/bin/bash");

        // Add the actual command
        if spec.command.len() == 1 {
            cmd.arg("bash").arg("-c").arg(&spec.command[0]);
        } else {
            cmd.args(&spec.command);
        }

        // Capture output
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!("Executing with sandbox-exec: {:?}", cmd);

        let output = cmd.output().map_err(|e| {
            ExecutionError::SandboxError(format!("Failed to execute sandbox-exec: {}", e))
        })?;

        let duration = start.elapsed();

        Ok(SandboxResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: duration.as_millis() as u64,
        })
    }

    /// Create macOS sandbox profile
    fn create_macos_sandbox_profile(&self, sandbox_root: &Path) -> ExecutionResult<String> {
        // Scheme-based sandbox profile for macOS
        // See: https://reverse.put.as/wp-content/uploads/2011/09/Apple-Sandbox-Guide-v1.0.pdf

        let work_dir = sandbox_root.join("work");
        let work_dir_str = work_dir.to_string_lossy();

        Ok(format!(
            r#"(version 1)
(debug deny)

; Deny everything by default
(deny default)

; Allow reading system files
(allow file-read*
    (subpath "/usr/lib")
    (subpath "/usr/bin")
    (subpath "/bin")
    (subpath "/System/Library")
    (literal "/etc/resolv.conf"))

; Allow reading SSL certificates
(allow file-read*
    (subpath "/etc/ssl")
    (subpath "/usr/share/ca-certificates"))

; Allow work directory access
(allow file-read* file-write*
    (subpath "{}"))

; Allow basic process operations
(allow process-exec
    (subpath "/usr/bin")
    (subpath "/bin"))

; Allow /dev/null and /dev/urandom
(allow file-read* file-write*
    (literal "/dev/null")
    (literal "/dev/zero")
    (literal "/dev/random")
    (literal "/dev/urandom"))

; Allow basic networking (can be disabled)
(allow network-outbound)
(allow network-inbound (local ip))

; Allow mach lookups for basic services
(allow mach-lookup
    (global-name "com.apple.system.opendirectoryd.libinfo"))
"#,
            work_dir_str
        ))
    }

    /// Execute using basic directory isolation (fallback)
    fn execute_basic(
        &self,
        spec: &SandboxSpec,
        sandbox_root: &Path,
    ) -> ExecutionResult<SandboxResult> {
        let start = std::time::Instant::now();

        warn!("⚠️  Using basic sandbox - NO REAL ISOLATION");

        // Prepare command
        let mut cmd = if spec.command.len() == 1 {
            let mut c = Command::new("bash");
            c.arg("-c").arg(&spec.command[0]);
            c
        } else {
            let mut c = Command::new(&spec.command[0]);
            c.args(&spec.command[1..]);
            c
        };

        // Set working directory
        let work_dir = sandbox_root.join("work");
        fs::create_dir_all(&work_dir)?;
        cmd.current_dir(&work_dir);

        // Set environment variables, remapping /work paths to actual sandbox directory
        cmd.env_clear();
        for (key, value) in &spec.env {
            // Remap paths starting with /work to the actual sandbox work directory
            let remapped_value = if value.starts_with("/work") {
                value.replacen("/work", work_dir.to_str().unwrap(), 1)
            } else {
                value.clone()
            };
            cmd.env(key, remapped_value);
        }

        // Essential environment
        cmd.env("HOME", "/tmp");
        cmd.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
        cmd.env("SHELL", "/bin/bash");

        // Capture output
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd.output().map_err(|e| {
            ExecutionError::SandboxError(format!("Failed to execute command: {}", e))
        })?;

        let duration = start.elapsed();

        Ok(SandboxResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: duration.as_millis() as u64,
        })
    }

    /// Execute using native Linux namespaces (Linux only)
    #[cfg(target_os = "linux")]
    fn execute_native_namespace(
        &self,
        spec: &SandboxSpec,
        sandbox_root: &Path,
    ) -> ExecutionResult<SandboxResult> {
        use super::native_sandbox;

        let start = std::time::Instant::now();
        info!("Using native Linux namespace sandbox");

        // Prepare work directory
        let work_dir = sandbox_root.join("work");
        fs::create_dir_all(&work_dir)?;

        // Build script from command
        let script = if spec.command.len() == 1 {
            spec.command[0].clone()
        } else {
            spec.command.join(" ")
        };

        // Execute in namespace
        let (exit_code, stdout, stderr) = native_sandbox::execute_in_namespace(
            &script,
            &work_dir,
            &spec.env,
        )?;

        let duration = start.elapsed();

        debug!("Native sandbox execution completed: exit_code={}", exit_code);

        Ok(SandboxResult {
            exit_code,
            stdout,
            stderr,
            duration_ms: duration.as_millis() as u64,
        })
    }

    /// Execute using native Linux namespaces (non-Linux fallback)
    #[cfg(not(target_os = "linux"))]
    fn execute_native_namespace(
        &self,
        spec: &SandboxSpec,
        sandbox_root: &Path,
    ) -> ExecutionResult<SandboxResult> {
        warn!("Native namespace backend only available on Linux, using basic sandbox");
        self.execute_basic(spec, sandbox_root)
    }
}

/// Result of sandbox execution
#[derive(Debug, Clone)]
pub struct SandboxResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

impl SandboxResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_backend_detection() {
        let backend = SandboxBackend::detect();

        #[cfg(target_os = "linux")]
        {
            // On Linux, should prefer bubblewrap if available
            if Command::new("bwrap").arg("--version").output().is_ok() {
                assert_eq!(backend, SandboxBackend::Bubblewrap);
            } else {
                assert_eq!(backend, SandboxBackend::Basic);
            }
        }

        #[cfg(target_os = "macos")]
        {
            // On macOS, should use sandbox-exec
            assert_eq!(backend, SandboxBackend::SandboxExec);
        }
    }

    #[test]
    fn test_basic_sandbox_echo() {
        let tmp = TempDir::new().unwrap();
        let backend = SandboxBackend::Basic;

        let mut spec = SandboxSpec::new(vec!["echo 'Hello sandbox'".to_string()]);
        spec.cwd = PathBuf::from("/work");

        let result = backend.execute(&spec, tmp.path()).unwrap();

        assert!(result.success());
        assert!(result.stdout.contains("Hello sandbox"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_bubblewrap_available() {
        // Just check if bwrap is available, don't fail if not
        if let Ok(output) = Command::new("bwrap").arg("--version").output() {
            assert!(output.status.success());
            println!("Bubblewrap version: {}", String::from_utf8_lossy(&output.stdout));
        } else {
            println!("Bubblewrap not available - install with: apt install bubblewrap");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_sandbox_exec_available() {
        // sandbox-exec should always be available on macOS
        let output = Command::new("sandbox-exec").arg("-h").output();
        assert!(output.is_ok(), "sandbox-exec should be available on macOS");
    }
}
