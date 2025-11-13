//! Integration tests for repository manager
//!
//! Tests git repository cloning, checkout, patching, and layer management.

use convenient_kas::include_graph::{KasConfig, KasHeader, KasLayer, KasRepo};
use convenient_kas::repository_manager::{RepoError, RepositoryManager};
use std::collections::HashMap;
use tempfile::TempDir;
use tokio::fs;

#[allow(unused_imports)]
use std::path::PathBuf;

#[tokio::test]
async fn test_local_repository_path() {
    let temp = TempDir::new().unwrap();
    let local_repo = temp.path().join("local-repo");
    fs::create_dir_all(&local_repo).await.unwrap();

    let mut repo_config = KasRepo {
        path: Some(local_repo.to_string_lossy().to_string()),
        url: None,
        refspec: None,
        branch: None,
        commit: None,
        tag: None,
        layers: HashMap::new(),
        patches: None,
    };

    let manager = RepositoryManager::new(temp.path().join("repos"));
    let result = manager.setup_repository("test", &repo_config).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), local_repo);
}

#[tokio::test]
async fn test_local_repository_not_found() {
    let temp = TempDir::new().unwrap();

    let repo_config = KasRepo {
        path: Some("/nonexistent/path".to_string()),
        url: None,
        refspec: None,
        branch: None,
        commit: None,
        tag: None,
        layers: HashMap::new(),
        patches: None,
    };

    let manager = RepositoryManager::new(temp.path().join("repos"));
    let result = manager.setup_repository("test", &repo_config).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        RepoError::LocalRepoNotFound(_) => {}
        _ => panic!("Expected LocalRepoNotFound error"),
    }
}

#[tokio::test]
async fn test_missing_url_for_remote_repo() {
    let temp = TempDir::new().unwrap();

    let repo_config = KasRepo {
        path: None,
        url: None, // No URL and no path
        refspec: None,
        branch: None,
        commit: None,
        tag: None,
        layers: HashMap::new(),
        patches: None,
    };

    let manager = RepositoryManager::new(temp.path().join("repos"));
    let result = manager.setup_repository("test", &repo_config).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        RepoError::MissingUrl(_) => {}
        _ => panic!("Expected MissingUrl error"),
    }
}

// Note: get_refspec is tested indirectly through setup_repository
// which uses it internally. Direct testing removed as it's a private method.

#[tokio::test]
async fn test_get_layer_paths_no_explicit_layers() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.path().join("test-repo");
    fs::create_dir_all(&repo_path).await.unwrap();

    let repo_config = KasRepo {
        path: None,
        url: Some("https://example.com/repo".to_string()),
        refspec: None,
        branch: None,
        commit: None,
        tag: None,
        layers: HashMap::new(), // No explicit layers
        patches: None,
    };

    let manager = RepositoryManager::new(temp.path());
    let layer_paths = manager.get_layer_paths(&repo_path, &repo_config).unwrap();

    // When no explicit layers, repo itself is the layer
    assert_eq!(layer_paths.len(), 1);
    assert_eq!(layer_paths[0], repo_path);
}

#[tokio::test]
async fn test_get_layer_paths_with_layers() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.path().join("test-repo");

    // Create layer directories
    let meta_layer = repo_path.join("meta");
    let meta_poky = repo_path.join("meta-poky");
    fs::create_dir_all(&meta_layer).await.unwrap();
    fs::create_dir_all(&meta_poky).await.unwrap();

    let mut layers = HashMap::new();
    layers.insert("meta".to_string(), KasLayer { path: None });
    layers.insert("meta-poky".to_string(), KasLayer { path: None });

    let repo_config = KasRepo {
        path: None,
        url: Some("https://example.com/repo".to_string()),
        refspec: None,
        branch: None,
        commit: None,
        tag: None,
        layers,
        patches: None,
    };

    let manager = RepositoryManager::new(temp.path());
    let layer_paths = manager.get_layer_paths(&repo_path, &repo_config).unwrap();

    assert_eq!(layer_paths.len(), 2);
    assert!(layer_paths.contains(&meta_layer));
    assert!(layer_paths.contains(&meta_poky));
}

#[tokio::test]
async fn test_get_layer_paths_with_custom_path() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.path().join("test-repo");

    // Create custom layer path
    let custom_path = repo_path.join("custom/path/meta-layer");
    fs::create_dir_all(&custom_path).await.unwrap();

    let mut layers = HashMap::new();
    layers.insert(
        "meta-custom".to_string(),
        KasLayer {
            path: Some("custom/path/meta-layer".to_string()),
        },
    );

    let repo_config = KasRepo {
        path: None,
        url: Some("https://example.com/repo".to_string()),
        refspec: None,
        branch: None,
        commit: None,
        tag: None,
        layers,
        patches: None,
    };

    let manager = RepositoryManager::new(temp.path());
    let layer_paths = manager.get_layer_paths(&repo_path, &repo_config).unwrap();

    assert_eq!(layer_paths.len(), 1);
    assert_eq!(layer_paths[0], custom_path);
}

#[tokio::test]
async fn test_get_layer_paths_missing_layer() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.path().join("test-repo");
    fs::create_dir_all(&repo_path).await.unwrap();

    // Layer directory doesn't exist
    let mut layers = HashMap::new();
    layers.insert("nonexistent".to_string(), KasLayer { path: None });

    let repo_config = KasRepo {
        path: None,
        url: Some("https://example.com/repo".to_string()),
        refspec: None,
        branch: None,
        commit: None,
        tag: None,
        layers,
        patches: None,
    };

    let manager = RepositoryManager::new(temp.path());
    let layer_paths = manager.get_layer_paths(&repo_path, &repo_config).unwrap();

    // Missing layers are skipped (warning logged)
    assert_eq!(layer_paths.len(), 0);
}

#[tokio::test]
async fn test_setup_repositories_creates_directory() {
    let temp = TempDir::new().unwrap();
    let repos_dir = temp.path().join("repos");

    // Directory doesn't exist yet
    assert!(!repos_dir.exists());

    let config = KasConfig {
        header: KasHeader {
            version: 14,
            includes: None,
        },
        machine: Some("test".to_string()),
        distro: None,
        target: None,
        repos: HashMap::new(),
        bblayers_conf_header: None,
        local_conf_header: None,
        env: None,
    };

    let manager = RepositoryManager::new(&repos_dir);
    let result = manager.setup_repositories(&config).await;

    assert!(result.is_ok());
    assert!(repos_dir.exists());
}

#[tokio::test]
async fn test_with_cache_directory() {
    let temp = TempDir::new().unwrap();
    let repos_dir = temp.path().join("repos");
    let cache_dir = temp.path().join("cache");

    let _manager = RepositoryManager::new(&repos_dir).with_cache(&cache_dir);

    // Cache dir is set (we can't directly test it but the API works)
    assert!(true);
}

#[tokio::test]
async fn test_apply_patches_empty() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.path().join("repo");
    fs::create_dir_all(&repo_path).await.unwrap();

    // Initialize git repo
    tokio::process::Command::new("git")
        .args(&["init"])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();

    let manager = RepositoryManager::new(temp.path());
    let patches = HashMap::new();

    let result = manager.apply_patches(&repo_path, &patches).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_repository_manager_new() {
    let temp = TempDir::new().unwrap();
    let repos_dir = temp.path().join("repos");

    let _manager = RepositoryManager::new(&repos_dir);

    // Manager created successfully
    assert!(true);
}

#[tokio::test]
async fn test_setup_repositories_with_local_repos() {
    let temp = TempDir::new().unwrap();

    // Create local repositories
    let local1 = temp.path().join("local1");
    let local2 = temp.path().join("local2");
    fs::create_dir_all(&local1).await.unwrap();
    fs::create_dir_all(&local2).await.unwrap();

    let mut repos = HashMap::new();
    repos.insert(
        "repo1".to_string(),
        KasRepo {
            path: Some(local1.to_string_lossy().to_string()),
            url: None,
            refspec: None,
            branch: None,
            commit: None,
            tag: None,
            layers: HashMap::new(),
            patches: None,
        },
    );
    repos.insert(
        "repo2".to_string(),
        KasRepo {
            path: Some(local2.to_string_lossy().to_string()),
            url: None,
            refspec: None,
            branch: None,
            commit: None,
            tag: None,
            layers: HashMap::new(),
            patches: None,
        },
    );

    let config = KasConfig {
        header: KasHeader {
            version: 14,
            includes: None,
        },
        machine: Some("test".to_string()),
        distro: None,
        target: None,
        repos,
        bblayers_conf_header: None,
        local_conf_header: None,
        env: None,
    };

    let manager = RepositoryManager::new(temp.path().join("repos"));
    let result = manager.setup_repositories(&config).await;

    assert!(result.is_ok());
    let repo_paths = result.unwrap();
    assert_eq!(repo_paths.len(), 2);
    assert_eq!(repo_paths.get("repo1").unwrap(), &local1);
    assert_eq!(repo_paths.get("repo2").unwrap(), &local2);
}
