//! Incremental build support with file change detection

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// File fingerprint for change detection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub path: PathBuf,
    pub mtime: SystemTime,
    pub size: u64,
    pub hash: Option<String>,
}

/// Incremental build state
pub struct IncrementalState {
    fingerprints: HashMap<PathBuf, FileFingerprint>,
    dirty_files: HashSet<PathBuf>,
}

impl IncrementalState {
    pub fn new() -> Self {
        Self {
            fingerprints: HashMap::new(),
            dirty_files: HashSet::new(),
        }
    }

    pub fn check_file(&mut self, path: &Path) -> bool {
        // TODO: Implement file change detection
        false
    }

    pub fn mark_dirty(&mut self, path: PathBuf) {
        self.dirty_files.insert(path);
    }

    pub fn is_dirty(&self, path: &Path) -> bool {
        self.dirty_files.contains(path)
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty_files.len()
    }
}

impl Default for IncrementalState {
    fn default() -> Self {
        Self::new()
    }
}
