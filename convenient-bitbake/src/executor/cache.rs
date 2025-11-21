//! Content-addressable storage and action cache

use super::types::{ContentHash, TaskOutput, ExecutionError, ExecutionResult};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::os::unix::fs::OpenOptionsExt;
use std::time::SystemTime;

/// Write data to a file atomically with fsync for durability
///
/// This implements the write-fsync-rename pattern for atomic writes:
/// 1. Write data to a temporary file
/// 2. fsync the temp file (flush to disk)
/// 3. Rename temp file to final destination (atomic operation)
/// 4. fsync the parent directory (ensure directory entry is durable)
fn atomic_write(path: &Path, data: &[u8]) -> ExecutionResult<()> {
    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write to temp file
    let temp_path = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(&temp_path)
        .map_err(|e| ExecutionError::CacheError(format!("Failed to create temp file: {e}")))?;

    file.write_all(data)
        .map_err(|e| ExecutionError::CacheError(format!("Failed to write data: {e}")))?;

    // CRITICAL: fsync the file data to disk before rename
    file.sync_all()
        .map_err(|e| ExecutionError::CacheError(format!("Failed to fsync file: {e}")))?;

    // Close file before rename
    drop(file);

    // Atomic rename (POSIX guarantees atomicity)
    fs::rename(&temp_path, path)
        .map_err(|e| ExecutionError::CacheError(format!("Failed to rename: {e}")))?;

    // CRITICAL: fsync parent directory to ensure directory entry is durable
    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all(); // Best effort - some filesystems don't support this
        }

    Ok(())
}

/// Acquire an exclusive lock on a file
///
/// Returns a file handle that holds the lock. The lock is released when the file is dropped.
#[cfg(unix)]
fn acquire_lock(path: &Path) -> ExecutionResult<File> {
    use std::os::unix::io::AsRawFd;
    use nix::fcntl::{flock, FlockArg};

    // Create lock directory if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .map_err(|e| ExecutionError::CacheError(format!("Failed to create lock file: {e}")))?;

    // Acquire exclusive lock (blocks if already locked)
    flock(lock_file.as_raw_fd(), FlockArg::LockExclusive)
        .map_err(|e| ExecutionError::CacheError(format!("Failed to acquire lock: {e}")))?;

    Ok(lock_file)
}

/// Cache configuration for garbage collection
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum total cache size in bytes (None = unlimited)
    pub max_size_bytes: Option<u64>,
    /// Trigger GC when cache exceeds this size
    pub gc_threshold_bytes: u64,
    /// Target size after GC (should be < gc_threshold)
    pub gc_target_bytes: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: Some(10 * 1024 * 1024 * 1024), // 10 GB default
            gc_threshold_bytes: 8 * 1024 * 1024 * 1024,     // Trigger at 8 GB
            gc_target_bytes: 6 * 1024 * 1024 * 1024,        // Clean down to 6 GB
        }
    }
}

/// Metadata for cached objects (used for GC and LRU)
#[derive(Debug, Clone)]
struct ObjectMetadata {
    path: PathBuf,
    size_bytes: u64,
    last_access_time: SystemTime,
}

/// Content-Addressable Store - stores files by their SHA-256 hash
pub struct ContentAddressableStore {
    root: PathBuf,
    /// In-memory index with metadata for GC/LRU
    index: HashMap<ContentHash, ObjectMetadata>,
    /// Cache configuration
    config: CacheConfig,
}

impl ContentAddressableStore {
    /// Create or open a CAS at the given root directory
    pub fn new(root: impl Into<PathBuf>) -> ExecutionResult<Self> {
        Self::with_config(root, CacheConfig::default())
    }

    /// Create or open a CAS with custom configuration
    pub fn with_config(root: impl Into<PathBuf>, config: CacheConfig) -> ExecutionResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;

        let mut cas = Self {
            root,
            index: HashMap::new(),
            config,
        };

        // Rebuild index by scanning filesystem
        cas.rebuild_index()?;

        Ok(cas)
    }

    /// Store content and return its hash
    pub fn put(&mut self, content: &[u8]) -> ExecutionResult<ContentHash> {
        let hash = ContentHash::from_bytes(content);
        let path = self.hash_to_path(&hash);

        // Skip if already exists (update access time)
        if path.exists() {
            if let Some(metadata) = self.index.get_mut(&hash) {
                metadata.last_access_time = SystemTime::now();
            }
            return Ok(hash);
        }

        // Acquire lock to prevent concurrent writes to same hash
        #[cfg(unix)]
        let lock_path = path.with_extension("lock");
        #[cfg(unix)]
        let _lock = acquire_lock(&lock_path)?;

        // Double-check after acquiring lock (another process might have written it)
        if path.exists() {
            return Ok(hash);
        }

        // Write atomically with fsync for durability
        atomic_write(&path, content)?;

        // Update index with metadata
        let metadata = ObjectMetadata {
            path: path.clone(),
            size_bytes: content.len() as u64,
            last_access_time: SystemTime::now(),
        };
        self.index.insert(hash.clone(), metadata);

        // Trigger GC if cache is too large
        self.gc_if_needed()?;

        Ok(hash)
    }

    /// Retrieve content by hash
    pub fn get(&mut self, hash: &ContentHash) -> ExecutionResult<Vec<u8>> {
        let path = self.hash_to_path(hash);

        // Update access time for LRU tracking
        if let Some(metadata) = self.index.get_mut(hash) {
            metadata.last_access_time = SystemTime::now();
        }

        fs::read(&path).map_err(|e| {
            ExecutionError::CacheError(format!("Failed to read {hash}: {e}"))
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
        if let Ok(()) = fs::hard_link(&source, dest) { Ok(()) } else {
            fs::copy(&source, dest)?;
            Ok(())
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
            .filter_map(std::result::Result::ok)
        {
            if entry.file_type().is_file() {
                let path = entry.path();
                // Extract hash from filename
                if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                    // Skip temp and lock files
                    if filename.ends_with(".tmp") || filename.ends_with(".lock") {
                        let _ = fs::remove_file(path); // Clean up temp/lock files
                        continue;
                    }

                    // Get file metadata
                    if let Ok(file_metadata) = fs::metadata(path) {
                        let hash = ContentHash::from_hex(filename);
                        let obj_metadata = ObjectMetadata {
                            path: path.to_path_buf(),
                            size_bytes: file_metadata.len(),
                            last_access_time: file_metadata.modified()
                                .unwrap_or_else(|_| SystemTime::now()),
                        };
                        self.index.insert(hash, obj_metadata);
                    }
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
            .map(|metadata| metadata.size_bytes)
            .sum()
    }

    /// Trigger GC if cache exceeds threshold
    ///
    /// Automatic GC only performs LRU eviction (not mark-and-sweep) because
    /// it doesn't have access to the ActionCache to determine reachability.
    /// For proper garbage collection, use gc_with_action_cache() explicitly.
    fn gc_if_needed(&mut self) -> ExecutionResult<()> {
        let total_size = self.compute_total_size();

        if total_size > self.config.gc_threshold_bytes {
            let bytes_to_free = total_size.saturating_sub(self.config.gc_target_bytes);
            eprintln!(
                "Cache size {} bytes exceeds threshold {} bytes, performing LRU eviction",
                total_size, self.config.gc_threshold_bytes
            );
            eprintln!("Target: free {} bytes to reach {} bytes", bytes_to_free, self.config.gc_target_bytes);

            // Only do LRU eviction in automatic GC (no mark-and-sweep)
            let evicted = self.evict_lru(bytes_to_free)?;
            eprintln!("Automatic GC: evicted {evicted} oldest objects");
        }

        Ok(())
    }

    /// Garbage collect: mark-and-sweep + LRU eviction
    ///
    /// This is a manual GC operation. Provide a list of content hashes that must
    /// be kept (all others will be deleted as unreachable). After sweep, if the
    /// cache is still over the target size, LRU eviction will be performed.
    ///
    /// For typical usage, prefer `gc_with_action_cache()` which automatically
    /// determines reachable objects from the ActionCache.
    pub fn gc(&mut self, keep: &[ContentHash]) -> ExecutionResult<usize> {
        let initial_count = self.index.len();
        let initial_size = self.compute_total_size();

        eprintln!(
            "Starting GC: {initial_count} objects, {initial_size} bytes"
        );

        // Mark phase: identify objects to keep
        let reachable: HashSet<ContentHash> = keep.iter().cloned().collect();

        // Sweep phase: delete unreachable objects
        let mut deleted = 0;
        let mut hashes_to_remove = Vec::new();

        for (hash, metadata) in &self.index {
            if !reachable.contains(hash) {
                // Delete from disk
                if let Err(e) = fs::remove_file(&metadata.path) {
                    eprintln!("Warning: Failed to delete {hash}: {e}");
                } else {
                    hashes_to_remove.push(hash.clone());
                    deleted += 1;
                }
            }
        }

        // Remove from index
        for hash in &hashes_to_remove {
            self.index.remove(hash);
        }

        let size_after_sweep = self.compute_total_size();

        eprintln!(
            "Sweep complete: deleted {deleted} unreachable objects, size now {size_after_sweep} bytes"
        );

        // LRU eviction: if still over target, evict oldest objects
        if size_after_sweep > self.config.gc_target_bytes {
            let evicted = self.evict_lru(size_after_sweep - self.config.gc_target_bytes)?;
            eprintln!("LRU eviction: removed {evicted} objects");
            deleted += evicted;
        }

        let final_size = self.compute_total_size();
        eprintln!(
            "GC complete: {} → {} objects, {} → {} bytes",
            initial_count,
            self.index.len(),
            initial_size,
            final_size
        );

        Ok(deleted)
    }

    /// Evict least recently used objects to free up space
    fn evict_lru(&mut self, bytes_to_free: u64) -> ExecutionResult<usize> {
        // Collect all objects with access times
        let mut objects: Vec<(ContentHash, SystemTime, u64)> = self
            .index
            .iter()
            .map(|(hash, metadata)| (hash.clone(), metadata.last_access_time, metadata.size_bytes))
            .collect();

        // Sort by access time (oldest first)
        objects.sort_by_key(|(_, access_time, _)| *access_time);

        let mut freed = 0u64;
        let mut evicted = 0;

        for (hash, _, size) in objects {
            if freed >= bytes_to_free {
                break;
            }

            // Remove object
            if let Some(metadata) = self.index.remove(&hash) {
                if let Err(e) = fs::remove_file(&metadata.path) {
                    eprintln!("Warning: Failed to evict {hash}: {e}");
                } else {
                    freed += size;
                    evicted += 1;
                }
            }
        }

        Ok(evicted)
    }

    /// Perform garbage collection with mark-and-sweep using ActionCache references
    ///
    /// This is the recommended way to run GC: it walks the ActionCache to find
    /// all referenced content hashes, then deletes unreachable objects.
    pub fn gc_with_action_cache(&mut self, action_cache: &ActionCache) -> ExecutionResult<usize> {
        // Mark phase: collect all reachable content hashes from action cache
        let reachable = action_cache.get_referenced_content_hashes();

        eprintln!(
            "Mark phase: found {} reachable content hashes from action cache",
            reachable.len()
        );

        // Run GC with the reachable set
        self.gc(&reachable.into_iter().collect::<Vec<_>>())
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
        let path = self.signature_to_path(&signature);

        // Acquire lock to prevent concurrent writes
        #[cfg(unix)]
        let lock_path = path.with_extension("lock");
        #[cfg(unix)]
        let _lock = acquire_lock(&lock_path)?;

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&output)?;

        // Write atomically with fsync for durability
        atomic_write(&path, json.as_bytes())?;

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
            .join(format!("{hex}.json"))
    }

    fn load_from_disk(&mut self) -> ExecutionResult<()> {
        if !self.root.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
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

    /// Get all content hashes referenced by cached task outputs
    ///
    /// This is used by GC to identify reachable objects in the CAS.
    pub fn get_referenced_content_hashes(&self) -> HashSet<ContentHash> {
        let mut hashes = HashSet::new();

        for output in self.cache.values() {
            // Add all output file hashes
            for hash in output.output_files.values() {
                hashes.insert(hash.clone());
            }
        }

        hashes
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
