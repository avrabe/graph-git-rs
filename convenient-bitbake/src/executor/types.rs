//! Core types for task execution

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use sha2::{Sha256, Digest};

/// Execution mode for task - determines sandboxing requirements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum ExecutionMode {
    /// Direct Rust execution - no sandbox, no shell, no host contamination
    /// Only for simple operations: file ops, env vars, logging
    /// Provides maximum performance and hermetic execution
    DirectRust,

    /// Shell script execution - requires full sandboxing
    /// Used when script contains complex bash operations
    #[default]
    Shell,

    /// Python script execution - requires full sandboxing
    /// Used for Python tasks with RustPython VM
    Python,

    /// Rust-based shell execution - in-process bash interpreter
    /// Uses brush-shell for bash compatibility without subprocess overhead
    /// Provides variable tracking and custom built-ins like RustPython
    RustShell,
}


impl ExecutionMode {
    /// Whether this mode requires sandboxing
    pub fn requires_sandbox(&self) -> bool {
        match self {
            ExecutionMode::DirectRust | ExecutionMode::RustShell => false,
            ExecutionMode::Shell | ExecutionMode::Python => true,
        }
    }

    /// Whether this mode can contaminate the host
    pub fn can_contaminate_host(&self) -> bool {
        self.requires_sandbox()
    }
}

/// Network isolation policy for sandbox
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum NetworkPolicy {
    /// No network access (default, most hermetic)
    /// Creates new network namespace with no interfaces
    #[default]
    Isolated,

    /// Loopback only (127.0.0.1 accessible)
    /// Creates new network namespace with loopback interface
    LoopbackOnly,

    /// Full network access (inherits host network)
    /// Does NOT create network namespace - required for real fetching
    /// Use for do_fetch tasks that need to download sources
    FullNetwork,

    /// Controlled external access with allow-list (not yet implemented)
    Controlled,
}


/// Resource limits for cgroup v2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// CPU quota in microseconds per 100ms period
    /// Example: 50000 = 50% of one CPU core
    pub cpu_quota_us: Option<u64>,

    /// Memory limit in bytes
    /// Example: 1073741824 = 1 GB
    pub memory_bytes: Option<u64>,

    /// Maximum number of PIDs (processes/threads)
    /// Prevents fork bombs
    pub pids_max: Option<u64>,

    /// I/O weight (10-1000, default 100)
    /// Higher = more I/O bandwidth
    pub io_weight: Option<u16>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            // Conservative defaults for build tasks
            cpu_quota_us: None,         // Unlimited CPU
            memory_bytes: Some(4 * 1024 * 1024 * 1024), // 4 GB default
            pids_max: Some(1024),       // Prevent fork bombs
            io_weight: Some(100),       // Default I/O priority
        }
    }
}

impl ResourceLimits {
    /// No limits (for trusted tasks)
    pub fn unlimited() -> Self {
        Self {
            cpu_quota_us: None,
            memory_bytes: None,
            pids_max: None,
            io_weight: None,
        }
    }

    /// Strict limits (for untrusted tasks)
    pub fn strict() -> Self {
        Self {
            cpu_quota_us: Some(100_000),  // 1 CPU core max
            memory_bytes: Some(2 * 1024 * 1024 * 1024), // 2 GB
            pids_max: Some(512),           // 512 processes max
            io_weight: Some(50),           // Lower I/O priority
        }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Timeout (in seconds, for serialization compatibility)
    #[serde(
        serialize_with = "serialize_duration",
        deserialize_with = "deserialize_duration"
    )]
    pub timeout: Option<Duration>,

    /// Execution mode (determines if sandboxing is needed)
    #[serde(default)]
    pub execution_mode: ExecutionMode,

    /// Network policy for this task (only used if execution_mode requires sandbox)
    pub network_policy: NetworkPolicy,

    /// Resource limits for this task (only used if execution_mode requires sandbox)
    pub resource_limits: ResourceLimits,
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

    /// Resource limits (cgroup v2)
    pub resource_limits: ResourceLimits,
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
            resource_limits: ResourceLimits::default(), // Conservative defaults
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

// Helper functions for Duration serialization
fn serialize_duration<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match duration {
        Some(d) => serializer.serialize_some(&d.as_secs()),
        None => serializer.serialize_none(),
    }
}

fn deserialize_duration<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let secs: Option<u64> = Option::deserialize(deserializer)?;
    Ok(secs.map(Duration::from_secs))
}
