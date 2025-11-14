//! Sandboxed task execution
//!
//! Provides secure execution environments using platform-specific sandboxing:
//! - Linux: Bubblewrap (user namespaces)
//! - macOS: sandbox-exec (App Sandbox)
//! - Fallback: Basic directory isolation

use super::sandbox_backend::{SandboxBackend, SandboxResult};
use super::types::{SandboxSpec, ExecutionError, ExecutionResult};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Manages sandbox creation and execution
pub struct SandboxManager {
    /// Root directory for all sandboxes
    root: PathBuf,

    /// Sandboxing backend to use
    backend: SandboxBackend,
}

impl SandboxManager {
    /// Create a new sandbox manager with auto-detected backend
    pub fn new(root: impl Into<PathBuf>) -> ExecutionResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        let backend = SandboxBackend::detect();

        Ok(Self { root, backend })
    }

    /// Create a new sandbox manager with explicit backend
    pub fn with_backend(root: impl Into<PathBuf>, backend: SandboxBackend) -> ExecutionResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;

        Ok(Self { root, backend })
    }

    /// Create a sandbox from spec
    pub fn create_sandbox(&self, spec: SandboxSpec) -> ExecutionResult<Sandbox> {
        // Generate unique sandbox ID
        let sandbox_id = uuid::Uuid::new_v4().to_string();
        let sandbox_root = self.root.join(&sandbox_id);

        Sandbox::create(sandbox_root, spec, self.backend)
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
    /// Sandboxing backend
    backend: SandboxBackend,
}

impl Sandbox {
    /// Create a new sandbox
    fn create(root: PathBuf, spec: SandboxSpec, backend: SandboxBackend) -> ExecutionResult<Self> {
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
            if std::os::unix::fs::symlink(host_path, &dest).is_err() {
                if host_path.is_file() {
                    fs::copy(host_path, &dest)?;
                } else if host_path.is_dir() {
                    copy_dir_recursive(host_path, &dest)?;
                }
            }
        }

        Ok(Self { root, spec, backend })
    }

    /// Update environment variables
    pub fn update_env(&mut self, env: HashMap<String, String>) {
        self.spec.env = env;
    }

    /// Execute command in sandbox using the configured backend
    pub fn execute(&self) -> ExecutionResult<SandboxResult> {
        self.backend.execute(&self.spec, &self.root)
    }

    /// Collect outputs from sandbox
    pub fn collect_outputs(&self) -> ExecutionResult<HashMap<PathBuf, Vec<u8>>> {
        use tracing::debug;
        let mut outputs = HashMap::new();

        debug!("Collecting outputs from sandbox root: {}", self.root.display());
        debug!("Expected outputs: {:?}", self.spec.outputs);

        for output_path in &self.spec.outputs {
            // Strip leading / and join with sandbox root
            let rel_path = output_path.strip_prefix("/").unwrap_or(output_path);
            let full_path = self.root.join(rel_path);

            debug!("Looking for output: {} at {}", output_path.display(), full_path.display());

            if full_path.is_file() {
                let content = fs::read(&full_path)?;
                debug!("✓ Found file: {} ({} bytes)", full_path.display(), content.len());
                outputs.insert(output_path.clone(), content);
            } else if full_path.is_dir() {
                debug!("✓ Found directory: {}", full_path.display());
                // Collect all files in directory
                collect_dir_outputs(&full_path, output_path, &mut outputs)?;
            } else {
                // List what actually exists
                if let Some(parent) = full_path.parent() {
                    if parent.exists() {
                        debug!("✗ Output not found. Parent directory contents:");
                        if let Ok(entries) = fs::read_dir(parent) {
                            for entry in entries.flatten() {
                                debug!("  - {}", entry.path().display());
                            }
                        }
                    } else {
                        debug!("✗ Parent directory doesn't exist: {}", parent.display());
                    }
                }
                return Err(ExecutionError::MissingOutput(output_path.clone()));
            }
        }

        debug!("Successfully collected {} outputs", outputs.len());
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
