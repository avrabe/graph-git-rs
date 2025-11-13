//! Cache management commands - Bazel-inspired CLI for Bitzel cache

use super::cache::{ActionCache, ContentAddressableStore};
use super::types::ExecutionResult;
use std::path::Path;

/// Cache management operations
pub struct CacheManager {
    cache_dir: std::path::PathBuf,
}

impl CacheManager {
    /// Create a cache manager for a given cache directory
    pub fn new(cache_dir: impl AsRef<Path>) -> Self {
        Self {
            cache_dir: cache_dir.as_ref().to_path_buf(),
        }
    }

    /// Clean cache - remove action cache but keep CAS
    /// Similar to `bazel clean`
    pub fn clean(&self) -> ExecutionResult<CleanStats> {
        let action_cache_dir = self.cache_dir.join("action-cache");
        let mut stats = CleanStats::default();

        if action_cache_dir.exists() {
            stats.action_cache_entries = count_files(&action_cache_dir);
            std::fs::remove_dir_all(&action_cache_dir)?;
            std::fs::create_dir_all(&action_cache_dir)?;
        }

        Ok(stats)
    }

    /// Expunge cache - remove everything including CAS
    /// Similar to `bazel clean --expunge`
    pub fn expunge(&self) -> ExecutionResult<ExpungeStats> {
        let mut stats = ExpungeStats::default();

        if self.cache_dir.exists() {
            // Count before removing
            let cas_dir = self.cache_dir.join("cas");
            let action_cache_dir = self.cache_dir.join("action-cache");
            let sandbox_dir = self.cache_dir.join("sandboxes");

            if cas_dir.exists() {
                stats.cas_objects = count_files(&cas_dir);
                stats.cas_bytes = dir_size(&cas_dir);
            }
            if action_cache_dir.exists() {
                stats.action_cache_entries = count_files(&action_cache_dir);
            }
            if sandbox_dir.exists() {
                stats.sandboxes = count_dirs(&sandbox_dir);
            }

            // Remove everything
            std::fs::remove_dir_all(&self.cache_dir)?;
        }

        Ok(stats)
    }

    /// Query cache statistics
    /// Similar to `bazel info`
    pub fn query(&self) -> ExecutionResult<CacheQuery> {
        let cas_dir = self.cache_dir.join("cas");
        let action_cache_dir = self.cache_dir.join("action-cache");
        let sandbox_dir = self.cache_dir.join("sandboxes");

        let mut query = CacheQuery {
            cache_dir: self.cache_dir.clone(),
            ..Default::default()
        };

        if cas_dir.exists() {
            query.cas_objects = count_files(&cas_dir);
            query.cas_bytes = dir_size(&cas_dir);
        }

        if action_cache_dir.exists() {
            query.action_cache_entries = count_files(&action_cache_dir);
        }

        if sandbox_dir.exists() {
            query.active_sandboxes = count_dirs(&sandbox_dir);
        }

        Ok(query)
    }

    /// GC - Remove unreferenced CAS objects
    /// (placeholder for future implementation)
    pub fn gc(&self) -> ExecutionResult<GcStats> {
        // TODO: Implement mark-and-sweep GC
        // 1. Load all action cache entries
        // 2. Mark all referenced CAS objects
        // 3. Sweep unreferenced objects

        Ok(GcStats {
            objects_removed: 0,
            bytes_freed: 0,
        })
    }
}

#[derive(Debug, Default)]
pub struct CleanStats {
    pub action_cache_entries: usize,
}

#[derive(Debug, Default)]
pub struct ExpungeStats {
    pub cas_objects: usize,
    pub cas_bytes: u64,
    pub action_cache_entries: usize,
    pub sandboxes: usize,
}

#[derive(Debug, Default)]
pub struct CacheQuery {
    pub cache_dir: std::path::PathBuf,
    pub cas_objects: usize,
    pub cas_bytes: u64,
    pub action_cache_entries: usize,
    pub active_sandboxes: usize,
}

#[derive(Debug, Default)]
pub struct GcStats {
    pub objects_removed: usize,
    pub bytes_freed: u64,
}

// Helper functions

fn count_files(dir: &Path) -> usize {
    walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count()
}

fn count_dirs(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

fn dir_size(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_clean() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        std::fs::create_dir_all(cache_dir.join("action-cache")).unwrap();
        std::fs::write(cache_dir.join("action-cache/test.json"), b"{}").unwrap();

        let manager = CacheManager::new(&cache_dir);
        let stats = manager.clean().unwrap();

        assert_eq!(stats.action_cache_entries, 1);
        assert!(!cache_dir.join("action-cache/test.json").exists());
    }

    #[test]
    fn test_expunge() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        std::fs::create_dir_all(cache_dir.join("cas")).unwrap();
        std::fs::write(cache_dir.join("cas/test"), b"data").unwrap();

        let manager = CacheManager::new(&cache_dir);
        let stats = manager.expunge().unwrap();

        assert!(stats.cas_objects > 0);
        assert!(!cache_dir.exists());
    }

    #[test]
    fn test_query() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        std::fs::create_dir_all(cache_dir.join("cas")).unwrap();
        std::fs::write(cache_dir.join("cas/obj1"), b"data").unwrap();

        let manager = CacheManager::new(&cache_dir);
        let query = manager.query().unwrap();

        assert_eq!(query.cas_objects, 1);
        assert!(query.cas_bytes > 0);
    }
}
