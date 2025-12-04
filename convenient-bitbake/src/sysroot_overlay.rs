//! OverlayFS-based sysroot assembly for hermetic builds
//!
//! This module provides sysroot assembly using Linux OverlayFS, which is superior
//! to hardlinks for hermetic builds:
//!
//! ## Why OverlayFS?
//!
//! - **Union semantics**: Automatically merges multiple sysroots
//! - **Read-only layers**: Dependencies can't be modified
//! - **Writable top layer**: Recipe-specific additions isolated
//! - **Cross-filesystem**: Works across different mount points
//! - **Hermetic**: Clear separation between inputs and outputs
//! - **Efficient**: No data copying, just metadata
//!
//! ## How it works
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │     Merged View (mountpoint)        │  ← What the build sees
//! └─────────────────────────────────────┘
//!           ↑
//!           │ OverlayFS mount
//!           │
//! ┌─────────┴───────────────────────────┐
//! │ Lower: dep1 sysroot (ro)            │  ← glibc headers/libs
//! │ Lower: dep2 sysroot (ro)            │  ← zlib headers/libs
//! │ Lower: dep3 sysroot (ro)            │  ← libssl headers/libs
//! │ Upper: recipe sysroot (rw)          │  ← Recipe-specific files
//! │ Work:  overlay work dir             │  ← OverlayFS internal
//! └─────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```no_run
//! use convenient_bitbake::sysroot_overlay::OverlaySysrootManager;
//! use std::path::PathBuf;
//!
//! let manager = OverlaySysrootManager::new(PathBuf::from("build/sysroots"));
//!
//! // Create overlay mount from dependencies
//! let lower_dirs = vec![
//!     PathBuf::from("cache/glibc/sysroot"),
//!     PathBuf::from("cache/zlib/sysroot"),
//! ];
//!
//! let mount_point = manager.create_overlay(
//!     "my-recipe",
//!     &lower_dirs,
//! ).unwrap();
//!
//! // Use the merged sysroot
//! // ... build happens here ...
//!
//! // Cleanup
//! manager.unmount(&mount_point).unwrap();
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during overlay sysroot operations
#[derive(Debug, Error)]
pub enum OverlaySysrootError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Mount failed: {0}")]
    MountFailed(String),

    #[error("Unmount failed: {0}")]
    UnmountFailed(String),

    #[error("OverlayFS not supported: {0}")]
    NotSupported(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

pub type OverlayResult<T> = Result<T, OverlaySysrootError>;

/// Manager for OverlayFS-based sysroot assembly
pub struct OverlaySysrootManager {
    /// Root directory for overlay mounts
    root_dir: PathBuf,

    /// Active mounts (recipe_name -> mount_point)
    active_mounts: HashMap<String, PathBuf>,
}

impl OverlaySysrootManager {
    /// Create a new overlay sysroot manager
    pub fn new(root_dir: PathBuf) -> Self {
        Self {
            root_dir,
            active_mounts: HashMap::new(),
        }
    }

    /// Check if OverlayFS is supported on this system
    pub fn is_supported() -> bool {
        // Check if overlay filesystem is available
        if let Ok(output) = Command::new("mount")
            .arg("-t")
            .arg("overlay")
            .arg("-o")
            .arg("lowerdir=/tmp,upperdir=/tmp,workdir=/tmp")
            .arg("none")
            .arg("/tmp/test-overlay")
            .output()
        {
            // If mount command exists and doesn't fail with "unknown filesystem type",
            // OverlayFS is likely supported
            !String::from_utf8_lossy(&output.stderr).contains("unknown filesystem type")
        } else {
            false
        }
    }

    /// Create an overlay mount for a recipe's sysroot
    ///
    /// # Arguments
    ///
    /// * `recipe_name` - Name of the recipe
    /// * `lower_dirs` - List of dependency sysroot directories (read-only layers)
    ///
    /// # Returns
    ///
    /// Path to the mounted overlay filesystem (the merged view)
    pub fn create_overlay(
        &mut self,
        recipe_name: &str,
        lower_dirs: &[PathBuf],
    ) -> OverlayResult<PathBuf> {
        // Create directory structure for this overlay
        let recipe_dir = self.root_dir.join(recipe_name);
        let upper_dir = recipe_dir.join("upper");  // Writable layer
        let work_dir = recipe_dir.join("work");    // OverlayFS work dir
        let merged_dir = recipe_dir.join("merged"); // Mount point

        fs::create_dir_all(&upper_dir)?;
        fs::create_dir_all(&work_dir)?;
        fs::create_dir_all(&merged_dir)?;

        debug!("Creating overlay mount for {}", recipe_name);
        debug!("  Upper: {}", upper_dir.display());
        debug!("  Work:  {}", work_dir.display());
        debug!("  Merged: {}", merged_dir.display());
        debug!("  Lower layers: {}", lower_dirs.len());

        // Build the overlay mount options
        // Format: lowerdir=dir1:dir2:dir3,upperdir=upper,workdir=work
        let lower_str = lower_dirs
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(":");

        if lower_str.is_empty() {
            warn!("No lower directories for {}, creating empty overlay", recipe_name);
        }

        let options = if lower_str.is_empty() {
            // No dependencies - just mount upper as overlay
            format!(
                "upperdir={},workdir={}",
                upper_dir.to_string_lossy(),
                work_dir.to_string_lossy()
            )
        } else {
            format!(
                "lowerdir={},upperdir={},workdir={}",
                lower_str,
                upper_dir.to_string_lossy(),
                work_dir.to_string_lossy()
            )
        };

        // Mount the overlay
        let output = Command::new("mount")
            .arg("-t")
            .arg("overlay")
            .arg("-o")
            .arg(&options)
            .arg("overlay")
            .arg(&merged_dir)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(OverlaySysrootError::MountFailed(format!(
                "Failed to mount overlay for {}: {}",
                recipe_name, stderr
            )));
        }

        info!("✓ Created overlay mount for {} at {}",
              recipe_name, merged_dir.display());

        // Track this mount
        self.active_mounts.insert(recipe_name.to_string(), merged_dir.clone());

        Ok(merged_dir)
    }

    /// Unmount an overlay filesystem
    pub fn unmount(&mut self, mount_point: &Path) -> OverlayResult<()> {
        debug!("Unmounting overlay at {}", mount_point.display());

        let output = Command::new("umount")
            .arg(mount_point)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If already unmounted, that's OK
            if !stderr.contains("not mounted") {
                return Err(OverlaySysrootError::UnmountFailed(format!(
                    "Failed to unmount {}: {}",
                    mount_point.display(),
                    stderr
                )));
            }
        }

        // Remove from tracking
        self.active_mounts.retain(|_, mp| mp != mount_point);

        info!("✓ Unmounted overlay at {}", mount_point.display());

        Ok(())
    }

    /// Unmount all active overlays
    pub fn unmount_all(&mut self) -> OverlayResult<()> {
        let mount_points: Vec<PathBuf> = self.active_mounts.values().cloned().collect();

        for mount_point in mount_points {
            self.unmount(&mount_point)?;
        }

        Ok(())
    }

    /// Get the mount point for a recipe (if mounted)
    pub fn get_mount_point(&self, recipe_name: &str) -> Option<&PathBuf> {
        self.active_mounts.get(recipe_name)
    }

    /// Check if a recipe has an active overlay mount
    pub fn is_mounted(&self, recipe_name: &str) -> bool {
        self.active_mounts.contains_key(recipe_name)
    }
}

impl Drop for OverlaySysrootManager {
    fn drop(&mut self) {
        // Attempt to clean up all mounts on drop
        if let Err(e) = self.unmount_all() {
            warn!("Failed to unmount all overlays during cleanup: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    #[ignore] // Requires root or user namespaces to create overlay mounts
    fn test_overlay_creation() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sysroots");
        fs::create_dir_all(&root).unwrap();

        // Create some lower directories
        let dep1 = tmp.path().join("dep1");
        let dep2 = tmp.path().join("dep2");
        fs::create_dir_all(dep1.join("usr/lib")).unwrap();
        fs::create_dir_all(dep2.join("usr/include")).unwrap();
        fs::write(dep1.join("usr/lib/libc.so"), b"fake lib").unwrap();
        fs::write(dep2.join("usr/include/stdio.h"), b"fake header").unwrap();

        let mut manager = OverlaySysrootManager::new(root);

        let merged = manager
            .create_overlay("test-recipe", &[dep1, dep2])
            .unwrap();

        // Check that files from both layers are visible
        assert!(merged.join("usr/lib/libc.so").exists());
        assert!(merged.join("usr/include/stdio.h").exists());

        // Cleanup
        manager.unmount(&merged).unwrap();
    }

    #[test]
    fn test_overlay_support_check() {
        // This test just checks that the function doesn't panic
        let _supported = OverlaySysrootManager::is_supported();
    }
}
