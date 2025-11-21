//! Repository management for kas-based builds
//!
//! Handles git repository cloning, updating, and patch application
//! based on kas configuration.

use crate::include_graph::{KasConfig, KasRepo};
use convenient_git::async_git::{AsyncGitRepository, GitError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Repository manager
pub struct RepositoryManager {
    repos_dir: PathBuf,
    cache_dir: Option<PathBuf>,
}

impl RepositoryManager {
    /// Create a new repository manager
    pub fn new(repos_dir: impl AsRef<Path>) -> Self {
        Self {
            repos_dir: repos_dir.as_ref().to_path_buf(),
            cache_dir: None,
        }
    }

    /// Set cache directory for git objects
    pub fn with_cache(mut self, cache_dir: impl AsRef<Path>) -> Self {
        self.cache_dir = Some(cache_dir.as_ref().to_path_buf());
        self
    }

    /// Setup all repositories from kas config
    pub async fn setup_repositories(
        &self,
        config: &KasConfig,
    ) -> Result<HashMap<String, PathBuf>, RepoError> {
        let mut repo_paths = HashMap::new();

        tokio::fs::create_dir_all(&self.repos_dir)
            .await
            .map_err(|e| RepoError::IoError(self.repos_dir.clone(), e.to_string()))?;

        for (name, repo_config) in &config.repos {
            let repo_path = self.setup_repository(name, repo_config).await?;
            repo_paths.insert(name.clone(), repo_path);
        }

        Ok(repo_paths)
    }

    /// Setup a single repository
    pub async fn setup_repository(
        &self,
        name: &str,
        config: &KasRepo,
    ) -> Result<PathBuf, RepoError> {
        // If path is specified, use it directly (local repo)
        if let Some(path) = &config.path {
            let repo_path = PathBuf::from(path);
            if tokio::fs::try_exists(&repo_path).await.unwrap_or(false) {
                info!("Using local repository: {} at {}", name, repo_path.display());
                return Ok(repo_path);
            }
            return Err(RepoError::LocalRepoNotFound(repo_path));
        }

        // Otherwise clone from URL
        let url = config
            .url
            .as_ref()
            .ok_or_else(|| RepoError::MissingUrl(name.to_string()))?;

        let repo_path = self.repos_dir.join(name);

        // Create AsyncGitRepository instance
        let git_repo = AsyncGitRepository::new(&repo_path, url, None);

        // Clone or open repository
        info!("Cloning/opening repository {} from {}", name, url);
        git_repo.clone_or_open().await.map_err(RepoError::from)?;

        // Checkout specific refspec if specified
        if let Some(refspec) = self.get_refspec(config) {
            info!("Checking out refspec: {}", refspec);
            git_repo.checkout(&refspec).await.map_err(RepoError::from)?;
        }

        Ok(repo_path)
    }

    /// Get refspec from config (commit, tag, branch, or refspec)
    fn get_refspec(&self, config: &KasRepo) -> Option<String> {
        config
            .commit
            .clone()
            .or_else(|| config.tag.clone())
            .or_else(|| config.branch.clone())
            .or_else(|| config.refspec.clone())
    }

    /// Apply patches to a repository
    pub async fn apply_patches(
        &self,
        repo_path: &Path,
        patches: &HashMap<String, Vec<String>>,
    ) -> Result<(), RepoError> {
        for (patch_id, patch_files) in patches {
            info!("Applying patch set: {}", patch_id);

            for patch_file in patch_files {
                self.apply_single_patch(repo_path, patch_file).await?;
            }
        }

        Ok(())
    }

    /// Apply a single patch file
    async fn apply_single_patch(&self, repo_path: &Path, patch_file: &str) -> Result<(), RepoError> {
        info!("Applying patch: {}", patch_file);

        let repo_path = repo_path.to_path_buf();
        let patch_file = patch_file.to_string();

        // Use git apply command (run in blocking task)
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .arg("apply")
                .arg(&patch_file)
                .current_dir(&repo_path)
                .output()
        })
        .await
        .map_err(|e| RepoError::PatchError(format!("Task join error: {e}")))?
        .map_err(|e| RepoError::PatchError(format!("Failed to run git apply: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RepoError::PatchError(format!("Patch failed: {stderr}")));
        }

        Ok(())
    }

    /// Get layer paths from a repository
    pub fn get_layer_paths(
        &self,
        repo_path: &Path,
        repo_config: &KasRepo,
    ) -> Result<Vec<PathBuf>, RepoError> {
        let mut layer_paths = Vec::new();

        if repo_config.layers.is_empty() {
            // No explicit layers means the repo itself is a layer
            layer_paths.push(repo_path.to_path_buf());
        } else {
            for (layer_name, layer_config) in &repo_config.layers {
                let layer_path = if let Some(path) = &layer_config.path {
                    repo_path.join(path)
                } else {
                    repo_path.join(layer_name)
                };

                if !layer_path.exists() {
                    warn!(
                        "Layer {} not found at {}",
                        layer_name,
                        layer_path.display()
                    );
                    continue;
                }

                layer_paths.push(layer_path);
            }
        }

        Ok(layer_paths)
    }
}

/// Repository error types
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    /// File system I/O error
    #[error("IO error at {0}: {1}")]
    IoError(PathBuf, String),

    /// Git operation error
    #[error("Git error: {0}")]
    Git(#[from] GitError),

    /// Repository URL not specified in configuration
    #[error("Missing URL for repository: {0}")]
    MissingUrl(String),

    /// Local repository path does not exist
    #[error("Local repository not found: {0}")]
    LocalRepoNotFound(PathBuf),

    /// Failed to apply patch to repository
    #[error("Patch application failed: {0}")]
    PatchError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_repository_manager_creation() {
        let temp = TempDir::new().unwrap();
        let _manager = RepositoryManager::new(temp.path());
        assert!(temp.path().exists());
    }
}
