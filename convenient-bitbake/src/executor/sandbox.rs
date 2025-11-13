//! Sandboxed task execution
//!
//! For the initial POC, we implement a simple directory-based sandbox.
//! Future versions will add Linux namespace isolation.

use super::types::{SandboxSpec, ExecutionError, ExecutionResult};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Manages sandbox creation and execution
pub struct SandboxManager {
    /// Root directory for all sandboxes
    root: PathBuf,
}

impl SandboxManager {
    /// Create a new sandbox manager
    pub fn new(root: impl Into<PathBuf>) -> ExecutionResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;

        Ok(Self { root })
    }

    /// Create a sandbox from spec
    pub fn create_sandbox(&self, spec: SandboxSpec) -> ExecutionResult<Sandbox> {
        // Generate unique sandbox ID
        let sandbox_id = uuid::Uuid::new_v4().to_string();
        let sandbox_root = self.root.join(&sandbox_id);

        Sandbox::create(sandbox_root, spec)
    }

    /// Clean up old sandboxes
    pub fn cleanup(&self) -> ExecutionResult<usize> {
        let mut count = 0;
        if self.root.exists() {
            for entry in fs::read_dir(&self.root)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    fs::remove_dir_all(entry.path())?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}

/// A sandboxed execution environment
pub struct Sandbox {
    /// Root directory of this sandbox
    root: PathBuf,
    /// Sandbox specification
    spec: SandboxSpec,
}

impl Sandbox {
    /// Create a new sandbox
    fn create(root: PathBuf, spec: SandboxSpec) -> ExecutionResult<Self> {
        fs::create_dir_all(&root)?;

        // Create writable directories
        for dir in &spec.rw_dirs {
            let full_path = root.join(dir.strip_prefix("/").unwrap_or(dir));
            fs::create_dir_all(&full_path)?;
        }

        // Mount (symlink/copy) read-only inputs
        for (host_path, sandbox_path) in &spec.ro_inputs {
            let dest = root.join(sandbox_path.strip_prefix("/").unwrap_or(sandbox_path));
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }

            // Try symlink first (fast), fall back to copy
            if let Err(_) = std::os::unix::fs::symlink(host_path, &dest) {
                if host_path.is_file() {
                    fs::copy(host_path, &dest)?;
                } else if host_path.is_dir() {
                    copy_dir_recursive(host_path, &dest)?;
                }
            }
        }

        Ok(Self { root, spec })
    }

    /// Execute command in sandbox
    pub fn execute(&self) -> ExecutionResult<SandboxResult> {
        let start = Instant::now();

        // Prepare command
        let mut cmd = if self.spec.command.len() == 1 {
            // Single command, use shell
            let mut c = Command::new("bash");
            c.arg("-c").arg(&self.spec.command[0]);
            c
        } else {
            // Multiple args, execute directly
            let mut c = Command::new(&self.spec.command[0]);
            c.args(&self.spec.command[1..]);
            c
        };

        // Set working directory
        let cwd = self.root.join(self.spec.cwd.strip_prefix("/").unwrap_or(&self.spec.cwd));
        fs::create_dir_all(&cwd)?;
        cmd.current_dir(&cwd);

        // Set environment variables
        cmd.env_clear();  // Start with clean environment
        for (key, value) in &self.spec.env {
            cmd.env(key, value);
        }

        // Essential environment
        cmd.env("HOME", "/tmp");
        cmd.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
        cmd.env("SHELL", "/bin/bash");

        // Capture output
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Execute
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

    /// Collect outputs from sandbox
    pub fn collect_outputs(&self) -> ExecutionResult<HashMap<PathBuf, Vec<u8>>> {
        let mut outputs = HashMap::new();

        for output_path in &self.spec.outputs {
            let full_path = self.root.join(output_path.strip_prefix("/").unwrap_or(output_path));

            if full_path.is_file() {
                let content = fs::read(&full_path)?;
                outputs.insert(output_path.clone(), content);
            } else if full_path.is_dir() {
                // Collect all files in directory
                collect_dir_outputs(&full_path, output_path, &mut outputs)?;
            } else if !full_path.exists() {
                return Err(ExecutionError::MissingOutput(output_path.clone()));
            }
        }

        Ok(outputs)
    }

    /// Get the root directory of this sandbox
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Clean up sandbox
    pub fn cleanup(self) -> ExecutionResult<()> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)?;
        }
        Ok(())
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

/// Recursively copy directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Collect all files from a directory into outputs map
fn collect_dir_outputs(
    dir: &Path,
    base_path: &Path,
    outputs: &mut HashMap<PathBuf, Vec<u8>>,
) -> ExecutionResult<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel_path = base_path.join(entry.file_name());

        if path.is_file() {
            let content = fs::read(&path)?;
            outputs.insert(rel_path, content);
        } else if path.is_dir() {
            collect_dir_outputs(&path, &rel_path, outputs)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sandbox_echo() {
        let tmp = TempDir::new().unwrap();
        let manager = SandboxManager::new(tmp.path()).unwrap();

        let mut spec = SandboxSpec::new(vec!["echo 'Hello from sandbox'".to_string()]);
        spec.cwd = PathBuf::from("/work");
        spec.rw_dirs.push(PathBuf::from("/work"));

        let sandbox = manager.create_sandbox(spec).unwrap();
        let result = sandbox.execute().unwrap();

        assert!(result.success());
        assert!(result.stdout.contains("Hello from sandbox"));

        sandbox.cleanup().unwrap();
    }

    #[test]
    fn test_sandbox_write_file() {
        let tmp = TempDir::new().unwrap();
        let manager = SandboxManager::new(tmp.path()).unwrap();

        let mut spec = SandboxSpec::new(vec![
            "echo 'test content' > /work/output.txt".to_string()
        ]);
        spec.cwd = PathBuf::from("/work");
        spec.rw_dirs.push(PathBuf::from("/work"));
        spec.outputs.push(PathBuf::from("/work/output.txt"));

        let sandbox = manager.create_sandbox(spec).unwrap();
        let result = sandbox.execute().unwrap();
        assert!(result.success());

        let outputs = sandbox.collect_outputs().unwrap();
        assert!(outputs.contains_key(&PathBuf::from("/work/output.txt")));

        let content = String::from_utf8_lossy(&outputs[&PathBuf::from("/work/output.txt")]);
        assert!(content.contains("test content"));

        sandbox.cleanup().unwrap();
    }

    #[test]
    fn test_sandbox_environment() {
        let tmp = TempDir::new().unwrap();
        let manager = SandboxManager::new(tmp.path()).unwrap();

        let mut spec = SandboxSpec::new(vec!["echo $TEST_VAR".to_string()]);
        spec.env.insert("TEST_VAR".to_string(), "test-value".to_string());
        spec.cwd = PathBuf::from("/work");
        spec.rw_dirs.push(PathBuf::from("/work"));

        let sandbox = manager.create_sandbox(spec).unwrap();
        let result = sandbox.execute().unwrap();

        assert!(result.success());
        assert!(result.stdout.contains("test-value"));

        sandbox.cleanup().unwrap();
    }
}
