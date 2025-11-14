//! Core types for task execution

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use sha2::{Sha256, Digest};

/// Network isolation policy for sandbox
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkPolicy {
    /// No network access (default, most hermetic)
    Isolated,

    /// Loopback only (127.0.0.1 accessible)
    LoopbackOnly,

    /// Controlled external access with allow-list (not yet implemented)
    Controlled,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        NetworkPolicy::Isolated  // Safe by default
    }
}

/// Content hash (SHA-256)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash(String);

impl ContentHash {
    /// Create from raw bytes
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Self(format!("{:x}", hasher.finalize()))
    }

    /// Create from file
    pub fn from_file(path: &std::path::Path) -> std::io::Result<Self> {
        let content = std::fs::read(path)?;
        Ok(Self::from_bytes(&content))
    }

    /// Get hex string
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        self.0.clone()
    }

    /// Parse from hex string
    pub fn from_hex(hex: impl Into<String>) -> Self {
        Self(hex.into())
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0[..8])  // Show first 8 chars
    }
}

/// Task signature - uniquely identifies task inputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSignature {
    /// Recipe name
    pub recipe: String,

    /// Task name (e.g., "do_compile")
    pub task: String,

    /// Input file hashes (path → hash)
    pub input_files: HashMap<PathBuf, ContentHash>,

    /// Dependency task signatures
    pub dep_signatures: Vec<ContentHash>,

    /// Environment variables that affect build
    pub env_vars: HashMap<String, String>,

    /// Task implementation hash (bash script content)
    pub task_code_hash: ContentHash,

    /// Combined signature
    #[serde(skip)]
    pub signature: Option<ContentHash>,
}

impl TaskSignature {
    /// Compute the final signature from all inputs
    pub fn compute(&mut self) -> ContentHash {
        let mut parts = Vec::new();

        // Recipe and task name
        parts.push(self.recipe.as_bytes().to_vec());
        parts.push(self.task.as_bytes().to_vec());

        // Input files (sorted for stability)
        let mut input_paths: Vec<_> = self.input_files.keys().collect();
        input_paths.sort();
        for path in input_paths {
            parts.push(path.to_string_lossy().as_bytes().to_vec());
            parts.push(self.input_files[path].as_str().as_bytes().to_vec());
        }

        // Dependencies (sorted)
        let mut deps = self.dep_signatures.clone();
        deps.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for dep in deps {
            parts.push(dep.as_str().as_bytes().to_vec());
        }

        // Environment variables (sorted by key)
        let mut env_keys: Vec<_> = self.env_vars.keys().collect();
        env_keys.sort();
        for key in env_keys {
            parts.push(key.as_bytes().to_vec());
            parts.push(self.env_vars[key].as_bytes().to_vec());
        }

        // Task code
        parts.push(self.task_code_hash.as_str().as_bytes().to_vec());

        // Concatenate and hash
        let combined: Vec<u8> = parts.concat();
        let sig = ContentHash::from_bytes(&combined);
        self.signature = Some(sig.clone());
        sig
    }

    /// Get or compute signature
    pub fn get_signature(&mut self) -> &ContentHash {
        if self.signature.is_none() {
            self.compute();
        }
        self.signature.as_ref().unwrap()
    }
}

/// Specification for task execution
#[derive(Debug, Clone)]
pub struct TaskSpec {
    /// Task name
    pub name: String,

    /// Recipe name
    pub recipe: String,

    /// Shell script to execute
    pub script: String,

    /// Working directory
    pub workdir: PathBuf,

    /// Environment variables
    pub env: HashMap<String, String>,

    /// Declared outputs (relative to workdir)
    pub outputs: Vec<PathBuf>,

    /// Timeout
    pub timeout: Option<Duration>,

    /// Network policy for this task
    pub network_policy: NetworkPolicy,
}

/// Sandbox specification
#[derive(Debug, Clone)]
pub struct SandboxSpec {
    /// Read-only input mounts (host → sandbox)
    pub ro_inputs: Vec<(PathBuf, PathBuf)>,

    /// Writable directories (created in sandbox)
    pub rw_dirs: Vec<PathBuf>,

    /// Output paths to collect
    pub outputs: Vec<PathBuf>,

    /// Environment variables
    pub env: HashMap<String, String>,

    /// Command to execute
    pub command: Vec<String>,

    /// Working directory (inside sandbox)
    pub cwd: PathBuf,

    /// Network policy
    pub network_policy: NetworkPolicy,

    /// Temp directory size limit (MB)
    pub tmp_size_mb: Option<usize>,
}

impl SandboxSpec {
    pub fn new(command: Vec<String>) -> Self {
        Self {
            ro_inputs: Vec::new(),
            rw_dirs: Vec::new(),
            outputs: Vec::new(),
            env: HashMap::new(),
            command,
            cwd: PathBuf::from("/work"),
            network_policy: NetworkPolicy::default(), // Isolated by default
            tmp_size_mb: Some(1024), // 1GB temp
        }
    }
}

/// Task execution output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    /// Task signature that produced this output
    pub signature: ContentHash,

    /// Output file hashes (path → hash)
    pub output_files: HashMap<PathBuf, ContentHash>,

    /// Standard output
    pub stdout: String,

    /// Standard error
    pub stderr: String,

    /// Exit code
    pub exit_code: i32,

    /// Execution time (ms)
    pub duration_ms: u64,
}

impl TaskOutput {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Result of task execution
pub type ExecutionResult<T> = Result<T, ExecutionError>;

/// Errors during execution
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Task failed with exit code {0}")]
    TaskFailed(i32),

    #[error("Task timed out after {0}s")]
    Timeout(u64),

    #[error("Sandbox error: {0}")]
    SandboxError(String),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Missing output: {0}")]
    MissingOutput(PathBuf),

    #[error("Signature mismatch: expected {expected}, got {actual}")]
    SignatureMismatch {
        expected: ContentHash,
        actual: ContentHash,
    },
}
