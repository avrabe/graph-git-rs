//! Bazel Remote Execution API v2 cache client
//!
//! This crate provides a client for interacting with bazel-remote cache servers,
//! implementing the Bazel Remote Execution API v2 HTTP protocol.
//!
//! # Features
//!
//! - Content-addressable storage (CAS) operations
//! - Action cache for build artifacts
//! - SHA256-based content hashing
//! - Async HTTP client using reqwest
//! - Type-safe error handling
//!
//! # Example
//!
//! ```no_run
//! use convenient_cache::BazelRemoteCache;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let cache = BazelRemoteCache::new("http://localhost:9090")?;
//!
//!     // Store content
//!     let content = b"Hello, cache!";
//!     let hash = cache.put_blob(content).await?;
//!     println!("Stored with hash: {}", hash);
//!
//!     // Retrieve content
//!     let retrieved = cache.get_blob(&hash).await?;
//!     assert_eq!(content, retrieved.as_slice());
//!
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]
#![warn(unused_results)]

pub mod grpc_client;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Error types for cache operations
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// HTTP request failed
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Content not found in cache
    #[error("Content not found: {0}")]
    NotFound(String),

    /// Invalid hash format
    #[error("Invalid hash: {0}")]
    InvalidHash(String),

    /// Cache server error
    #[error("Server error: {0}")]
    ServerError(String),

    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

/// Result type for cache operations
pub type CacheResult<T> = Result<T, CacheError>;

/// Content hash (SHA256)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash(String);

impl ContentHash {
    /// Create a new content hash from a string
    ///
    /// # Errors
    ///
    /// Returns `CacheError::InvalidHash` if the hash is not a valid SHA256 hex string
    pub fn new(hash: String) -> CacheResult<Self> {
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(CacheError::InvalidHash(hash));
        }
        Ok(Self(hash))
    }

    /// Calculate SHA256 hash of content
    #[must_use]
    pub fn from_content(content: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(content);
        let result = hasher.finalize();
        Self(hex::encode(result))
    }

    /// Get the hash as a string
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Action cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCacheEntry {
    /// Hash of the action
    pub action_hash: ContentHash,
    /// Hash of the result
    pub result_hash: ContentHash,
    /// Timestamp when cached
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

/// Bazel Remote Cache client
///
/// Implements the Bazel Remote Execution API v2 HTTP protocol for
/// content-addressable storage and action caching.
#[derive(Clone)]
pub struct BazelRemoteCache {
    base_url: String,
    client: Client,
}

impl BazelRemoteCache {
    /// Create a new cache client
    ///
    /// # Arguments
    ///
    /// * `base_url` - Base URL of the cache server (e.g., "<http://localhost:9090>")
    ///
    /// # Errors
    ///
    /// Returns `CacheError::InvalidUrl` if the URL is malformed
    pub fn new(base_url: &str) -> CacheResult<Self> {
        let base_url = base_url.trim_end_matches('/').to_string();

        // Validate URL
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(CacheError::InvalidUrl(base_url));
        }

        Ok(Self {
            base_url,
            client: Client::new(),
        })
    }

    /// Store a blob in the content-addressable storage
    ///
    /// # Arguments
    ///
    /// * `content` - The content to store
    ///
    /// # Returns
    ///
    /// The SHA256 hash of the stored content
    ///
    /// # Errors
    ///
    /// Returns `CacheError::Http` if the request fails
    /// Returns `CacheError::ServerError` if the server returns an error
    pub async fn put_blob(&self, content: &[u8]) -> CacheResult<ContentHash> {
        let hash = ContentHash::from_content(content);
        let url = format!("{}/cas/{}", self.base_url, hash);

        let response = self.client
            .put(&url)
            .body(content.to_vec())
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(CacheError::ServerError(format!(
                "Failed to store blob: {}",
                response.status()
            )));
        }

        Ok(hash)
    }

    /// Retrieve a blob from the content-addressable storage
    ///
    /// # Arguments
    ///
    /// * `hash` - The SHA256 hash of the content to retrieve
    ///
    /// # Returns
    ///
    /// The content bytes
    ///
    /// # Errors
    ///
    /// Returns `CacheError::Http` if the request fails
    /// Returns `CacheError::NotFound` if the content doesn't exist
    /// Returns `CacheError::ServerError` if the server returns an error
    pub async fn get_blob(&self, hash: &ContentHash) -> CacheResult<Vec<u8>> {
        let url = format!("{}/cas/{}", self.base_url, hash);

        let response = self.client
            .get(&url)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(CacheError::NotFound(hash.to_string()));
        }

        if !response.status().is_success() {
            return Err(CacheError::ServerError(format!(
                "Failed to retrieve blob: {}",
                response.status()
            )));
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Check if a blob exists in the cache
    ///
    /// # Arguments
    ///
    /// * `hash` - The SHA256 hash to check
    ///
    /// # Returns
    ///
    /// `true` if the blob exists, `false` otherwise
    ///
    /// # Errors
    ///
    /// Returns `CacheError::Http` if the request fails
    pub async fn has_blob(&self, hash: &ContentHash) -> CacheResult<bool> {
        let url = format!("{}/cas/{}", self.base_url, hash);

        let response = self.client
            .head(&url)
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// Store an action cache entry
    ///
    /// # Arguments
    ///
    /// * `entry` - The action cache entry to store
    ///
    /// # Errors
    ///
    /// Returns `CacheError::Http` if the request fails
    /// Returns `CacheError::ServerError` if the server returns an error
    pub async fn put_action(&self, entry: &ActionCacheEntry) -> CacheResult<()> {
        let url = format!("{}/ac/{}", self.base_url, entry.action_hash);

        let response = self.client
            .put(&url)
            .json(entry)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(CacheError::ServerError(format!(
                "Failed to store action: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Retrieve an action cache entry
    ///
    /// # Arguments
    ///
    /// * `action_hash` - The hash of the action to retrieve
    ///
    /// # Returns
    ///
    /// The action cache entry if it exists
    ///
    /// # Errors
    ///
    /// Returns `CacheError::Http` if the request fails
    /// Returns `CacheError::NotFound` if the action doesn't exist
    pub async fn get_action(&self, action_hash: &ContentHash) -> CacheResult<ActionCacheEntry> {
        let url = format!("{}/ac/{}", self.base_url, action_hash);

        let response = self.client
            .get(&url)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(CacheError::NotFound(action_hash.to_string()));
        }

        if !response.status().is_success() {
            return Err(CacheError::ServerError(format!(
                "Failed to retrieve action: {}",
                response.status()
            )));
        }

        let entry = response.json().await?;
        Ok(entry)
    }

    /// Get cache statistics
    ///
    /// # Returns
    ///
    /// Cache statistics as a JSON value
    ///
    /// # Errors
    ///
    /// Returns `CacheError::Http` if the request fails
    pub async fn get_stats(&self) -> CacheResult<serde_json::Value> {
        let url = format!("{}/status", self.base_url);

        let response = self.client
            .get(&url)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(CacheError::ServerError(format!(
                "Failed to get stats: {}",
                response.status()
            )));
        }

        let stats = response.json().await?;
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_hash_from_content() {
        let content = b"Hello, World!";
        let hash = ContentHash::from_content(content);

        // SHA256 of "Hello, World!" is known
        assert_eq!(hash.as_str().len(), 64);
        assert!(hash.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_content_hash_validation() {
        // Valid hash
        let valid = "a".repeat(64);
        assert!(ContentHash::new(valid).is_ok());

        // Invalid length
        let invalid_length = "a".repeat(63);
        assert!(ContentHash::new(invalid_length).is_err());

        // Invalid characters
        let invalid_chars = "z".repeat(64);
        assert!(ContentHash::new(invalid_chars).is_err());
    }

    #[test]
    fn test_cache_client_creation() {
        assert!(BazelRemoteCache::new("http://localhost:9090").is_ok());
        assert!(BazelRemoteCache::new("https://cache.example.com").is_ok());
        assert!(BazelRemoteCache::new("invalid-url").is_err());
    }

    #[test]
    fn test_content_hash_consistency() {
        let content = b"test content";
        let hash1 = ContentHash::from_content(content);
        let hash2 = ContentHash::from_content(content);
        assert_eq!(hash1, hash2);

        let different = b"different content";
        let hash3 = ContentHash::from_content(different);
        assert_ne!(hash1, hash3);
    }
}
