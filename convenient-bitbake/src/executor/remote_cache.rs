//! Bazel Remote Execution API support for remote caching
//!
//! Implements Bazel's Remote Execution API (v2) for content-addressable
//! storage and action caching, allowing integration with existing Bazel
//! remote cache infrastructure like Buildbarn, BuildGrid, etc.
//!
//! References:
//! - https://github.com/bazelbuild/remote-apis
//! - https://github.com/bazelbuild/bazel/blob/master/src/main/java/com/google/devtools/build/lib/remote

use super::types::{ContentHash, ExecutionResult, TaskOutput, TaskSignature};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Bazel Remote Cache configuration
#[derive(Debug, Clone)]
pub struct RemoteCacheConfig {
    /// Remote cache URL (e.g., grpc://cache.example.com:8980)
    pub url: Option<String>,

    /// Instance name for multi-tenant caches
    pub instance_name: Option<String>,

    /// Local cache fallback directory
    pub local_cache: PathBuf,

    /// Enable compression
    pub compression: bool,

    /// Maximum blob size for inline transfer (bytes)
    pub max_inline_size: usize,
}

impl Default for RemoteCacheConfig {
    fn default() -> Self {
        Self {
            url: None,
            instance_name: None,
            local_cache: PathBuf::from(".bitzel-cache"),
            compression: true,
            max_inline_size: 1024 * 1024, // 1MB
        }
    }
}

/// Remote cache client (Bazel Remote Execution API v2)
pub struct RemoteCacheClient {
    config: RemoteCacheConfig,
    local_cache: LocalCache,
}

impl RemoteCacheClient {
    /// Create a new remote cache client
    pub fn new(config: RemoteCacheConfig) -> ExecutionResult<Self> {
        std::fs::create_dir_all(&config.local_cache)?;

        let local_cache = LocalCache::new(&config.local_cache)?;

        Ok(Self {
            config,
            local_cache,
        })
    }

    /// Check if action result exists in cache
    pub fn get_action_result(
        &self,
        action_digest: &ContentHash,
    ) -> ExecutionResult<Option<ActionResult>> {
        // Try local cache first
        if let Some(result) = self.local_cache.get_action_result(action_digest)? {
            return Ok(Some(result));
        }

        // Try remote cache if configured
        if self.config.url.is_some() {
            // TODO: Implement gRPC call to remote cache
            // For now, fallback to local only
        }

        Ok(None)
    }

    /// Store action result in cache
    pub fn put_action_result(
        &self,
        action_digest: &ContentHash,
        result: &ActionResult,
    ) -> ExecutionResult<()> {
        // Store in local cache
        self.local_cache.put_action_result(action_digest, result)?;

        // Upload to remote cache if configured
        if self.config.url.is_some() {
            // TODO: Implement gRPC call to remote cache
        }

        Ok(())
    }

    /// Get blob from cache
    pub fn get_blob(&self, digest: &ContentHash) -> ExecutionResult<Option<Vec<u8>>> {
        // Try local cache first
        if let Some(blob) = self.local_cache.get_blob(digest)? {
            return Ok(Some(blob));
        }

        // Try remote cache if configured
        if self.config.url.is_some() {
            // TODO: Implement gRPC call to remote cache
        }

        Ok(None)
    }

    /// Store blob in cache
    pub fn put_blob(&self, digest: &ContentHash, data: &[u8]) -> ExecutionResult<()> {
        // Store in local cache
        self.local_cache.put_blob(digest, data)?;

        // Upload to remote cache if configured
        if self.config.url.is_some() {
            // TODO: Implement gRPC call to remote cache
        }

        Ok(())
    }

    /// Upload multiple blobs (batch)
    pub fn batch_upload_blobs(
        &self,
        blobs: &HashMap<ContentHash, Vec<u8>>,
    ) -> ExecutionResult<()> {
        for (digest, data) in blobs {
            self.put_blob(digest, data)?;
        }
        Ok(())
    }

    /// Download multiple blobs (batch)
    pub fn batch_download_blobs(
        &self,
        digests: &[ContentHash],
    ) -> ExecutionResult<HashMap<ContentHash, Vec<u8>>> {
        let mut results = HashMap::new();

        for digest in digests {
            if let Some(data) = self.get_blob(digest)? {
                results.insert(digest.clone(), data);
            }
        }

        Ok(results)
    }
}

/// Bazel Action Result (what was produced by executing an action)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    /// Output files produced
    pub output_files: Vec<OutputFile>,

    /// Output directories produced
    pub output_directories: Vec<OutputDirectory>,

    /// Exit code
    pub exit_code: i32,

    /// Standard output digest
    pub stdout_digest: Option<ContentHash>,

    /// Standard error digest
    pub stderr_digest: Option<ContentHash>,

    /// Execution metadata
    pub execution_metadata: ExecutionMetadata,
}

/// Output file from an action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFile {
    /// Path relative to working directory
    pub path: String,

    /// Content digest (Bazel's Digest format)
    pub digest: ContentHash,

    /// Whether the file is executable
    pub is_executable: bool,
}

/// Output directory from an action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDirectory {
    /// Path relative to working directory
    pub path: String,

    /// Tree digest (recursive directory structure)
    pub tree_digest: ContentHash,
}

/// Execution metadata (timing, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetadata {
    /// Execution start timestamp (milliseconds since epoch)
    pub execution_start_timestamp: u64,

    /// Execution completed timestamp
    pub execution_completed_timestamp: u64,

    /// Queue time (milliseconds)
    pub queued_timestamp: Option<u64>,

    /// Worker hostname
    pub worker: Option<String>,
}

/// Local cache implementation
struct LocalCache {
    cache_dir: PathBuf,
}

impl LocalCache {
    fn new(cache_dir: &Path) -> ExecutionResult<Self> {
        std::fs::create_dir_all(cache_dir)?;
        std::fs::create_dir_all(cache_dir.join("ac"))?; // action cache
        std::fs::create_dir_all(cache_dir.join("cas"))?; // content-addressable storage

        Ok(Self {
            cache_dir: cache_dir.to_path_buf(),
        })
    }

    fn get_action_result(&self, digest: &ContentHash) -> ExecutionResult<Option<ActionResult>> {
        let path = self.action_cache_path(digest);

        if !path.exists() {
            return Ok(None);
        }

        let data = std::fs::read(&path)?;
        let result: ActionResult = serde_json::from_slice(&data)
            .map_err(|e| super::types::ExecutionError::CacheError(e.to_string()))?;

        Ok(Some(result))
    }

    fn put_action_result(&self, digest: &ContentHash, result: &ActionResult) -> ExecutionResult<()> {
        let path = self.action_cache_path(digest);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let data = serde_json::to_vec(result)
            .map_err(|e| super::types::ExecutionError::CacheError(e.to_string()))?;

        std::fs::write(&path, data)?;

        Ok(())
    }

    fn get_blob(&self, digest: &ContentHash) -> ExecutionResult<Option<Vec<u8>>> {
        let path = self.cas_path(digest);

        if !path.exists() {
            return Ok(None);
        }

        let data = std::fs::read(&path)?;
        Ok(Some(data))
    }

    fn put_blob(&self, digest: &ContentHash, data: &[u8]) -> ExecutionResult<()> {
        let path = self.cas_path(digest);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, data)?;

        Ok(())
    }

    fn action_cache_path(&self, digest: &ContentHash) -> PathBuf {
        let hex = digest.to_hex();
        // Shard by first 4 chars for better filesystem performance
        self.cache_dir
            .join("ac")
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex)
    }

    fn cas_path(&self, digest: &ContentHash) -> PathBuf {
        let hex = digest.to_hex();
        self.cache_dir
            .join("cas")
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex)
    }
}

/// Convert TaskOutput to Bazel ActionResult
pub fn task_output_to_action_result(
    output: &TaskOutput,
    start_time: u64,
    end_time: u64,
) -> ActionResult {
    let output_files = output
        .output_files
        .iter()
        .map(|(path, digest)| OutputFile {
            path: path.to_string_lossy().to_string(),
            digest: digest.clone(),
            is_executable: false, // TODO: Check file permissions
        })
        .collect();

    ActionResult {
        output_files,
        output_directories: Vec::new(), // TODO: Handle directories
        exit_code: 0,
        stdout_digest: None, // TODO: Store stdout/stderr as blobs
        stderr_digest: None,
        execution_metadata: ExecutionMetadata {
            execution_start_timestamp: start_time,
            execution_completed_timestamp: end_time,
            queued_timestamp: None,
            worker: Some(hostname::get().ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "unknown".to_string())),
        },
    }
}

/// Convert TaskSignature to action digest
pub fn task_signature_to_digest(signature: &mut TaskSignature) -> ContentHash {
    signature.compute()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_local_cache() {
        let temp = TempDir::new().unwrap();
        let cache = LocalCache::new(temp.path()).unwrap();

        let digest = ContentHash::from_bytes(b"test");
        let data = b"test data";

        cache.put_blob(&digest, data).unwrap();

        let retrieved = cache.get_blob(&digest).unwrap();
        assert_eq!(retrieved, Some(data.to_vec()));
    }

    #[test]
    fn test_action_result_serialization() {
        let result = ActionResult {
            output_files: vec![OutputFile {
                path: "output.txt".to_string(),
                digest: ContentHash::from_bytes(b"content"),
                is_executable: false,
            }],
            output_directories: vec![],
            exit_code: 0,
            stdout_digest: None,
            stderr_digest: None,
            execution_metadata: ExecutionMetadata {
                execution_start_timestamp: 1000,
                execution_completed_timestamp: 2000,
                queued_timestamp: None,
                worker: Some("worker1".to_string()),
            },
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ActionResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.exit_code, 0);
        assert_eq!(deserialized.output_files.len(), 1);
    }
}
