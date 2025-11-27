//! Integration tests for AsyncGitRepository
//!
//! These tests verify async git operations and error handling.

use convenient_git::async_git::{AsyncGitRepository, GitCredentials, GitError};
use std::path::Path;
use tempfile::TempDir;

/// Test creating an AsyncGitRepository instance
#[test]
fn test_async_git_repository_new() {
    let repo = AsyncGitRepository::new(
        Path::new("/tmp/test"),
        "https://github.com/example/repo.git",
        None,
    );

    // Just verify construction doesn't panic
    assert!(true);
    drop(repo);
}

/// Test creating with credentials
#[test]
fn test_async_git_repository_new_with_credentials() {
    let creds = GitCredentials {
        username: "test_user".to_string(),
        password: "test_pass".to_string(),
    };

    let repo = AsyncGitRepository::new(
        Path::new("/tmp/test"),
        "https://github.com/example/repo.git",
        Some(creds),
    );

    // Just verify construction doesn't panic
    assert!(true);
    drop(repo);
}

/// Test clone_or_open with existing local repository
#[tokio::test]
async fn test_clone_or_open_existing_repo() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository first
    git2::Repository::init(&repo_path).expect("Failed to init test repo");

    // Now try to open it via AsyncGitRepository
    let repo = AsyncGitRepository::new(
        &repo_path,
        "file:///dummy", // URL doesn't matter since repo exists
        None,
    );

    let result = repo.clone_or_open().await;
    assert!(result.is_ok(), "Expected to open existing repo, got error: {:?}", result.err());
}

/// Test clone_or_open with non-existent path and invalid URL returns error
#[tokio::test]
async fn test_clone_or_open_invalid_url_returns_error() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("nonexistent_repo");

    // Use invalid URL
    let repo = AsyncGitRepository::new(
        &repo_path,
        "file:///this/path/does/not/exist",
        None,
    );

    let result = repo.clone_or_open().await;
    assert!(result.is_err(), "Expected error for invalid URL");

    // Verify it's a CloneFailed error
    match result {
        Err(GitError::CloneFailed(_)) => (),
        Err(GitError::Git(_)) => (), // Also acceptable
        Err(e) => panic!("Unexpected error type: {:?}", e),
        Ok(_) => panic!("Expected error"),
    }
}

/// Test fetch on non-existent repository returns error
#[tokio::test]
async fn test_fetch_nonexistent_repo_returns_error() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("nonexistent_repo");

    let repo = AsyncGitRepository::new(
        &repo_path,
        "https://example.com/repo.git",
        None,
    );

    let result = repo.fetch().await;
    assert!(result.is_err(), "Expected error when fetching non-existent repo");
}

/// Test fetch on repository without remote returns error
#[tokio::test]
async fn test_fetch_no_remote_returns_error() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository without remote
    git2::Repository::init(&repo_path).expect("Failed to init test repo");

    let repo = AsyncGitRepository::new(
        &repo_path,
        "https://example.com/repo.git",
        None,
    );

    let result = repo.fetch().await;
    assert!(result.is_err(), "Expected error when fetching repo without remote");
}

/// Test checkout on non-existent repository returns error
#[tokio::test]
async fn test_checkout_nonexistent_repo_returns_error() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("nonexistent_repo");

    let repo = AsyncGitRepository::new(
        &repo_path,
        "https://example.com/repo.git",
        None,
    );

    let result = repo.checkout("main").await;
    assert!(result.is_err(), "Expected error when checking out non-existent repo");
}

/// Test head_commit on non-existent repository returns error
#[tokio::test]
async fn test_head_commit_nonexistent_repo_returns_error() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("nonexistent_repo");

    let repo = AsyncGitRepository::new(
        &repo_path,
        "https://example.com/repo.git",
        None,
    );

    let result = repo.head_commit().await;
    assert!(result.is_err(), "Expected error for head_commit on non-existent repo");
}

/// Test head_commit on empty repository (no commits) returns error
#[tokio::test]
async fn test_head_commit_empty_repo_returns_error() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository (empty, no commits)
    git2::Repository::init(&repo_path).expect("Failed to init test repo");

    let repo = AsyncGitRepository::new(
        &repo_path,
        "file:///dummy",
        None,
    );

    let result = repo.head_commit().await;
    assert!(result.is_err(), "Expected error for head_commit on empty repo");
}

/// Test head_commit on repository with commits succeeds
#[tokio::test]
async fn test_head_commit_with_commits_succeeds() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository with a commit
    let git_repo = git2::Repository::init(&repo_path).expect("Failed to init test repo");
    {
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = git_repo.index().unwrap().write_tree().unwrap();
        let tree = git_repo.find_tree(tree_id).unwrap();
        git_repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[]).unwrap();
    }

    let repo = AsyncGitRepository::new(
        &repo_path,
        "file:///dummy",
        None,
    );

    let result = repo.head_commit().await;
    assert!(result.is_ok(), "Expected Ok for head_commit on repo with commits, got: {:?}", result.err());

    let commit_id = result.unwrap();
    assert!(!commit_id.is_empty(), "Commit ID should not be empty");
    // Verify it looks like a git commit SHA (40 hex characters)
    assert_eq!(commit_id.len(), 40, "Commit ID should be 40 characters");
    assert!(commit_id.chars().all(|c| c.is_ascii_hexdigit()), "Commit ID should be hex");
}

/// Test GitError Display implementations
#[test]
fn test_git_error_display() {
    let err = GitError::NotFound(std::path::PathBuf::from("/test/path"));
    let display = format!("{}", err);
    assert!(display.contains("/test/path"), "Error display should contain path");

    let err = GitError::InvalidReference("test_ref".to_string());
    let display = format!("{}", err);
    assert!(display.contains("test_ref"), "Error display should contain reference");

    let err = GitError::AuthFailed;
    let display = format!("{}", err);
    assert!(display.contains("Authentication"), "Error display should mention authentication");

    let err = GitError::CloneFailed("test error".to_string());
    let display = format!("{}", err);
    assert!(display.contains("test error"), "Error display should contain error message");
}

/// Test GitCredentials clone
#[test]
fn test_git_credentials_clone() {
    let creds = GitCredentials {
        username: "user".to_string(),
        password: "pass".to_string(),
    };

    let cloned = creds.clone();
    assert_eq!(cloned.username, "user");
    assert_eq!(cloned.password, "pass");
}
