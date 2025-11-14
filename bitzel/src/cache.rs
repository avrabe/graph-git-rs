//! Cache management using Bazel Remote Execution API v2

use crate::Result;
use convenient_cache::{BazelRemoteCache, ContentHash};

/// Manages build artifact caching
pub struct CacheManager {
    cache: Option<BazelRemoteCache>,
    hits: usize,
    misses: usize,
}

impl CacheManager {
    /// Create a new cache manager
    ///
    /// If `cache_url` is None, caching will be disabled
    pub fn new(cache_url: Option<&str>) -> Result<Self> {
        let cache = if let Some(url) = cache_url {
            tracing::info!("Connecting to cache: {}", url);
            Some(BazelRemoteCache::new(url)?)
        } else {
            tracing::warn!("No cache configured - all tasks will execute");
            None
        };

        Ok(Self {
            cache,
            hits: 0,
            misses: 0,
        })
    }

    /// Check if a blob exists in the cache
    pub async fn has(&mut self, content_hash: &str) -> Result<bool> {
        if let Some(cache) = &self.cache {
            let hash = ContentHash::new(content_hash.to_string())?;
            let exists = cache.has_blob(&hash).await?;

            if exists {
                self.hits += 1;
            } else {
                self.misses += 1;
            }

            Ok(exists)
        } else {
            self.misses += 1;
            Ok(false)
        }
    }

    /// Store a blob in the cache
    pub async fn put(&mut self, content: &[u8]) -> Result<String> {
        if let Some(cache) = &self.cache {
            let hash = cache.put_blob(content).await?;
            Ok(hash.as_str().to_string())
        } else {
            // No cache configured, return a dummy hash
            Ok("no-cache".to_string())
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize, f64) {
        let total = self.hits + self.misses;
        let hit_rate = if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        };

        (self.hits, self.misses, hit_rate)
    }

    /// Check if cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.cache.is_some()
    }
}
