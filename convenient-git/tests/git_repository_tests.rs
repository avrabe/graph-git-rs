//! Integration tests for GitRepository
//!
//! These tests verify that GitRepository properly handles errors
//! and returns Result types instead of panicking.

use convenient_git::{GitRepository, RefsKind};
use tempfile::TempDir;

/// Test that creating a GitRepository with an invalid URL returns an error
/// instead of panicking (validates the Result return type change)
#[test]
fn test_new_with_nonexistent_path_and_invalid_url_returns_error() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("nonexistent_repo");

    // Use an invalid URL that will fail to clone
    let git_url = "file:///this/path/does/not/exist".to_string();
    let git_user = "".to_string();
    let git_password = "".to_string();

    // This should return an error, not panic
    let result = GitRepository::new(&repo_path, &git_url, &git_user, &git_password);

    assert!(result.is_err(), "Expected error for invalid URL, got Ok");
}

/// Test that creating a GitRepository with an existing local repository works
#[test]
fn test_new_with_existing_local_repo() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository first
    git2::Repository::init(&repo_path).expect("Failed to init test repo");

    // Now try to open it via GitRepository
    let git_url = format!("file://{}", repo_path.display());
    let git_user = "".to_string();
    let git_password = "".to_string();

    let result = GitRepository::new(&repo_path, &git_url, &git_user, &git_password);

    // Should succeed since the repo already exists
    assert!(result.is_ok(), "Expected Ok for existing repo, got Err: {:?}", result.err());

    let repo = result.unwrap();
    assert!(repo.repo.is_some(), "Repository should be Some");
}

/// Test that find_reference returns None for non-existent reference (not panic)
#[test]
fn test_find_reference_nonexistent_returns_none() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository
    git2::Repository::init(&repo_path).expect("Failed to init test repo");

    let git_url = format!("file://{}", repo_path.display());
    let repo = GitRepository::new(&repo_path, &git_url, &"".to_string(), &"".to_string())
        .expect("Failed to create GitRepository");

    // Try to find a reference that doesn't exist
    let result = repo.find_reference("nonexistent_branch");

    // Should return None, not panic
    assert!(result.is_none(), "Expected None for non-existent reference");
}

/// Test that get_remote_heads returns None when no remote exists (not panic)
#[test]
fn test_get_remote_heads_no_remote_returns_none() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository (no remote configured)
    git2::Repository::init(&repo_path).expect("Failed to init test repo");

    let git_url = format!("file://{}", repo_path.display());
    let repo = GitRepository::new(&repo_path, &git_url, &"".to_string(), &"".to_string())
        .expect("Failed to create GitRepository");

    // Try to get remote heads when no remote exists
    let result = repo.get_remote_heads(RefsKind::Branch);

    // Should return None gracefully, not panic
    assert!(result.is_none(), "Expected None when no remote configured");
}

/// Test update_from_remote handles missing remote gracefully (no panic)
#[test]
fn test_update_from_remote_no_remote_no_panic() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository (no remote configured)
    git2::Repository::init(&repo_path).expect("Failed to init test repo");

    let git_url = format!("file://{}", repo_path.display());
    let repo = GitRepository::new(&repo_path, &git_url, &"".to_string(), &"".to_string())
        .expect("Failed to create GitRepository");

    // This should not panic even though there's no remote
    repo.update_from_remote();

    // If we get here, the test passes (no panic)
}

/// Test checkout returns error for non-existent reference (not panic)
#[test]
fn test_checkout_nonexistent_returns_error() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository
    let git_repo = git2::Repository::init(&repo_path).expect("Failed to init test repo");

    // Create an initial commit so the repo isn't empty
    {
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = git_repo.index().unwrap().write_tree().unwrap();
        let tree = git_repo.find_tree(tree_id).unwrap();
        git_repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[]).unwrap();
    }

    let git_url = format!("file://{}", repo_path.display());
    let repo = GitRepository::new(&repo_path, &git_url, &"".to_string(), &"".to_string())
        .expect("Failed to create GitRepository");

    // Try to checkout a non-existent reference
    let result = repo.checkout("nonexistent_branch");

    // Should return error, not panic
    assert!(result.is_err(), "Expected error for non-existent checkout target");
}

/// Test map_remote_branches_local handles no remote gracefully (no panic)
#[test]
fn test_map_remote_branches_local_no_remote_no_panic() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let repo_path = tmp.path().join("test_repo");

    // Initialize a local git repository (no remote configured)
    git2::Repository::init(&repo_path).expect("Failed to init test repo");

    let git_url = format!("file://{}", repo_path.display());
    let repo = GitRepository::new(&repo_path, &git_url, &"".to_string(), &"".to_string())
        .expect("Failed to create GitRepository");

    // This should not panic even though there's no remote
    repo.map_remote_branches_local();

    // If we get here, the test passes (no panic)
}
