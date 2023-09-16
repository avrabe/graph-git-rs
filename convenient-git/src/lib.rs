use std::path::Path;

use git2::{BranchType, Commit, FetchOptions, Repository};
use tracing::error;

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
        // Try opening the repository
        let mut builder = git2::build::RepoBuilder::new();
        builder.bare(true);

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

    pub fn update_from_remote(&self) {
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
