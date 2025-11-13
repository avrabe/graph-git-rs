//! Content-addressable storage and action cache

use super::types::{ContentHash, TaskOutput, ExecutionError, ExecutionResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Content-Addressable Store - stores files by their SHA-256 hash
pub struct ContentAddressableStore {
    root: PathBuf,
    /// In-memory index for fast lookups
    index: HashMap<ContentHash, PathBuf>,
}

impl ContentAddressableStore {
    /// Create or open a CAS at the given root directory
    pub fn new(root: impl Into<PathBuf>) -> ExecutionResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;

        let mut cas = Self {
            root,
            index: HashMap::new(),
        };

        // Rebuild index by scanning filesystem
        cas.rebuild_index()?;

        Ok(cas)
    }

    /// Store content and return its hash
    pub fn put(&mut self, content: &[u8]) -> ExecutionResult<ContentHash> {
        let hash = ContentHash::from_bytes(content);
        let path = self.hash_to_path(&hash);

        // Skip if already exists
        if path.exists() {
            return Ok(hash);
        }

        // Create parent directories
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write atomically (write to temp, then rename)
        let temp_path = path.with_extension("tmp");
        fs::write(&temp_path, content)?;
        fs::rename(&temp_path, &path)?;

        // Update index
        self.index.insert(hash.clone(), path);

        Ok(hash)
    }

    /// Retrieve content by hash
    pub fn get(&self, hash: &ContentHash) -> ExecutionResult<Vec<u8>> {
        let path = self.hash_to_path(hash);
        fs::read(&path).map_err(|e| {
            ExecutionError::CacheError(format!("Failed to read {}: {}", hash, e))
        })
    }

    /// Check if hash exists in store
    pub fn contains(&self, hash: &ContentHash) -> bool {
        self.hash_to_path(hash).exists()
    }

    /// Store a file and return its hash
    pub fn put_file(&mut self, path: &Path) -> ExecutionResult<ContentHash> {
        let content = fs::read(path)?;
        self.put(&content)
    }

    /// Restore file from hash to destination
    pub fn get_file(&self, hash: &ContentHash, dest: &Path) -> ExecutionResult<()> {
        let source = self.hash_to_path(hash);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy(&source, dest)?;
        Ok(())
    }

    /// Hard-link file from CAS to destination (zero-copy)
    pub fn link_file(&self, hash: &ContentHash, dest: &Path) -> ExecutionResult<()> {
        let source = self.hash_to_path(hash);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        // Try hard link first (fast), fall back to copy
        match fs::hard_link(&source, dest) {
            Ok(_) => Ok(()),
            Err(_) => {
                fs::copy(&source, dest)?;
                Ok(())
            }
        }
    }

    /// Map content hash to filesystem path
    fn hash_to_path(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.to_hex();
        // Use first 2 bytes for directory sharding (00-ff)
        self.root
            .join("sha256")
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex)
    }

    /// Rebuild index by scanning filesystem
    fn rebuild_index(&mut self) -> ExecutionResult<()> {
        let sha256_dir = self.root.join("sha256");
        if !sha256_dir.exists() {
            return Ok(());
        }

        // Walk directory tree
        for entry in walkdir::WalkDir::new(&sha256_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let path = entry.path();
                // Extract hash from filename
                if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                    // Skip temp files
                    if filename.ends_with(".tmp") {
                        let _ = fs::remove_file(path); // Clean up temp files
                        continue;
                    }
                    let hash = ContentHash::from_hex(filename);
                    self.index.insert(hash, path.to_path_buf());
                }
            }
        }

        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            object_count: self.index.len(),
            total_size_bytes: self.compute_total_size(),
        }
    }

    fn compute_total_size(&self) -> u64 {
        self.index
            .values()
            .filter_map(|path| fs::metadata(path).ok())
            .map(|meta| meta.len())
            .sum()
    }

    /// Garbage collect unused objects (placeholder)
    pub fn gc(&mut self, _keep: &[ContentHash]) -> ExecutionResult<usize> {
        // TODO: Implement garbage collection
        Ok(0)
    }
}

/// Cache statistics
#[derive(Debug)]
pub struct CacheStats {
    pub object_count: usize,
    pub total_size_bytes: u64,
}

/// Action cache - maps task signatures to outputs
pub struct ActionCache {
    root: PathBuf,
    /// In-memory cache
    cache: HashMap<ContentHash, TaskOutput>,
}

impl ActionCache {
    /// Create or open an action cache
    pub fn new(root: impl Into<PathBuf>) -> ExecutionResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;

        let mut cache = Self {
            root,
            cache: HashMap::new(),
        };

        // Load existing cache entries
        cache.load_from_disk()?;

        Ok(cache)
    }

    /// Look up cached result by signature
    pub fn get(&self, signature: &ContentHash) -> Option<&TaskOutput> {
        self.cache.get(signature)
    }

    /// Store task output
    pub fn put(&mut self, signature: ContentHash, output: TaskOutput) -> ExecutionResult<()> {
        // Write to disk
        let path = self.signature_to_path(&signature);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(&output)?;
        fs::write(&path, json)?;

        // Update in-memory cache
        self.cache.insert(signature, output);

        Ok(())
    }

    /// Check if signature exists
    pub fn contains(&self, signature: &ContentHash) -> bool {
        self.cache.contains_key(signature)
    }

    /// Invalidate an entry
    pub fn invalidate(&mut self, signature: &ContentHash) -> ExecutionResult<()> {
        self.cache.remove(signature);
        let path = self.signature_to_path(signature);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Clear entire cache
    pub fn clear(&mut self) -> ExecutionResult<()> {
        self.cache.clear();
        if self.root.exists() {
            fs::remove_dir_all(&self.root)?;
            fs::create_dir_all(&self.root)?;
        }
        Ok(())
    }

    fn signature_to_path(&self, signature: &ContentHash) -> PathBuf {
        let hex = signature.to_hex();
        self.root
            .join(&hex[0..2])
            .join(format!("{}.json", hex))
    }

    fn load_from_disk(&mut self) -> ExecutionResult<()> {
        if !self.root.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() && entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                let json = fs::read_to_string(entry.path())?;
                if let Ok(output) = serde_json::from_str::<TaskOutput>(&json) {
                    self.cache.insert(output.signature.clone(), output);
                }
            }
        }

        Ok(())
    }

    pub fn stats(&self) -> ActionCacheStats {
        ActionCacheStats {
            entry_count: self.cache.len(),
        }
    }
}

#[derive(Debug)]
pub struct ActionCacheStats {
    pub entry_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cas_put_get() {
        let tmp = TempDir::new().unwrap();
        let mut cas = ContentAddressableStore::new(tmp.path()).unwrap();

        let content = b"Hello, Bitzel!";
        let hash = cas.put(content).unwrap();

        let retrieved = cas.get(&hash).unwrap();
        assert_eq!(retrieved, content);
    }

    #[test]
    fn test_cas_contains() {
        let tmp = TempDir::new().unwrap();
        let mut cas = ContentAddressableStore::new(tmp.path()).unwrap();

        let content = b"test";
        let hash = cas.put(content).unwrap();

        assert!(cas.contains(&hash));
        assert!(!cas.contains(&ContentHash::from_hex("deadbeef")));
    }

    #[test]
    fn test_action_cache() {
        let tmp = TempDir::new().unwrap();
        let mut cache = ActionCache::new(tmp.path()).unwrap();

        let sig = ContentHash::from_bytes(b"test-signature");
        let output = TaskOutput {
            signature: sig.clone(),
            output_files: HashMap::new(),
            stdout: "success".to_string(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 100,
        };

        cache.put(sig.clone(), output.clone()).unwrap();
        let retrieved = cache.get(&sig).unwrap();

        assert_eq!(retrieved.stdout, "success");
        assert_eq!(retrieved.exit_code, 0);
    }

    #[test]
    fn test_action_cache_persistence() {
        let tmp = TempDir::new().unwrap();

        let sig = ContentHash::from_bytes(b"persistent");
        let output = TaskOutput {
            signature: sig.clone(),
            output_files: HashMap::new(),
            stdout: "persistent".to_string(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 50,
        };

        // Write to cache
        {
            let mut cache = ActionCache::new(tmp.path()).unwrap();
            cache.put(sig.clone(), output).unwrap();
        }

        // Reload cache
        {
            let cache = ActionCache::new(tmp.path()).unwrap();
            let retrieved = cache.get(&sig).unwrap();
            assert_eq!(retrieved.stdout, "persistent");
        }
    }
}
