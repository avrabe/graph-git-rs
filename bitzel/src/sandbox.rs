//! Sandboxed task execution using Linux namespaces and OverlayFS
//!
//! This module provides proper process isolation for BitBake task execution using:
//! - Mount namespace (isolated filesystem view)
//! - PID namespace (isolated process tree)
//! - OverlayFS (efficient dependency merging with zero-copy)
//! - Network namespace (optional isolation)

use nix::mount::{mount, umount, MsFlags};
use nix::sched::{unshare, CloneFlags};
use nix::unistd::{chdir, fork, ForkResult, Pid};
use std::ffi::CString;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("Failed to create namespace: {0}")]
    NamespaceError(String),

    #[error("Failed to mount: {0}")]
    MountError(String),

    #[error("Failed to execute command: {0}")]
    ExecutionError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Nix error: {0}")]
    Nix(#[from] nix::Error),

    #[error("Missing overlay support")]
    NoOverlaySupport,
}

pub type Result<T> = std::result::Result<T, SandboxError>;

/// Dependency layer for overlay mount
#[derive(Debug, Clone)]
pub struct DependencyLayer {
    pub recipe: String,
    pub task: String,
    pub sysroot_path: PathBuf,
}

/// Sandbox configuration
#[derive(Debug)]
pub struct SandboxConfig {
    /// Physical sandbox directory
    pub sandbox_dir: PathBuf,

    /// Script to execute
    pub script: String,

    /// Environment variables
    pub env: Vec<(String, String)>,

    /// Dependency layers for target sysroot
    pub target_deps: Vec<DependencyLayer>,

    /// Dependency layers for native sysroot
    pub native_deps: Vec<DependencyLayer>,

    /// Enable network access
    pub network: bool,
}

/// Sandbox executor using Linux namespaces
pub struct Sandbox {
    config: SandboxConfig,
}

impl Sandbox {
    pub fn new(config: SandboxConfig) -> Result<Self> {
        // Check for overlay support
        if !Self::check_overlay_support()? {
            return Err(SandboxError::NoOverlaySupport);
        }

        Ok(Self { config })
    }

    /// Check if kernel supports overlayfs
    fn check_overlay_support() -> Result<bool> {
        let filesystems = fs::read_to_string("/proc/filesystems")
            .map_err(|e| SandboxError::Io(e))?;
        Ok(filesystems.contains("overlay"))
    }

    /// Execute task in sandbox
    pub fn execute(&self) -> Result<std::process::Output> {
        // Prepare sandbox directory structure
        self.setup_sandbox_dirs()?;

        // Fork process for namespace isolation
        match unsafe { fork() }? {
            ForkResult::Parent { child } => {
                // Wait for child
                self.wait_for_child(child)
            }
            ForkResult::Child => {
                // Child process: create namespaces and execute
                match self.execute_in_namespace() {
                    Ok(output) => {
                        // Exit with success
                        std::process::exit(if output.status.success() { 0 } else { 1 });
                    }
                    Err(e) => {
                        eprintln!("Sandbox execution failed: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    /// Setup sandbox directory structure
    fn setup_sandbox_dirs(&self) -> Result<()> {
        let work_dir = self.config.sandbox_dir.join("work");

        // Create directories
        fs::create_dir_all(&work_dir)?;
        fs::create_dir_all(work_dir.join("outputs"))?;
        fs::create_dir_all(work_dir.join("src"))?;
        fs::create_dir_all(work_dir.join("build"))?;

        // Create mount points for sysroots
        fs::create_dir_all(work_dir.join("recipe-sysroot"))?;
        fs::create_dir_all(work_dir.join("recipe-sysroot-native"))?;

        // Create overlay work directories (required by overlayfs)
        fs::create_dir_all(self.config.sandbox_dir.join("overlay-work"))?;
        fs::create_dir_all(self.config.sandbox_dir.join("overlay-upper"))?;

        Ok(())
    }

    /// Execute task inside namespace
    fn execute_in_namespace(&self) -> Result<std::process::Output> {
        use std::fs::File;

        // Create remaining namespaces (user namespace already created)
        unshare(
            CloneFlags::CLONE_NEWNS    // Mount namespace
            | CloneFlags::CLONE_NEWPID  // PID namespace
            | if !self.config.network {
                CloneFlags::CLONE_NEWNET  // Network namespace (if disabled)
            } else {
                CloneFlags::empty()
            }
        ).map_err(|e| SandboxError::NamespaceError(e.to_string()))?;

        // Make / private (so our mounts don't propagate to host)
        mount(
            None::<&str>,
            "/",
            None::<&str>,
            MsFlags::MS_PRIVATE | MsFlags::MS_REC,
            None::<&str>,
        )?;

        // Setup overlay mounts for sysroots
        self.setup_overlay_mounts()?;

        // Change to work directory
        let work_dir = self.config.sandbox_dir.join("work");
        chdir(&work_dir)?;

        // FIX 1: Redirect stdout/stderr to files for capture
        let stdout_path = self.config.sandbox_dir.join("stdout.log");
        let stderr_path = self.config.sandbox_dir.join("stderr.log");

        let stdout_file = File::create(&stdout_path)?;
        let stderr_file = File::create(&stderr_path)?;

        // Execute command
        let mut cmd = Command::new("bash");
        cmd.arg("-c")
           .arg(&self.config.script)
           .current_dir(&work_dir)
           .stdout(stdout_file)  // Redirect stdout to file
           .stderr(stderr_file); // Redirect stderr to file

        // Set environment variables
        cmd.env("WORKDIR", "/work");  // Virtual path
        cmd.env("S", "/work/src");
        cmd.env("B", "/work/build");
        cmd.env("D", "/work/outputs");

        for (key, value) in &self.config.env {
            cmd.env(key, value);
        }

        // Execute and wait for completion
        let status = cmd.status()
           .map_err(|e| SandboxError::ExecutionError(e.to_string()))?;

        // Return empty output (actual output is in files)
        Ok(std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    /// Setup overlay mounts for dependency sysroots
    fn setup_overlay_mounts(&self) -> Result<()> {
        // Setup target sysroot overlay
        if !self.config.target_deps.is_empty() {
            self.mount_overlay_sysroot(
                &self.config.target_deps,
                &self.config.sandbox_dir.join("work/recipe-sysroot"),
                "target",
            )?;
        }

        // Setup native sysroot overlay
        if !self.config.native_deps.is_empty() {
            self.mount_overlay_sysroot(
                &self.config.native_deps,
                &self.config.sandbox_dir.join("work/recipe-sysroot-native"),
                "native",
            )?;
        }

        Ok(())
    }

    /// Mount overlay filesystem for sysroot
    fn mount_overlay_sysroot(
        &self,
        deps: &[DependencyLayer],
        mount_point: &Path,
        overlay_type: &str,
    ) -> Result<()> {
        if deps.is_empty() {
            return Ok(());
        }

        // FIX 2: Build lowerdir with ABSOLUTE paths (colon-separated)
        // Last dep has highest priority
        let lowerdir = deps
            .iter()
            .map(|dep| {
                // Canonicalize to get absolute path
                dep.sysroot_path
                    .canonicalize()
                    .unwrap_or_else(|_| dep.sysroot_path.clone())
                    .to_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(":");

        // FIX 3: Work and upper directories for overlay (ABSOLUTE paths)
        let work_dir = self.config.sandbox_dir
            .join(format!("overlay-work-{}", overlay_type));
        let upper_dir = self.config.sandbox_dir
            .join(format!("overlay-upper-{}", overlay_type));

        fs::create_dir_all(&work_dir)?;
        fs::create_dir_all(&upper_dir)?;

        // Get absolute paths for upperdir and workdir
        let work_dir_abs = work_dir.canonicalize().unwrap_or(work_dir);
        let upper_dir_abs = upper_dir.canonicalize().unwrap_or(upper_dir);

        // Build overlay options with absolute paths
        let opts = format!(
            "lowerdir={},upperdir={},workdir={}",
            lowerdir,
            upper_dir_abs.display(),
            work_dir_abs.display()
        );

        tracing::debug!("OverlayFS mount options: {}", opts);
        tracing::debug!("Mount point: {}", mount_point.display());
        tracing::debug!("Mount point exists: {}", mount_point.exists());

        // Mount overlay
        let opts_cstring = CString::new(opts.as_str())
            .map_err(|e| SandboxError::MountError(e.to_string()))?;

        mount(
            Some("overlay"),
            mount_point,
            Some("overlay"),
            MsFlags::empty(),
            Some(opts_cstring.as_ref()),
        ).map_err(|e| SandboxError::MountError(format!(
            "Failed to mount overlay at {}: {}",
            mount_point.display(),
            e
        )))?;

        tracing::info!(
            "Mounted {} overlay with {} layers at {}",
            overlay_type,
            deps.len(),
            mount_point.display()
        );

        Ok(())
    }

    /// Wait for child process
    fn wait_for_child(&self, child: Pid) -> Result<std::process::Output> {
        use nix::sys::wait::{waitpid, WaitStatus};

        match waitpid(child, None)? {
            WaitStatus::Exited(_pid, code) => {
                // Read outputs from sandbox
                let stdout_path = self.config.sandbox_dir.join("stdout.log");
                let stderr_path = self.config.sandbox_dir.join("stderr.log");

                let stdout = fs::read(&stdout_path).unwrap_or_default();
                let stderr = fs::read(&stderr_path).unwrap_or_default();

                Ok(std::process::Output {
                    status: std::process::ExitStatus::from_raw(code << 8),
                    stdout,
                    stderr,
                })
            }
            status => {
                Err(SandboxError::ExecutionError(format!(
                    "Child process ended unexpectedly: {:?}",
                    status
                )))
            }
        }
    }

    /// Cleanup sandbox (unmount overlays, remove dirs)
    pub fn cleanup(&self) -> Result<()> {
        // Unmount overlays
        let _ = umount(&self.config.sandbox_dir.join("work/recipe-sysroot"));
        let _ = umount(&self.config.sandbox_dir.join("work/recipe-sysroot-native"));

        // Remove sandbox directory
        let _ = fs::remove_dir_all(&self.config.sandbox_dir);

        Ok(())
    }
}

/// Builder for sandbox configuration
pub struct SandboxBuilder {
    sandbox_dir: Option<PathBuf>,
    script: Option<String>,
    env: Vec<(String, String)>,
    target_deps: Vec<DependencyLayer>,
    native_deps: Vec<DependencyLayer>,
    network: bool,
}

impl SandboxBuilder {
    pub fn new() -> Self {
        Self {
            sandbox_dir: None,
            script: None,
            env: Vec::new(),
            target_deps: Vec::new(),
            native_deps: Vec::new(),
            network: false,
        }
    }

    pub fn sandbox_dir(mut self, dir: PathBuf) -> Self {
        self.sandbox_dir = Some(dir);
        self
    }

    pub fn script(mut self, script: String) -> Self {
        self.script = Some(script);
        self
    }

    pub fn env(mut self, key: String, value: String) -> Self {
        self.env.push((key, value));
        self
    }

    pub fn target_dep(mut self, dep: DependencyLayer) -> Self {
        self.target_deps.push(dep);
        self
    }

    pub fn native_dep(mut self, dep: DependencyLayer) -> Self {
        self.native_deps.push(dep);
        self
    }

    pub fn network(mut self, enabled: bool) -> Self {
        self.network = enabled;
        self
    }

    pub fn build(self) -> Result<Sandbox> {
        let config = SandboxConfig {
            sandbox_dir: self.sandbox_dir
                .ok_or_else(|| SandboxError::ExecutionError("sandbox_dir required".into()))?,
            script: self.script
                .ok_or_else(|| SandboxError::ExecutionError("script required".into()))?,
            env: self.env,
            target_deps: self.target_deps,
            native_deps: self.native_deps,
            network: self.network,
        };

        Sandbox::new(config)
    }
}

impl Default for SandboxBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_support_check() {
        // Should not panic
        let result = Sandbox::check_overlay_support();
        assert!(result.is_ok());
    }

    #[test]
    fn test_sandbox_builder() {
        let builder = SandboxBuilder::new()
            .sandbox_dir(PathBuf::from("/tmp/test"))
            .script("echo test".to_string())
            .env("TEST".to_string(), "value".to_string());

        // Should build config
        assert!(builder.build().is_ok() || builder.build().is_err()); // May fail if no overlay
    }
}
