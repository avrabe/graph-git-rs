//! Convenient Git - High-performance async git operations
//!
//! Provides both legacy synchronous and modern async git operations
//! with proper error handling and progress tracking.

#![warn(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::pedantic
)]
#![allow(
    missing_docs,  // TODO: Add comprehensive docs for public API
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::similar_names,
    clippy::needless_pass_by_value,
    clippy::needless_continue
)]

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use git2::{
    build::CheckoutBuilder, BranchType, Commit, Cred, FetchOptions, Oid, Progress, ProxyOptions,
    Remote, RemoteCallbacks, Repository,
};
use tracing::{error, info, info_span, span, warn, Level};

pub mod async_git;

pub struct GitRemoteHead {
    pub oid: String,
    pub name: String,
}

pub struct GitCommit {
    pub oid: String,
    pub name: String,
    pub email: String,
    pub message: String,
}

impl GitCommit {
    pub fn new(commit: Commit) -> GitCommit {
        GitCommit {
            oid: commit.id().to_string(),
            name: commit.author().name().unwrap_or("unknown").to_string(),
            email: commit.author().email().unwrap_or("unknown@unknown").to_string(),
            message: commit.message().unwrap_or("").to_string(),
        }
    }
}

pub struct GitRepository {
    pub repo: Option<Repository>,
    pub git_url: String,
    pub git_user: String,
    pub git_password: String,
}

struct State {
    progress: Option<Progress<'static>>,
    total: usize,
    current: usize,
    path: Option<PathBuf>,
}

pub enum RefsKind {
    Tag,
    Branch,
}

/// Creates a new Git repository instance by first trying to open the
/// repository at the provided path. If that fails with a NotFound error,
/// it will clone the repository from the provided URL into the given path.
///
/// # Arguments
///
/// * `repo_path` - The path where the repository should be created/opened.
/// * `git_url` - The URL of the Git repository to clone if not found locally.
///
/// # Returns
///
/// A `Repository` instance for the repository at the provided path,
/// cloning from the URL if it did not already exist.
///
/// # Panics
///
/// Panics if failed to open the repository for any reason other than it not existing.
impl GitRepository {
    fn proxy_opts_auto() -> ProxyOptions<'static> {
        let mut proxy_opts = ProxyOptions::new();
        proxy_opts.auto();
        proxy_opts
    }

    fn remote_connect(&self, remote: &mut Remote<'_>) -> Result<(), git2::Error> {
        let mut cb = RemoteCallbacks::new();
        cb.credentials(|_url, _username_from_url, _allowed_types| {
            Cred::userpass_plaintext(&self.git_user, &self.git_password)
        });
        remote.connect_auth(
            git2::Direction::Fetch,
            Some(cb),
            Some(Self::proxy_opts_auto()),
        )?;
        Ok(())
    }

    pub fn new(
        repo_path: &Path,
        git_url: &str,
        git_user: &str,
        git_password: &str,
    ) -> Result<GitRepository, git2::Error> {
        let span = span!(Level::INFO, "clone", uri=%git_url);
        let _enter = span.enter();

        let state = RefCell::new(State {
            progress: None,
            total: 0,
            current: 0,
            path: None,
        });
        let mut cb = RemoteCallbacks::new();
        cb.transfer_progress(|stats| {
            let mut state = state.borrow_mut();
            state.progress = Some(stats.to_owned());
            GitRepository::print(&mut state);
            true
        });

        let mut co = CheckoutBuilder::new();
        co.progress(|path, cur, total| {
            let mut state = state.borrow_mut();
            state.path = path.map(std::path::Path::to_path_buf);
            state.current = cur;
            state.total = total;
            GitRepository::print(&mut state);
        });
        cb.credentials(|_url, _username_from_url, _allowed_types| {
            Cred::userpass_plaintext(git_user, git_password)
        });
        let mut fo = FetchOptions::new();
        fo.remote_callbacks(cb);
        fo.proxy_options(Self::proxy_opts_auto());
        fo.download_tags(git2::AutotagOption::All);

        // Try opening the repository
        let mut binding = git2::build::RepoBuilder::new();
        let builder = binding.bare(false).fetch_options(fo).with_checkout(co);

        let repo = match Repository::open(repo_path) {
            Ok(repo) => repo,
            Err(e) if e.code() == git2::ErrorCode::NotFound => {
                // Repository not found, clone it
                const MAX_CLONE_RETRIES: u32 = 3;
                let mut last_error = e;
                for attempt in 1..=MAX_CLONE_RETRIES {
                    match builder.clone(git_url, Path::new(repo_path)) {
                        Ok(repo) => return Ok(GitRepository {
                            repo: Some(repo),
                            git_url: git_url.to_owned(),
                            git_user: git_user.to_owned(),
                            git_password: git_password.to_owned(),
                        }),
                        Err(e) => {
                            warn!("Clone attempt {}/{} failed: {}", attempt, MAX_CLONE_RETRIES, e);
                            last_error = e;
                        }
                    }
                }
                return Err(last_error);
            }
            Err(e) => {
                // Some other error, return it
                return Err(e);
            }
        };
        Ok(GitRepository {
            repo: Some(repo),
            git_url: git_url.to_owned(),
            git_user: git_user.to_owned(),
            git_password: git_password.to_owned(),
        })
    }

    fn is_multiple_of_one_percent_or_total(n: usize, total: usize) -> bool {
        let total_one_percent: usize = total / 100;
        let total_one_percent = if total_one_percent == 0 {
            1
        } else {
            total_one_percent
        };
        n.is_multiple_of(total_one_percent) || n == total
    }

    fn print(state: &mut State) {
        let Some(stats) = state.progress.as_ref() else {
            return;
        };
        if stats.total_objects() == 0 {
            return;
        }
        let network_pct = (100 * stats.received_objects()) / stats.total_objects();
        let index_pct = (100 * stats.indexed_objects()) / stats.total_objects();
        let co_pct = if state.total > 0 {
            (100 * state.current) / state.total
        } else {
            0
        };
        let kbytes = stats.received_bytes() / 1024;
        if stats.received_objects() == stats.total_objects() {
            if Self::is_multiple_of_one_percent_or_total(
                stats.indexed_deltas(),
                stats.total_deltas(),
            ) {
                info!(
                    "Resolving deltas {}/{}",
                    stats.indexed_deltas(),
                    stats.total_deltas()
                );
            }
        } else if Self::is_multiple_of_one_percent_or_total(
            stats.received_objects(),
            stats.total_objects(),
        ) || Self::is_multiple_of_one_percent_or_total(
            stats.indexed_objects(),
            stats.total_objects(),
        ) || co_pct > 0
        {
            info!(
                "net {:3}% ({:4}kb,{:5}/{:5})/idx {:3}% ({:5}/{:5})/chk {:3}% ({:4}/{:4}) {}",
                network_pct,
                kbytes,
                stats.received_objects(),
                stats.total_objects(),
                index_pct,
                stats.indexed_objects(),
                stats.total_objects(),
                co_pct,
                state.current,
                state.total,
                state
                    .path
                    .as_ref()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            );
        }
    }

    pub fn update_from_remote(&self) {
        let span = span!(Level::INFO, "update");
        let _enter = span.enter();
        if let Some(repo) = &self.repo {
            let mut cb = RemoteCallbacks::new();
            cb.credentials(|_url, _username_from_url, _allowed_types| {
                Cred::userpass_plaintext(&self.git_user, &self.git_password)
            });

            let mut fo = FetchOptions::new();
            fo.remote_callbacks(cb);
            fo.proxy_options(Self::proxy_opts_auto());
            fo.download_tags(git2::AutotagOption::All);
            match repo.find_remote("origin") {
                Ok(mut remote) => {
                    if let Err(e) = self.remote_connect(&mut remote) {
                        error!("Failed to connect to remote: {}", e);
                        return;
                    }
                    if let Err(e) = remote.download(&[] as &[&str], Some(&mut fo)) {
                        error!("Download failed: {}", e);
                    }
                    let _ = remote.disconnect();
                }
                Err(e) if e.code() == git2::ErrorCode::NotFound => {
                    // No origin remote found
                    error!("No remote found");
                }
                Err(e) => {
                    // Some other error
                    error!("Error: {}", e);
                }
            }
        } else {
            error!("No repository found");
        }
    }

    pub fn checkout(&self, name: &str) -> Result<(), git2::Error> {
        let span = info_span!("checkout", to = &name);
        let _enter = span.enter();
        info!("Checking out {}", name);
        if let Some(repo) = &self.repo {
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            if let Some(reference) = self.find_reference(name) {
                let oid = Oid::from_str(&reference.oid)
                    .map_err(|_| git2::Error::from_str("Invalid OID"))?;
                repo.set_head_detached(oid)?;
                repo.checkout_head(Some(&mut co))?;
                info!("Checked out {}", name);
                Ok(())
            } else {
                error!("Error: {}", name);
                Err(git2::Error::from_str("No reference found"))
            }
        } else {
            error!("No repository found");
            Err(git2::Error::from_str("No repository found"))
        }
    }

    pub fn map_remote_branches_local(&self) {
        let span = span!(Level::INFO, "map");
        let _enter = span.enter();

        if let Some(repo) = &self.repo {
            let mut remote = match repo.find_remote("origin") {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to find origin remote: {}", e);
                    return;
                }
            };
            if let Err(e) = self.remote_connect(&mut remote) {
                error!("Failed to connect to remote: {}", e);
                return;
            }

            let branches = match remote.list() {
                Ok(b) => b,
                Err(e) => {
                    error!("Failed to list remote branches: {}", e);
                    return;
                }
            };

            for branch in branches {
                if branch.name().starts_with("refs/heads") {
                    let branch_name = branch.name().to_string();
                    let branch_ref = branch.oid();
                    let branch_commit = match repo.find_commit(branch_ref) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("Failed to find commit for branch {}: {}", branch_name, e);
                            continue;
                        }
                    };

                    if let Ok(local_branch) = repo.find_branch(&branch_name, BranchType::Local) {
                        if !local_branch.is_head() {
                            if let Err(e) = repo.branch(&branch_name, &branch_commit, true) {
                                warn!("Failed to create branch {}: {}", branch_name, e);
                            }
                        }
                    } else if let Err(e) = repo.branch(&branch_name, &branch_commit, true) {
                        warn!("Failed to create branch {}: {}", branch_name, e);
                    }
                }
            }
        } else {
            error!("No repository found");
        }
    }

    pub fn get_remote_heads(&self, refs_kind: RefsKind) -> Option<Vec<GitRemoteHead>> {
        let span = span!(Level::INFO, "get_remote_heads");
        let repo = match &self.repo {
            Some(repo) => repo,
            None => return None,
        };
        let _ = repo.tag_foreach({
            |oid, name| {
                info!("Found tag{}: {:?}", oid, name);
                true
            }
        });
        let refs_string = match refs_kind {
            RefsKind::Tag => "refs/tags/",
            RefsKind::Branch => "refs/heads/",
        };
        let _enter = span.enter();
        if let Some(repo) = &self.repo {
            let mut remote = match repo.find_remote("origin") {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to find origin remote: {}", e);
                    return None;
                }
            };
            if let Err(e) = self.remote_connect(&mut remote) {
                error!("Failed to connect to remote: {}", e);
                return None;
            }

            let branch_list = match remote.list() {
                Ok(list) => list,
                Err(e) => {
                    error!("Failed to list remote branches: {}", e);
                    return None;
                }
            };

            let git_remote_heads = branch_list
                .iter()
                .filter(|branch| branch.name().starts_with(refs_string))
                .filter_map(|branch| {
                    let name = branch.name().split(refs_string).last()?;
                    Some(GitRemoteHead {
                        oid: branch.oid().to_string(),
                        name: name.to_string(),
                    })
                })
                .collect::<Vec<GitRemoteHead>>();
            info!("Found {} remote heads", git_remote_heads.len());
            Some(git_remote_heads)
        } else {
            error!("No repository found");
            None
        }
    }

    pub fn find_reference(&self, name: &str) -> Option<GitCommit> {
        let span = span!(Level::INFO, "find_reference");
        let _enter = span.enter();
        if let Some(repo) = &self.repo {
            let remote_name = format!("refs/remotes/origin/{name}");
            let reference = repo.find_reference(remote_name.as_str());
            match reference {
                Ok(reference) => match reference.peel_to_commit() {
                    Ok(commit) => Some(GitCommit::new(commit)),
                    Err(e) => {
                        error!("Failed to peel reference to commit: {}", e);
                        None
                    }
                },
                Err(e) => {
                    error!("Error: {}", e);
                    None
                }
            }
        } else {
            error!("No repository found");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_multiple_of_one_percent_or_total() {
        assert!(GitRepository::is_multiple_of_one_percent_or_total(1, 100));
        assert!(GitRepository::is_multiple_of_one_percent_or_total(50, 100));
        assert!(GitRepository::is_multiple_of_one_percent_or_total(100, 100));
        assert!(!GitRepository::is_multiple_of_one_percent_or_total(
            99, 1000
        ));
        assert!(GitRepository::is_multiple_of_one_percent_or_total(
            100, 1000
        ));

        assert!(GitRepository::is_multiple_of_one_percent_or_total(2, 100));
        assert!(GitRepository::is_multiple_of_one_percent_or_total(51, 100));

        assert!(GitRepository::is_multiple_of_one_percent_or_total(1, 1));
        assert!(GitRepository::is_multiple_of_one_percent_or_total(2, 1));

        assert!(GitRepository::is_multiple_of_one_percent_or_total(0, 0));
    }
}
