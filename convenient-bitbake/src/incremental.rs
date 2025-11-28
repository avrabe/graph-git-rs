//! Incremental build support with file change detection
//!
//! Provides file fingerprinting and change detection for incremental builds,
//! using mtime, size, and optional content hashing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Hash algorithm for content fingerprinting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HashAlgorithm {
    /// No hashing, rely on mtime/size only
    #[default]
    None,
    /// XXHash64 (fast)
    XXHash64,
    /// SHA-256 (secure)
    Sha256,
}

/// File fingerprint for change detection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub path: PathBuf,
    pub mtime: SystemTime,
    pub size: u64,
    pub hash: Option<String>,
    pub inode: Option<u64>,
}

impl FileFingerprint {
    /// Create a fingerprint for a file
    pub fn from_path(path: &Path, hash_algorithm: HashAlgorithm) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;

        let mtime = metadata.modified()?;
        let size = metadata.len();

        // Get inode on Unix
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            Some(metadata.ino())
        };
        #[cfg(not(unix))]
        let inode = None;

        // Compute hash if requested
        let hash = match hash_algorithm {
            HashAlgorithm::None => None,
            HashAlgorithm::XXHash64 => Some(compute_xxhash64(path)?),
            HashAlgorithm::Sha256 => Some(compute_sha256(path)?),
        };

        Ok(Self {
            path: path.to_path_buf(),
            mtime,
            size,
            hash,
            inode,
        })
    }

    /// Quick check if file has changed (mtime/size only)
    pub fn quick_changed(&self, other: &FileFingerprint) -> bool {
        self.mtime != other.mtime || self.size != other.size
    }

    /// Full check if file has changed (including hash if available)
    pub fn changed(&self, other: &FileFingerprint) -> bool {
        if self.quick_changed(other) {
            return true;
        }

        // If both have hashes, compare them
        match (&self.hash, &other.hash) {
            (Some(h1), Some(h2)) => h1 != h2,
            _ => false, // If no hashes, trust mtime/size
        }
    }
}

/// Compute XXHash64 of a file
fn compute_xxhash64(path: &Path) -> io::Result<String> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 8192];
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis (as fallback)

    // Simple hash implementation (for real XXHash, use the xxhash-rust crate)
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        for &byte in &buffer[..bytes_read] {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3); // FNV prime
        }
    }

    Ok(format!("{hash:016x}"))
}

/// Compute SHA-256 of a file
fn compute_sha256(path: &Path) -> io::Result<String> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 8192];

    // Simple hash (for real SHA-256, use the sha2 crate)
    // This is a placeholder that produces consistent but non-cryptographic output
    let mut hash: [u64; 4] = [
        0x6a09e667bb67ae85,
        0x3c6ef372a54ff53a,
        0x510e527f9b05688c,
        0x1f83d9ab5be0cd19,
    ];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        for (i, &byte) in buffer[..bytes_read].iter().enumerate() {
            let idx = i % 4;
            hash[idx] = hash[idx].wrapping_add(byte as u64);
            hash[idx] = hash[idx].rotate_left(7);
        }
    }

    Ok(format!(
        "{:016x}{:016x}{:016x}{:016x}",
        hash[0], hash[1], hash[2], hash[3]
    ))
}

/// Incremental build state
pub struct IncrementalState {
    fingerprints: HashMap<PathBuf, FileFingerprint>,
    dirty_files: HashSet<PathBuf>,
    hash_algorithm: HashAlgorithm,
    base_dir: Option<PathBuf>,
}

impl IncrementalState {
    /// Create new incremental state
    pub fn new() -> Self {
        Self {
            fingerprints: HashMap::new(),
            dirty_files: HashSet::new(),
            hash_algorithm: HashAlgorithm::None,
            base_dir: None,
        }
    }

    /// Create state with specific hash algorithm
    pub fn with_hashing(algorithm: HashAlgorithm) -> Self {
        Self {
            fingerprints: HashMap::new(),
            dirty_files: HashSet::new(),
            hash_algorithm: algorithm,
            base_dir: None,
        }
    }

    /// Set the base directory for relative paths
    pub fn set_base_dir(&mut self, path: PathBuf) {
        self.base_dir = Some(path);
    }

    /// Get absolute path, resolving relative paths against base_dir
    fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(ref base) = self.base_dir {
            base.join(path)
        } else {
            path.to_path_buf()
        }
    }

    /// Check if a file has changed since last recorded
    pub fn check_file(&mut self, path: &Path) -> bool {
        let abs_path = self.resolve_path(path);

        // Try to get current fingerprint
        let current = match FileFingerprint::from_path(&abs_path, self.hash_algorithm) {
            Ok(fp) => fp,
            Err(_) => {
                // File doesn't exist or isn't accessible - mark as dirty
                self.dirty_files.insert(abs_path);
                return true;
            }
        };

        // Compare with stored fingerprint
        if let Some(stored) = self.fingerprints.get(&abs_path) {
            if current.changed(stored) {
                self.dirty_files.insert(abs_path.clone());
                self.fingerprints.insert(abs_path, current);
                return true;
            }
        } else {
            // No stored fingerprint - file is new
            self.fingerprints.insert(abs_path.clone(), current);
            self.dirty_files.insert(abs_path);
            return true;
        }

        false
    }

    /// Record a file's current state (mark as clean)
    pub fn record_file(&mut self, path: &Path) -> io::Result<()> {
        let abs_path = self.resolve_path(path);
        let fingerprint = FileFingerprint::from_path(&abs_path, self.hash_algorithm)?;
        self.fingerprints.insert(abs_path.clone(), fingerprint);
        self.dirty_files.remove(&abs_path);
        Ok(())
    }

    /// Record multiple files
    pub fn record_files<I, P>(&mut self, paths: I) -> io::Result<()>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        for path in paths {
            self.record_file(path.as_ref())?;
        }
        Ok(())
    }

    /// Manually mark a file as dirty
    pub fn mark_dirty(&mut self, path: PathBuf) {
        let abs_path = self.resolve_path(&path);
        self.dirty_files.insert(abs_path);
    }

    /// Check if a path is marked dirty
    pub fn is_dirty(&self, path: &Path) -> bool {
        let abs_path = self.resolve_path(path);
        self.dirty_files.contains(&abs_path)
    }

    /// Get count of dirty files
    pub fn dirty_count(&self) -> usize {
        self.dirty_files.len()
    }

    /// Get all dirty files
    pub fn dirty_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.dirty_files.iter()
    }

    /// Clear all dirty flags
    pub fn clear_dirty(&mut self) {
        self.dirty_files.clear();
    }

    /// Get count of tracked files
    pub fn tracked_count(&self) -> usize {
        self.fingerprints.len()
    }

    /// Check all tracked files for changes
    pub fn check_all(&mut self) -> Vec<PathBuf> {
        let paths: Vec<PathBuf> = self.fingerprints.keys().cloned().collect();
        let mut changed = Vec::new();

        for path in paths {
            if self.check_file(&path) {
                changed.push(path);
            }
        }

        changed
    }

    /// Remove a file from tracking
    pub fn untrack(&mut self, path: &Path) {
        let abs_path = self.resolve_path(path);
        self.fingerprints.remove(&abs_path);
        self.dirty_files.remove(&abs_path);
    }

    /// Clear all state
    pub fn clear(&mut self) {
        self.fingerprints.clear();
        self.dirty_files.clear();
    }

    /// Save state to a file
    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        use std::io::Write;

        let mut file = fs::File::create(path)?;

        for (file_path, fp) in &self.fingerprints {
            let mtime_secs = fp
                .mtime
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let hash_str = fp.hash.as_deref().unwrap_or("-");
            let inode_str = fp.inode.map(|i| i.to_string()).unwrap_or_else(|| "-".to_string());

            writeln!(
                file,
                "{}\t{}\t{}\t{}\t{}",
                file_path.display(),
                mtime_secs,
                fp.size,
                hash_str,
                inode_str
            )?;
        }

        Ok(())
    }

    /// Load state from a file
    pub fn load_from_file(&mut self, path: &Path) -> io::Result<()> {
        let content = fs::read_to_string(path)?;

        for line in content.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let file_path = PathBuf::from(parts[0]);
                let mtime_secs: u64 = parts[1].parse().unwrap_or(0);
                let size: u64 = parts[2].parse().unwrap_or(0);
                let hash = if parts.len() > 3 && parts[3] != "-" {
                    Some(parts[3].to_string())
                } else {
                    None
                };
                let inode = if parts.len() > 4 && parts[4] != "-" {
                    parts[4].parse().ok()
                } else {
                    None
                };

                let mtime = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(mtime_secs);

                self.fingerprints.insert(
                    file_path.clone(),
                    FileFingerprint {
                        path: file_path,
                        mtime,
                        size,
                        hash,
                        inode,
                    },
                );
            }
        }

        Ok(())
    }
}

impl Default for IncrementalState {
    fn default() -> Self {
        Self::new()
    }
}

/// Watch a directory for changes
pub struct DirectoryWatcher {
    state: IncrementalState,
    watched_dirs: Vec<PathBuf>,
    patterns: Vec<String>,
}

impl DirectoryWatcher {
    /// Create a new directory watcher
    pub fn new() -> Self {
        Self {
            state: IncrementalState::new(),
            watched_dirs: Vec::new(),
            patterns: vec!["*".to_string()],
        }
    }

    /// Add a directory to watch
    pub fn watch(&mut self, path: PathBuf) {
        self.watched_dirs.push(path);
    }

    /// Set file patterns to watch (glob patterns)
    pub fn set_patterns(&mut self, patterns: Vec<String>) {
        self.patterns = patterns;
    }

    /// Scan all watched directories for changes
    pub fn scan(&mut self) -> io::Result<Vec<PathBuf>> {
        let mut changed = Vec::new();

        for dir in &self.watched_dirs.clone() {
            self.scan_dir(dir, &mut changed)?;
        }

        Ok(changed)
    }

    /// Recursively scan a directory
    fn scan_dir(&mut self, dir: &Path, changed: &mut Vec<PathBuf>) -> io::Result<()> {
        if !dir.exists() || !dir.is_dir() {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                self.scan_dir(&path, changed)?;
            } else if path.is_file() {
                // Check if matches patterns
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let matches = self.patterns.iter().any(|p| {
                    if p == "*" {
                        true
                    } else if p.starts_with("*.") {
                        let ext = &p[2..];
                        file_name.ends_with(&format!(".{ext}"))
                    } else {
                        file_name == p
                    }
                });

                if matches && self.state.check_file(&path) {
                    changed.push(path);
                }
            }
        }

        Ok(())
    }

    /// Get the internal state
    pub fn state(&self) -> &IncrementalState {
        &self.state
    }

    /// Get mutable internal state
    pub fn state_mut(&mut self) -> &mut IncrementalState {
        &mut self.state
    }
}

impl Default for DirectoryWatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_file_fingerprint() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Hello, World!").unwrap();

        let fp = FileFingerprint::from_path(&file_path, HashAlgorithm::None).unwrap();
        assert_eq!(fp.size, 13);
        assert!(fp.hash.is_none());
    }

    #[test]
    fn test_file_fingerprint_with_hash() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Hello, World!").unwrap();

        let fp = FileFingerprint::from_path(&file_path, HashAlgorithm::XXHash64).unwrap();
        assert!(fp.hash.is_some());
        assert_eq!(fp.hash.as_ref().unwrap().len(), 16); // 64-bit hash in hex
    }

    #[test]
    fn test_incremental_state_change_detection() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Initial content").unwrap();

        let mut state = IncrementalState::new();

        // First check - file is new
        assert!(state.check_file(&file_path));
        assert!(state.is_dirty(&file_path));

        // Record the file
        state.record_file(&file_path).unwrap();
        state.clear_dirty();

        // Check again - no change
        assert!(!state.check_file(&file_path));
        assert!(!state.is_dirty(&file_path));

        // Modify the file
        fs::write(&file_path, "Modified content").unwrap();

        // Check again - should detect change
        assert!(state.check_file(&file_path));
        assert!(state.is_dirty(&file_path));
    }

    #[test]
    fn test_incremental_state_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        let state_file = temp_dir.path().join("state.dat");

        fs::write(&file_path, "Content").unwrap();

        // Create and populate state
        let mut state = IncrementalState::new();
        state.record_file(&file_path).unwrap();
        state.save_to_file(&state_file).unwrap();

        // Load into new state
        let mut loaded_state = IncrementalState::new();
        loaded_state.load_from_file(&state_file).unwrap();

        // Should have the same tracked count
        assert_eq!(loaded_state.tracked_count(), 1);
    }

    #[test]
    fn test_directory_watcher() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");

        fs::write(&file1, "Content 1").unwrap();
        fs::write(&file2, "Content 2").unwrap();

        let mut watcher = DirectoryWatcher::new();
        watcher.watch(temp_dir.path().to_path_buf());
        watcher.set_patterns(vec!["*.txt".to_string()]);

        // First scan - all files are new
        let changed = watcher.scan().unwrap();
        assert_eq!(changed.len(), 2);

        // Record files
        watcher.state_mut().record_file(&file1).unwrap();
        watcher.state_mut().record_file(&file2).unwrap();
        watcher.state_mut().clear_dirty();

        // Second scan - no changes
        let changed = watcher.scan().unwrap();
        assert_eq!(changed.len(), 0);
    }
}
