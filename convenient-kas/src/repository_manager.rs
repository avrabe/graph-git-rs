//! Repository management for kas-based builds
//!
//! Handles git repository cloning, updating, and patch application
//! based on kas configuration.

use crate::include_graph::{KasConfig, KasRepo};
use git2::{build::CheckoutBuilder, FetchOptions, Repository};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
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
    pub fn setup_repositories(
        &self,
        config: &KasConfig,
    ) -> Result<HashMap<String, PathBuf>, RepoError> {
        let mut repo_paths = HashMap::new();

        std::fs::create_dir_all(&self.repos_dir)
            .map_err(|e| RepoError::IoError(self.repos_dir.clone(), e.to_string()))?;

        for (name, repo_config) in &config.repos {
            let repo_path = self.setup_repository(name, repo_config)?;
            repo_paths.insert(name.clone(), repo_path);
        }

        Ok(repo_paths)
    }

    /// Setup a single repository
    pub fn setup_repository(
        &self,
        name: &str,
        config: &KasRepo,
    ) -> Result<PathBuf, RepoError> {
        // If path is specified, use it directly (local repo)
        if let Some(path) = &config.path {
            let repo_path = PathBuf::from(path);
            if repo_path.exists() {
                info!("Using local repository: {} at {}", name, repo_path.display());
                return Ok(repo_path);
            } else {
                return Err(RepoError::LocalRepoNotFound(repo_path));
            }
        }

        // Otherwise clone from URL
        let url = config
            .url
            .as_ref()
            .ok_or_else(|| RepoError::MissingUrl(name.to_string()))?;

        let repo_path = self.repos_dir.join(name);

        if repo_path.exists() {
            info!("Repository {} already exists, updating...", name);
            self.update_repository(&repo_path, config)?;
        } else {
            info!("Cloning repository {} from {}", name, url);
            self.clone_repository(url, &repo_path, config)?;
        }

        Ok(repo_path)
    }

    /// Clone a repository
    fn clone_repository(
        &self,
        url: &str,
        dest: &Path,
        config: &KasRepo,
    ) -> Result<(), RepoError> {
        // Build fetch options
        let mut fetch_opts = FetchOptions::new();

        // For shallow clones (CI optimization)
        // fetch_opts.depth(1);

        // Clone the repository
        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch_opts);

        let repo = builder
            .clone(url, dest)
            .map_err(|e| RepoError::GitError(format!("Clone failed: {}", e)))?;

        // Checkout specific refspec/branch/commit/tag
        self.checkout_refspec(&repo, config)?;

        Ok(())
    }

    /// Update an existing repository
    fn update_repository(&self, repo_path: &Path, config: &KasRepo) -> Result<(), RepoError> {
        let repo = Repository::open(repo_path)
            .map_err(|e| RepoError::GitError(format!("Open failed: {}", e)))?;

        // Fetch latest changes
        let mut remote = repo
            .find_remote("origin")
            .map_err(|e| RepoError::GitError(format!("Find remote failed: {}", e)))?;

        let mut fetch_opts = FetchOptions::new();
        remote
            .fetch(&["refs/heads/*:refs/remotes/origin/*"], Some(&mut fetch_opts), None)
            .map_err(|e| RepoError::GitError(format!("Fetch failed: {}", e)))?;

        // Checkout requested refspec
        self.checkout_refspec(&repo, config)?;

        Ok(())
    }

    /// Checkout specific refspec/branch/commit/tag
    fn checkout_refspec(&self, repo: &Repository, config: &KasRepo) -> Result<(), RepoError> {
        let refspec = if let Some(commit) = &config.commit {
            commit.clone()
        } else if let Some(tag) = &config.tag {
            format!("refs/tags/{}", tag)
        } else if let Some(branch) = &config.branch {
            format!("refs/heads/{}", branch)
        } else if let Some(refspec) = &config.refspec {
            refspec.clone()
        } else {
            "HEAD".to_string()
        };

        info!("Checking out refspec: {}", refspec);

        // Resolve reference
        let (object, reference) = repo
            .revparse_ext(&refspec)
            .map_err(|e| RepoError::GitError(format!("Refspec '{}' not found: {}", refspec, e)))?;

        // Checkout
        let mut checkout_builder = CheckoutBuilder::new();
        checkout_builder.force();

        repo.checkout_tree(&object, Some(&mut checkout_builder))
            .map_err(|e| RepoError::GitError(format!("Checkout failed: {}", e)))?;

        // Set HEAD
        match reference {
            Some(gref) => repo.set_head(gref.name().unwrap()),
            None => repo.set_head_detached(object.id()),
        }
        .map_err(|e| RepoError::GitError(format!("Set HEAD failed: {}", e)))?;

        Ok(())
    }

    /// Apply patches to a repository
    pub fn apply_patches(
        &self,
        repo_path: &Path,
        patches: &HashMap<String, Vec<String>>,
    ) -> Result<(), RepoError> {
        let repo = Repository::open(repo_path)
            .map_err(|e| RepoError::GitError(format!("Open failed: {}", e)))?;

        for (patch_id, patch_files) in patches {
            info!("Applying patch set: {}", patch_id);

            for patch_file in patch_files {
                self.apply_single_patch(&repo, repo_path, patch_file)?;
            }
        }

        Ok(())
    }

    /// Apply a single patch file
    fn apply_single_patch(
        &self,
        _repo: &Repository,
        repo_path: &Path,
        patch_file: &str,
    ) -> Result<(), RepoError> {
        info!("Applying patch: {}", patch_file);

        // Use git apply command
        let output = std::process::Command::new("git")
            .arg("apply")
            .arg(patch_file)
            .current_dir(repo_path)
            .output()
            .map_err(|e| RepoError::PatchError(format!("Failed to run git apply: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RepoError::PatchError(format!(
                "Patch failed: {}",
                stderr
            )));
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
    #[error("IO error at {0}: {1}")]
    IoError(PathBuf, String),

    #[error("Git error: {0}")]
    GitError(String),

    #[error("Missing URL for repository: {0}")]
    MissingUrl(String),

    #[error("Local repository not found: {0}")]
    LocalRepoNotFound(PathBuf),

    #[error("Patch application failed: {0}")]
    PatchError(String),

    #[error("Invalid refspec: {0}")]
    InvalidRefspec(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_repository_manager_creation() {
        let temp = TempDir::new().unwrap();
        let manager = RepositoryManager::new(temp.path());
        assert!(temp.path().exists());
    }
}
