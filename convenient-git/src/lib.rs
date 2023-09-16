use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use git2::{
    build::CheckoutBuilder, BranchType, Commit, FetchOptions, Progress, RemoteCallbacks, Repository,
};
use tracing::{error, info, span, Level};

pub struct GitRemoteHead {
    pub oid: String,
    pub name: String,
}

pub struct GitCommit {
    pub name: String,
    pub email: String,
    pub message: String,
}

impl GitCommit {
    pub fn new(commit: Commit) -> GitCommit {
        GitCommit {
            name: commit.author().name().unwrap().to_string(),
            email: commit.author().email().unwrap().to_string(),
            message: commit.message().unwrap().to_string(),
        }
    }
}

pub struct GitRepository {
    pub repo: Option<Repository>,
    pub git_url: String,
}

struct State {
    progress: Option<Progress<'static>>,
    total: usize,
    current: usize,
    path: Option<PathBuf>,
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
    pub fn new(repo_path: &Path, git_url: &String) -> GitRepository {
        let span = span!(Level::INFO, "clone");
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
            GitRepository::print(&mut *state);
            true
        });

        let mut co = CheckoutBuilder::new();
        co.progress(|path, cur, total| {
            let mut state = state.borrow_mut();
            state.path = path.map(|p| p.to_path_buf());
            state.current = cur;
            state.total = total;
            GitRepository::print(&mut *state);
        });

        let mut fo = FetchOptions::new();
        fo.remote_callbacks(cb);

        // Try opening the repository
        let mut binding = git2::build::RepoBuilder::new();
        let builder = binding.bare(true).fetch_options(fo).with_checkout(co);

        let repo = match Repository::open(repo_path) {
            Ok(repo) => repo,
            Err(e) if e.code() == git2::ErrorCode::NotFound => {
                // Repository not found, clone it
                builder.clone(git_url, Path::new(repo_path)).unwrap()
            }
            Err(e) => {
                // Some other error, panic
                panic!("failed to open: {}", e);
            }
        };
        GitRepository {
            repo: Some(repo),
            git_url: git_url.to_string(),
        }
    }

    fn print(state: &mut State) {
        let stats = state.progress.as_ref().unwrap();
        let network_pct = (100 * stats.received_objects()) / stats.total_objects();
        let index_pct = (100 * stats.indexed_objects()) / stats.total_objects();
        let co_pct = if state.total > 0 {
            (100 * state.current) / state.total
        } else {
            0
        };
        let kbytes = stats.received_bytes() / 1024;
        if stats.received_objects() == stats.total_objects() {
            info!(
                "Resolving deltas {}/{}\r",
                stats.indexed_deltas(),
                stats.total_deltas()
            );
        } else {
            info!(
                "net {:3}% ({:4} kb, {:5}/{:5})  /  idx {:3}% ({:5}/{:5})  \
                 /  chk {:3}% ({:4}/{:4}) {}\r",
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
            )
        }
    }

    pub fn update_from_remote(&self) {
        let span = span!(Level::INFO, "update");
        let _enter = span.enter();
        if let Some(repo) = &self.repo {
            let mut fo = FetchOptions::new();
            match repo.find_remote("origin") {
                Ok(mut remote) => {
                    remote.connect(git2::Direction::Fetch).unwrap();
                    remote.download(&[] as &[&str], Some(&mut fo)).unwrap();
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
            };
        } else {
            error!("No repository found");
        }
    }

    pub fn map_remote_branches_local(&self) {
        let span = span!(Level::INFO, "map");
        let _enter = span.enter();

        if let Some(repo) = &self.repo {
            let mut remote = repo.find_remote("origin").unwrap();
            remote.connect(git2::Direction::Fetch).unwrap();

            for branch in remote.list().unwrap() {
                if branch.name().starts_with("refs/heads") {
                    let branch_name = branch.name().to_string();
                    let branch_ref = branch.oid();
                    let branch_commit = repo.find_commit(branch_ref).unwrap();

                    if let Ok(local_branch) = repo.find_branch(&branch_name, BranchType::Local) {
                        if !local_branch.is_head() {
                            repo.branch(&branch_name, &branch_commit, true).unwrap();
                        }
                    } else {
                        repo.branch(&branch_name, &branch_commit, true).unwrap();
                    }
                }
            }
        } else {
            error!("No repository found");
        }
    }

    pub fn get_remote_heads(&self) -> Option<Vec<GitRemoteHead>> {
        let span = span!(Level::INFO, "get_remote_heads");
        let _enter = span.enter();
        if let Some(repo) = &self.repo {
            let mut remote = repo.find_remote("origin").unwrap();
            let _ = remote.connect(git2::Direction::Fetch);
            let git_remote_heads = remote
                .list()
                .unwrap()
                .iter()
                .filter(|branch| branch.name().starts_with("refs/heads/"))
                .map(|branch| GitRemoteHead {
                    oid: branch.oid().to_string(),
                    name: branch
                        .name()
                        .to_string()
                        .split("refs/heads/")
                        .last()
                        .unwrap()
                        .to_string(),
                })
                .collect::<Vec<GitRemoteHead>>();
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
            let remote_name = format!("refs/remotes/origin/{}", name);
            let reference = repo.find_reference(remote_name.as_str());
            match reference {
                Ok(reference) => Some(GitCommit::new(reference.peel_to_commit().unwrap().clone())),
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
