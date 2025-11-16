//! Hardlink-based sysroot assembly for BitBake recipe-specific sysroots
//!
//! This module implements BitBake's copyhardlinktree() approach for assembling
//! recipe-specific sysroots from multiple dependency outputs.
//!
//! ## Why Hardlinks?
//!
//! BitBake needs to combine outputs from multiple dependencies into a single
//! sysroot directory. Symlinks don't work because you can't have multiple symlinks
//! for the same directory (e.g., /usr/lib from glibc AND libz).
//!
//! Hardlinks solve this:
//! - Multiple directory entries point to the same inode (no disk duplication)
//! - Fast (no copying)
//! - Atomic (filesystem operation)
//! - Works when artifacts and sandbox are on the same filesystem

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during sysroot assembly
#[derive(Debug, Error)]
pub enum SysrootError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("File conflict: {path} provided by both {existing} and {new}")]
    FileConflict {
        path: PathBuf,
        existing: String,
        new: String,
    },

    #[error("Manifest parse error: {0}")]
    ManifestParse(#[from] serde_json::Error),

    #[error("Command execution failed: {0}")]
    CommandFailed(String),

    #[error("WalkDir error: {0}")]
    WalkDir(#[from] walkdir::Error),
}

pub type SysrootResult<T> = Result<T, SysrootError>;

/// Manifest tracking which files a task provides to the sysroot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysrootManifest {
    /// Recipe name (e.g., "glibc")
    pub recipe: String,

    /// Task name (e.g., "do_install")
    pub task: String,

    /// Task signature hash
    pub signature: String,

    /// List of files provided (relative paths within sysroot)
    /// Example: ["usr/lib/libc.so.6", "usr/include/stdio.h"]
    pub files: Vec<PathBuf>,
}

impl SysrootManifest {
    /// Create a new manifest
    pub fn new(recipe: String, task: String, signature: String) -> Self {
        Self {
            recipe,
            task,
            signature,
            files: Vec::new(),
        }
    }

    /// Add a file to the manifest
    pub fn add_file(&mut self, path: PathBuf) {
        self.files.push(path);
    }

    /// Load manifest from JSON file
    pub fn load(path: &Path) -> SysrootResult<Self> {
        let content = fs::read_to_string(path)?;
        let manifest = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    /// Save manifest to JSON file
    pub fn save(&self, path: &Path) -> SysrootResult<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }
}

/// Builder for hardlinking directory trees
pub struct HardlinkTreeBuilder {
    /// Whitelist for harmless file duplicates
    dup_whitelist: Vec<PathBuf>,
}

impl HardlinkTreeBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            dup_whitelist: vec![
                PathBuf::from("usr/share/licenses"),
                PathBuf::from("usr/share/doc"),
                PathBuf::from("etc/sgml"),
            ],
        }
    }

    /// Check if source and destination are on same filesystem
    #[cfg(unix)]
    fn same_filesystem(src: &Path, dst: &Path) -> SysrootResult<bool> {
        use std::os::unix::fs::MetadataExt;

        let src_dev = fs::metadata(src)?.dev();
        let dst_dev = fs::metadata(dst)?.dev();

        Ok(src_dev == dst_dev)
    }

    /// Hardlink tree from src to dst (like BitBake's copyhardlinktree)
    ///
    /// This uses `cp -afl` to create hardlinks instead of copying file data.
    pub fn copyhardlinktree(&self, src: &Path, dst: &Path) -> SysrootResult<()> {
        // Create destination directory
        fs::create_dir_all(dst)?;

        #[cfg(unix)]
        {
            if Self::same_filesystem(src, dst)? {
                // Same filesystem: use hardlinks (fast, no disk usage)
                self.copyhardlinktree_hardlink(src, dst)?;
            } else {
                // Different filesystem: fall back to copy
                self.copyhardlinktree_copy(src, dst)?;
            }
        }

        #[cfg(not(unix))]
        {
            // Non-Unix: always copy
            self.copyhardlinktree_copy(src, dst)?;
        }

        Ok(())
    }

    /// Use cp -afl to create hardlinks (Unix only)
    #[cfg(unix)]
    fn copyhardlinktree_hardlink(&self, src: &Path, dst: &Path) -> SysrootResult<()> {
        // cp -afl:
        // -a: archive mode (preserve permissions, timestamps, ownership)
        // -f: force
        // -l: create hardlinks instead of copying

        let output = Command::new("cp")
            .arg("-afl")
            .arg("--preserve=xattr")
            .arg(format!("{}/*", src.display()))
            .arg(dst)
            .output()?;

        if !output.status.success() {
            return Err(SysrootError::CommandFailed(format!(
                "cp -afl failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Fall back to recursive copy
    fn copyhardlinktree_copy(&self, src: &Path, dst: &Path) -> SysrootResult<()> {
        use walkdir::WalkDir;

        for entry in WalkDir::new(src) {
            let entry = entry?;
            let rel_path = entry.path().strip_prefix(src).unwrap();
            let dst_path = dst.join(rel_path);

            if entry.file_type().is_dir() {
                fs::create_dir_all(&dst_path)?;
            } else if entry.file_type().is_file() {
                if let Some(parent) = dst_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(entry.path(), &dst_path)?;
            }
        }

        Ok(())
    }
}

impl Default for HardlinkTreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Task dependency specification for sysroot assembly
#[derive(Debug, Clone)]
pub struct TaskDependency {
    pub recipe: String,
    pub task: String,
    pub signature: String,
}

/// Assembler for recipe-specific sysroots with conflict detection
pub struct SysrootAssembler {
    hardlink_builder: HardlinkTreeBuilder,
    dup_whitelist: Vec<PathBuf>,
}

impl SysrootAssembler {
    /// Create a new sysroot assembler
    pub fn new() -> Self {
        Self {
            hardlink_builder: HardlinkTreeBuilder::new(),
            dup_whitelist: vec![
                PathBuf::from("usr/share/licenses"),
                PathBuf::from("usr/share/doc"),
                PathBuf::from("etc/sgml"),
            ],
        }
    }

    /// Assemble recipe-sysroot from multiple dependency sysroots with conflict detection
    ///
    /// This function:
    /// 1. Checks for file conflicts between dependencies
    /// 2. Hardlinks all dependency outputs into the recipe sysroot
    /// 3. Tracks which files came from which dependency
    ///
    /// # Arguments
    ///
    /// * `dependencies` - List of task dependencies to include
    /// * `artifact_cache` - Root directory of the artifact cache
    /// * `output_sysroot` - Destination sysroot directory
    ///
    /// # Errors
    ///
    /// Returns `SysrootError::FileConflict` if the same file is provided by
    /// multiple dependencies (unless whitelisted).
    pub fn assemble_sysroot(
        &self,
        dependencies: &[TaskDependency],
        artifact_cache: &Path,
        output_sysroot: &Path,
    ) -> SysrootResult<()> {
        // Track which files have been staged and by whom
        let mut staged_files: HashMap<PathBuf, String> = HashMap::new();

        // Create output sysroot directory
        fs::create_dir_all(output_sysroot)?;

        for dep in dependencies {
            // Construct path to dependency's artifact
            let dep_artifact = artifact_cache.join(format!(
                "{}/{}-{}",
                dep.recipe, dep.task, dep.signature
            ));

            let dep_sysroot = dep_artifact.join("sysroot");
            let dep_manifest_path = dep_artifact.join("manifest.json");

            // Skip if dependency sysroot doesn't exist
            if !dep_sysroot.exists() {
                eprintln!(
                    "Warning: Sysroot not found for {}:{}",
                    dep.recipe, dep.task
                );
                continue;
            }

            // Load manifest if it exists, otherwise generate from files
            let manifest = if dep_manifest_path.exists() {
                SysrootManifest::load(&dep_manifest_path)?
            } else {
                // Generate manifest by walking the sysroot
                generate_sysroot_manifest(&dep_sysroot, &dep.recipe, &dep.task, &dep.signature)?
            };

            // Check for conflicts
            for file in &manifest.files {
                // Skip whitelisted paths
                if self.is_whitelisted(file) {
                    continue;
                }

                // Check if already staged
                if let Some(existing_from) = staged_files.get(file) {
                    return Err(SysrootError::FileConflict {
                        path: file.clone(),
                        existing: existing_from.clone(),
                        new: format!("{}:{}-{}", dep.recipe, dep.task, dep.signature),
                    });
                }
            }

            // No conflicts - hardlink files into sysroot
            eprintln!(
                "Staging sysroot from {}:{} ({} files)",
                dep.recipe,
                dep.task,
                manifest.files.len()
            );

            self.hardlink_builder
                .copyhardlinktree(&dep_sysroot, output_sysroot)?;

            // Track staged files
            for file in &manifest.files {
                staged_files.insert(
                    file.clone(),
                    format!("{}:{}-{}", dep.recipe, dep.task, dep.signature),
                );
            }
        }

        eprintln!(
            "✓ Assembled sysroot with {} files from {} dependencies",
            staged_files.len(),
            dependencies.len()
        );

        Ok(())
    }

    /// Check if a file path is whitelisted for duplicates
    fn is_whitelisted(&self, file: &Path) -> bool {
        self.dup_whitelist.iter().any(|w| file.starts_with(w))
    }
}

impl Default for SysrootAssembler {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a sysroot manifest by scanning a directory
pub fn generate_sysroot_manifest(
    sysroot_dir: &Path,
    recipe: &str,
    task: &str,
    signature: &str,
) -> SysrootResult<SysrootManifest> {
    use walkdir::WalkDir;

    let mut manifest = SysrootManifest::new(
        recipe.to_string(),
        task.to_string(),
        signature.to_string(),
    );

    for entry in WalkDir::new(sysroot_dir).follow_links(false) {
        let entry = entry?;

        if entry.file_type().is_file() {
            // Store relative path
            let rel_path = entry.path().strip_prefix(sysroot_dir).unwrap();
            manifest.add_file(rel_path.to_path_buf());
        }
    }

    // Sort for determinism
    manifest.files.sort();

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_manifest_serialization() {
        let mut manifest = SysrootManifest::new(
            "glibc".to_string(),
            "do_install".to_string(),
            "abc123".to_string(),
        );

        manifest.add_file(PathBuf::from("usr/lib/libc.so.6"));
        manifest.add_file(PathBuf::from("usr/include/stdio.h"));

        // Serialize
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("glibc"));
        assert!(json.contains("libc.so.6"));

        // Deserialize
        let deserialized: SysrootManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.recipe, "glibc");
        assert_eq!(deserialized.files.len(), 2);
    }

    #[test]
    fn test_hardlink_tree_builder() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        // Create source files
        fs::create_dir_all(src.join("usr/lib")).unwrap();
        fs::create_dir_all(src.join("usr/include")).unwrap();
        fs::write(src.join("usr/lib/test.so"), b"library").unwrap();
        fs::write(src.join("usr/include/test.h"), b"header").unwrap();

        // Hardlink tree
        let builder = HardlinkTreeBuilder::new();
        builder.copyhardlinktree(&src, &dst).unwrap();

        // Verify files exist
        assert!(dst.join("usr/lib/test.so").exists());
        assert!(dst.join("usr/include/test.h").exists());

        // Verify content
        let content = fs::read_to_string(dst.join("usr/lib/test.so")).unwrap();
        assert_eq!(content, "library");
    }

    #[test]
    fn test_sysroot_assembler_conflict_detection() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache");
        let sysroot = tmp.path().join("sysroot");

        // Create two dependencies with conflicting files
        let dep1 = cache.join("glibc/do_install-sig1");
        let dep1_sysroot = dep1.join("sysroot");
        fs::create_dir_all(dep1_sysroot.join("usr/lib")).unwrap();
        fs::write(dep1_sysroot.join("usr/lib/libc.so"), b"glibc-2.38").unwrap();

        let dep2 = cache.join("glibc-old/do_install-sig2");
        let dep2_sysroot = dep2.join("sysroot");
        fs::create_dir_all(dep2_sysroot.join("usr/lib")).unwrap();
        fs::write(dep2_sysroot.join("usr/lib/libc.so"), b"glibc-2.37").unwrap();

        // Generate manifests
        let manifest1 = generate_sysroot_manifest(&dep1_sysroot, "glibc", "do_install", "sig1").unwrap();
        manifest1.save(&dep1.join("manifest.json")).unwrap();

        let manifest2 = generate_sysroot_manifest(&dep2_sysroot, "glibc-old", "do_install", "sig2").unwrap();
        manifest2.save(&dep2.join("manifest.json")).unwrap();

        // Try to assemble - should detect conflict
        let assembler = SysrootAssembler::new();
        let dependencies = vec![
            TaskDependency {
                recipe: "glibc".to_string(),
                task: "do_install".to_string(),
                signature: "sig1".to_string(),
            },
            TaskDependency {
                recipe: "glibc-old".to_string(),
                task: "do_install".to_string(),
                signature: "sig2".to_string(),
            },
        ];

        let result = assembler.assemble_sysroot(&dependencies, &cache, &sysroot);

        // Should fail with conflict error
        assert!(result.is_err());
        match result {
            Err(SysrootError::FileConflict { path, .. }) => {
                assert!(path.to_string_lossy().contains("libc.so"));
            }
            _ => panic!("Expected FileConflict error"),
        }
    }

    #[test]
    fn test_sysroot_assembler_success() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache");
        let sysroot = tmp.path().join("sysroot");

        // Create two dependencies with different files (no conflict)
        let dep1 = cache.join("glibc/do_install-sig1");
        let dep1_sysroot = dep1.join("sysroot");
        fs::create_dir_all(dep1_sysroot.join("usr/lib")).unwrap();
        fs::write(dep1_sysroot.join("usr/lib/libc.so"), b"glibc").unwrap();

        let dep2 = cache.join("libz/do_install-sig2");
        let dep2_sysroot = dep2.join("sysroot");
        fs::create_dir_all(dep2_sysroot.join("usr/lib")).unwrap();
        fs::write(dep2_sysroot.join("usr/lib/libz.so"), b"zlib").unwrap();

        // Generate manifests
        let manifest1 = generate_sysroot_manifest(&dep1_sysroot, "glibc", "do_install", "sig1").unwrap();
        manifest1.save(&dep1.join("manifest.json")).unwrap();

        let manifest2 = generate_sysroot_manifest(&dep2_sysroot, "libz", "do_install", "sig2").unwrap();
        manifest2.save(&dep2.join("manifest.json")).unwrap();

        // Assemble sysroot
        let assembler = SysrootAssembler::new();
        let dependencies = vec![
            TaskDependency {
                recipe: "glibc".to_string(),
                task: "do_install".to_string(),
                signature: "sig1".to_string(),
            },
            TaskDependency {
                recipe: "libz".to_string(),
                task: "do_install".to_string(),
                signature: "sig2".to_string(),
            },
        ];

        let result = assembler.assemble_sysroot(&dependencies, &cache, &sysroot);

        // Should succeed
        assert!(result.is_ok());

        // Verify both files exist in sysroot
        assert!(sysroot.join("usr/lib/libc.so").exists());
        assert!(sysroot.join("usr/lib/libz.so").exists());

        // Verify content
        let libc_content = fs::read_to_string(sysroot.join("usr/lib/libc.so")).unwrap();
        assert_eq!(libc_content, "glibc");

        let libz_content = fs::read_to_string(sysroot.join("usr/lib/libz.so")).unwrap();
        assert_eq!(libz_content, "zlib");
    }
}
