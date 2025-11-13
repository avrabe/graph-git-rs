//! Async Git operations for high-performance repository management
//!
//! Provides async wrappers around git2 operations with proper error handling,
//! progress tracking, and authentication support.

use git2::{
    build::CheckoutBuilder, Cred, FetchOptions, Oid, ProxyOptions, RemoteCallbacks,
    Repository,
};
use std::path::{Path, PathBuf};
use tokio::task;
use tracing::{debug, info};

/// Git repository errors
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git error: {0}")]
    Git(#[from] git2::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Repository not found at {0}")]
    NotFound(PathBuf),

    #[error("Invalid reference: {0}")]
    InvalidReference(String),

    #[error("Authentication failed")]
    AuthFailed,

    #[error("Clone failed: {0}")]
    CloneFailed(String),
}

pub type GitResult<T> = Result<T, GitError>;

/// Async Git repository wrapper
pub struct AsyncGitRepository {
    path: PathBuf,
    url: String,
    credentials: Option<GitCredentials>,
}

/// Git credentials
#[derive(Clone)]
pub struct GitCredentials {
    pub username: String,
    pub password: String,
}

impl AsyncGitRepository {
    /// Create a new async git repository instance
    ///
    /// # Arguments
    ///
    /// * `path` - Local path where repository should be cloned/opened
    /// * `url` - Git repository URL
    /// * `credentials` - Optional authentication credentials
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use convenient_git::async_git::{AsyncGitRepository, GitCredentials};
    /// use std::path::Path;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let creds = GitCredentials {
    ///     username: "user".to_string(),
    ///     password: "pass".to_string(),
    /// };
    ///
    /// let repo = AsyncGitRepository::new(
    ///     Path::new("/tmp/repo"),
    ///     "https://github.com/example/repo.git",
    ///     Some(creds)
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(
        path: impl AsRef<Path>,
        url: impl Into<String>,
        credentials: Option<GitCredentials>,
    ) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            url: url.into(),
            credentials,
        }
    }

    /// Clone or open repository asynchronously
    ///
    /// If the repository exists, it will be opened. Otherwise, it will be cloned
    /// from the configured URL.
    ///
    /// # Errors
    ///
    /// Returns `GitError` if clone or open operation fails
    pub async fn clone_or_open(&self) -> GitResult<Repository> {
        let path = self.path.clone();
        let url = self.url.clone();
        let credentials = self.credentials.clone();

        task::spawn_blocking(move || {
            // Try to open existing repository first
            if path.exists() {
                debug!("Opening existing repository at {}", path.display());
                return Repository::open(&path).map_err(GitError::from);
            }

            // Clone repository
            info!("Cloning repository from {} to {}", url, path.display());

            let mut callbacks = RemoteCallbacks::new();

            // Setup credentials if provided
            if let Some(creds) = credentials {
                callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                    Cred::userpass_plaintext(&creds.username, &creds.password)
                });
            }

            callbacks.transfer_progress(|stats| {
                if stats.received_objects() == stats.total_objects() {
                    debug!(
                        "Resolving deltas {}/{}",
                        stats.indexed_deltas(),
                        stats.total_deltas()
                    );
                } else if stats.received_objects() % 100 == 0 {
                    debug!(
                        "Received {}/{} objects ({} kb)",
                        stats.received_objects(),
                        stats.total_objects(),
                        stats.received_bytes() / 1024
                    );
                }
                true
            });

            let mut fetch_options = FetchOptions::new();
            fetch_options.remote_callbacks(callbacks);

            let mut proxy_opts = ProxyOptions::new();
            proxy_opts.auto();
            fetch_options.proxy_options(proxy_opts);
            fetch_options.download_tags(git2::AutotagOption::All);

            let mut checkout_builder = CheckoutBuilder::new();
            checkout_builder.progress(|_path, cur, total| {
                if cur % 100 == 0 || cur == total {
                    debug!("Checkout progress: {}/{}", cur, total);
                }
            });

            let repo = git2::build::RepoBuilder::new()
                .fetch_options(fetch_options)
                .with_checkout(checkout_builder)
                .clone(&url, &path)
                .map_err(|e| GitError::CloneFailed(e.to_string()))?;

            info!("Successfully cloned repository");
            Ok(repo)
        })
        .await
        .map_err(|e| GitError::CloneFailed(e.to_string()))?
    }

    /// Fetch updates from remote asynchronously
    ///
    /// # Errors
    ///
    /// Returns `GitError` if repository doesn't exist or fetch fails
    pub async fn fetch(&self) -> GitResult<()> {
        let path = self.path.clone();
        let credentials = self.credentials.clone();

        task::spawn_blocking(move || {
            let repo = Repository::open(&path)?;

            let mut remote = repo
                .find_remote("origin")
                .map_err(|_| GitError::InvalidReference("origin".to_string()))?;

            let mut callbacks = RemoteCallbacks::new();

            if let Some(creds) = credentials {
                callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                    Cred::userpass_plaintext(&creds.username, &creds.password)
                });
            }

            let mut fetch_options = FetchOptions::new();
            fetch_options.remote_callbacks(callbacks);

            let mut proxy_opts = ProxyOptions::new();
            proxy_opts.auto();
            fetch_options.proxy_options(proxy_opts);

            remote.fetch(&[] as &[&str], Some(&mut fetch_options), None)?;

            info!("Successfully fetched updates");
            Ok(())
        })
        .await
        .map_err(|e| GitError::CloneFailed(e.to_string()))?
    }

    /// Checkout a specific reference (branch, tag, commit) asynchronously
    ///
    /// # Arguments
    ///
    /// * `refspec` - The reference to checkout (e.g., "main", "v1.0.0", commit hash)
    ///
    /// # Errors
    ///
    /// Returns `GitError` if reference is invalid or checkout fails
    pub async fn checkout(&self, refspec: impl Into<String>) -> GitResult<()> {
        let path = self.path.clone();
        let refspec = refspec.into();

        task::spawn_blocking(move || {
            let repo = Repository::open(&path)?;

            // Try to resolve reference
            let oid = if let Ok(oid) = Oid::from_str(&refspec) {
                // Direct commit hash
                oid
            } else {
                // Try as branch or tag
                let reference = repo
                    .find_reference(&format!("refs/remotes/origin/{}", refspec))
                    .or_else(|_| repo.find_reference(&format!("refs/tags/{}", refspec)))
                    .or_else(|_| repo.find_reference(&format!("refs/heads/{}", refspec)))
                    .map_err(|_| GitError::InvalidReference(refspec.clone()))?;

                reference.peel_to_commit()?.id()
            };

            // Detach HEAD and checkout
            repo.set_head_detached(oid)?;

            let mut checkout_builder = CheckoutBuilder::new();
            checkout_builder.force();
            repo.checkout_head(Some(&mut checkout_builder))?;

            info!("Successfully checked out {}", refspec);
            Ok(())
        })
        .await
        .map_err(|e| GitError::CloneFailed(e.to_string()))?
    }

    /// Get current HEAD commit hash
    ///
    /// # Errors
    ///
    /// Returns `GitError` if repository doesn't exist or HEAD is invalid
    pub async fn head_commit(&self) -> GitResult<String> {
        let path = self.path.clone();

        task::spawn_blocking(move || {
            let repo = Repository::open(&path)?;
            let head = repo.head()?;
            let commit = head.peel_to_commit()?;
            Ok(commit.id().to_string())
        })
        .await
        .map_err(|e| GitError::CloneFailed(e.to_string()))?
    }
}

/// Helper function to setup proxy options
fn proxy_opts_auto() -> ProxyOptions<'static> {
    let mut proxy_opts = ProxyOptions::new();
    proxy_opts.auto();
    proxy_opts
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_async_git_repository() {
        let temp = TempDir::new().unwrap();
        let repo = AsyncGitRepository::new(
            temp.path(),
            "https://github.com/rust-lang/rust.git",
            None,
        );

        // Note: This test doesn't actually clone to avoid network dependency
        // In real tests, you would use a local test repository
        assert_eq!(repo.path, temp.path());
    }
}
