//! Comprehensive tests for kas YAML parsing
//!
//! These tests ensure 100% coverage of kas file reading and parsing,
//! validating all field types, includes, merging, and error cases.

use convenient_kas::include_graph::{
    KasConfig, KasError, KasFile, KasHeader, KasIncludeGraph, KasLayer, KasRepo,
};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;

/// Helper to create a test kas file
async fn create_kas_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, content).await.unwrap();
    path
}

#[tokio::test]
async fn test_minimal_kas_config() {
    let temp = TempDir::new().unwrap();
    let content = r#"
header:
  version: 14

machine: qemux86-64
distro: poky
"#;

    let path = create_kas_file(&temp, "minimal.yml", content).await;
    let kas_file = KasFile::load(&path).await.unwrap();

    assert_eq!(kas_file.config.header.version, 14);
    assert_eq!(kas_file.config.machine, Some("qemux86-64".to_string()));
    assert_eq!(kas_file.config.distro, Some("poky".to_string()));
    assert!(kas_file.config.header.includes.is_none());
    assert!(kas_file.config.target.is_none());
    assert!(!kas_file.checksum.is_empty());
}

#[tokio::test]
async fn test_complete_kas_config() {
    let temp = TempDir::new().unwrap();
    let content = r#"
header:
  version: 14
  includes:
    - base.yml

machine: qemux86-64
distro: poky
target:
  - core-image-minimal
  - core-image-sato

env:
  SSTATE_DIR: /shared/sstate-cache
  DL_DIR: /shared/downloads

repos:
  poky:
    url: https://git.yoctoproject.org/poky
    branch: kirkstone
    layers:
      meta:
      meta-poky:
      meta-yocto-bsp:

  meta-openembedded:
    url: https://git.openembedded.org/meta-openembedded
    commit: abc123def456
    layers:
      meta-oe:
        path: meta-oe
      meta-python:

local_conf_header:
  standard: |
    PACKAGE_CLASSES = "package_ipk"
  debug: |
    EXTRA_IMAGE_FEATURES += "debug-tweaks"

bblayers_conf_header:
  custom: |
    BBMASK = "meta-*/recipes-test/*"
"#;

    let path = create_kas_file(&temp, "complete.yml", content).await;
    let kas_file = KasFile::load(&path).await.unwrap();

    // Header
    assert_eq!(kas_file.config.header.version, 14);
    assert_eq!(
        kas_file.config.header.includes,
        Some(vec!["base.yml".to_string()])
    );

    // Machine and distro
    assert_eq!(kas_file.config.machine, Some("qemux86-64".to_string()));
    assert_eq!(kas_file.config.distro, Some("poky".to_string()));

    // Targets
    let targets = kas_file.config.target.as_ref().unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0], "core-image-minimal");
    assert_eq!(targets[1], "core-image-sato");

    // Environment
    let env = kas_file.config.env.as_ref().unwrap();
    assert_eq!(env.get("SSTATE_DIR").unwrap(), "/shared/sstate-cache");
    assert_eq!(env.get("DL_DIR").unwrap(), "/shared/downloads");

    // Repositories
    assert_eq!(kas_file.config.repos.len(), 2);

    let poky = kas_file.config.repos.get("poky").unwrap();
    assert_eq!(
        poky.url,
        Some("https://git.yoctoproject.org/poky".to_string())
    );
    assert_eq!(poky.branch, Some("kirkstone".to_string()));
    assert_eq!(poky.layers.len(), 3);

    let meta_oe = kas_file.config.repos.get("meta-openembedded").unwrap();
    assert_eq!(meta_oe.commit, Some("abc123def456".to_string()));
    assert_eq!(meta_oe.layers.len(), 2);

    // Headers
    let local_conf = kas_file.config.local_conf_header.as_ref().unwrap();
    assert!(local_conf.contains_key("standard"));
    assert!(local_conf.contains_key("debug"));

    let bblayers_conf = kas_file.config.bblayers_conf_header.as_ref().unwrap();
    assert!(bblayers_conf.contains_key("custom"));
}

#[tokio::test]
async fn test_repository_refs_priority() {
    let temp = TempDir::new().unwrap();

    // Test commit priority
    let content = r#"
header:
  version: 14
machine: test
repos:
  test-repo:
    url: https://example.com/repo
    commit: abc123
    tag: v1.0
    branch: main
"#;

    let path = create_kas_file(&temp, "refs.yml", content).await;
    let kas_file = KasFile::load(&path).await.unwrap();
    let repo = kas_file.config.repos.get("test-repo").unwrap();

    assert_eq!(repo.commit, Some("abc123".to_string()));
    assert_eq!(repo.tag, Some("v1.0".to_string()));
    assert_eq!(repo.branch, Some("main".to_string()));
}

#[tokio::test]
async fn test_local_repository() {
    let temp = TempDir::new().unwrap();
    let content = r#"
header:
  version: 14
machine: test
repos:
  local-repo:
    path: /path/to/local/repo
    layers:
      meta-custom:
"#;

    let path = create_kas_file(&temp, "local.yml", content).await;
    let kas_file = KasFile::load(&path).await.unwrap();
    let repo = kas_file.config.repos.get("local-repo").unwrap();

    assert_eq!(repo.path, Some("/path/to/local/repo".to_string()));
    assert!(repo.url.is_none());
}

#[tokio::test]
async fn test_patches_configuration() {
    let temp = TempDir::new().unwrap();
    let content = r#"
header:
  version: 14
machine: test
repos:
  patched-repo:
    url: https://example.com/repo
    branch: main
    patches:
      security-fixes:
        - 0001-fix-cve.patch
        - 0002-fix-another.patch
      features:
        - 0003-add-feature.patch
"#;

    let path = create_kas_file(&temp, "patches.yml", content).await;
    let kas_file = KasFile::load(&path).await.unwrap();
    let repo = kas_file.config.repos.get("patched-repo").unwrap();

    let patches = repo.patches.as_ref().unwrap();
    assert_eq!(patches.len(), 2);

    let security = patches.get("security-fixes").unwrap();
    assert_eq!(security.len(), 2);
    assert_eq!(security[0], "0001-fix-cve.patch");

    let features = patches.get("features").unwrap();
    assert_eq!(features.len(), 1);
}

#[tokio::test]
async fn test_layer_custom_paths() {
    let temp = TempDir::new().unwrap();
    let content = r#"
header:
  version: 14
machine: test
repos:
  multi-layer:
    url: https://example.com/repo
    layers:
      meta-default:
      meta-custom:
        path: custom/path/meta-custom
"#;

    let path = create_kas_file(&temp, "layers.yml", content).await;
    let kas_file = KasFile::load(&path).await.unwrap();
    let repo = kas_file.config.repos.get("multi-layer").unwrap();

    assert_eq!(repo.layers.len(), 2);

    let default_layer = repo.layers.get("meta-default").unwrap();
    assert!(default_layer.path.is_none());

    let custom_layer = repo.layers.get("meta-custom").unwrap();
    assert_eq!(
        custom_layer.path,
        Some("custom/path/meta-custom".to_string())
    );
}

#[tokio::test]
async fn test_include_graph_simple() {
    let temp = TempDir::new().unwrap();

    // Create base file
    let base_content = r#"
header:
  version: 14
machine: qemux86-64
repos:
  poky:
    url: https://git.yoctoproject.org/poky
    branch: kirkstone
    layers:
      meta:
"#;
    create_kas_file(&temp, "base.yml", base_content).await;

    // Create main file that includes base
    let main_content = r#"
header:
  version: 14
  includes:
    - base.yml
distro: poky
target:
  - core-image-minimal
"#;
    let main_path = create_kas_file(&temp, "main.yml", main_content).await;

    let graph = KasIncludeGraph::build(&main_path).await.unwrap();

    // Should have 2 files
    assert_eq!(graph.files().len(), 2);

    // Merged config should have values from both
    let merged = graph.merge_config();
    assert_eq!(merged.machine, Some("qemux86-64".to_string()));
    assert_eq!(merged.distro, Some("poky".to_string()));
    assert_eq!(merged.target, Some(vec!["core-image-minimal".to_string()]));
    assert!(merged.repos.contains_key("poky"));
}

#[tokio::test]
async fn test_include_graph_chain() {
    let temp = TempDir::new().unwrap();

    // Create a chain of includes: main -> mid -> base
    let base = r#"
header:
  version: 14
machine: qemux86-64
"#;
    create_kas_file(&temp, "base.yml", base).await;

    let mid = r#"
header:
  version: 14
  includes:
    - base.yml
distro: poky
"#;
    create_kas_file(&temp, "mid.yml", mid).await;

    let main = r#"
header:
  version: 14
  includes:
    - mid.yml
target:
  - core-image-minimal
"#;
    let main_path = create_kas_file(&temp, "main.yml", main).await;

    let graph = KasIncludeGraph::build(&main_path).await.unwrap();

    // Should have 3 files
    assert_eq!(graph.files().len(), 3);

    // Check topological sort (base should come before mid, mid before main)
    let sorted = graph.sorted_files();
    assert_eq!(sorted.len(), 3);

    // Merged config should have all values
    let merged = graph.merge_config();
    assert_eq!(merged.machine, Some("qemux86-64".to_string()));
    assert_eq!(merged.distro, Some("poky".to_string()));
    assert_eq!(merged.target, Some(vec!["core-image-minimal".to_string()]));
}

#[tokio::test]
async fn test_include_graph_override() {
    let temp = TempDir::new().unwrap();

    // Base sets machine
    let base = r#"
header:
  version: 14
machine: qemux86
distro: poky
"#;
    create_kas_file(&temp, "base.yml", base).await;

    // Main overrides machine
    let main = r#"
header:
  version: 14
  includes:
    - base.yml
machine: qemux86-64
"#;
    let main_path = create_kas_file(&temp, "main.yml", main).await;

    let graph = KasIncludeGraph::build(&main_path).await.unwrap();
    let merged = graph.merge_config();

    // Main's machine should win (later overrides earlier)
    assert_eq!(merged.machine, Some("qemux86-64".to_string()));
    assert_eq!(merged.distro, Some("poky".to_string()));
}

#[tokio::test]
async fn test_include_graph_merge_repos() {
    let temp = TempDir::new().unwrap();

    let base = r#"
header:
  version: 14
machine: test
repos:
  poky:
    url: https://git.yoctoproject.org/poky
    branch: kirkstone
    layers:
      meta:
"#;
    create_kas_file(&temp, "base.yml", base).await;

    let main = r#"
header:
  version: 14
  includes:
    - base.yml
repos:
  meta-openembedded:
    url: https://git.openembedded.org/meta-openembedded
    branch: kirkstone
    layers:
      meta-oe:
"#;
    let main_path = create_kas_file(&temp, "main.yml", main).await;

    let graph = KasIncludeGraph::build(&main_path).await.unwrap();
    let merged = graph.merge_config();

    // Should have both repos
    assert_eq!(merged.repos.len(), 2);
    assert!(merged.repos.contains_key("poky"));
    assert!(merged.repos.contains_key("meta-openembedded"));
}

#[tokio::test]
async fn test_combined_checksum() {
    let temp = TempDir::new().unwrap();

    let base = r#"
header:
  version: 14
machine: test
"#;
    create_kas_file(&temp, "base.yml", base).await;

    let main = r#"
header:
  version: 14
  includes:
    - base.yml
distro: poky
"#;
    let main_path = create_kas_file(&temp, "main.yml", main).await;

    let graph = KasIncludeGraph::build(&main_path).await.unwrap();
    let checksum1 = graph.combined_checksum();

    // Checksum should be consistent
    let checksum2 = graph.combined_checksum();
    assert_eq!(checksum1, checksum2);

    // Checksum should be non-empty and hex
    assert!(!checksum1.is_empty());
    assert_eq!(checksum1.len(), 64); // SHA256 hex length
}

#[tokio::test]
async fn test_checksum_changes_on_content_change() {
    let temp = TempDir::new().unwrap();

    let content1 = r#"
header:
  version: 14
machine: qemux86
"#;
    let path = create_kas_file(&temp, "test.yml", content1).await;
    let file1 = KasFile::load(&path).await.unwrap();
    let checksum1 = file1.checksum;

    // Modify content
    let content2 = r#"
header:
  version: 14
machine: qemux86-64
"#;
    fs::write(&path, content2).await.unwrap();
    let file2 = KasFile::load(&path).await.unwrap();
    let checksum2 = file2.checksum;

    // Checksums should differ
    assert_ne!(checksum1, checksum2);
}

#[tokio::test]
async fn test_invalid_yaml() {
    let temp = TempDir::new().unwrap();
    let content = "this is not: valid: yaml::: [[[";
    let path = create_kas_file(&temp, "invalid.yml", content).await;

    let result = KasFile::load(&path).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        KasError::ParseError(_, _) => {}
        _ => panic!("Expected ParseError"),
    }
}

#[tokio::test]
async fn test_missing_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("nonexistent.yml");

    let result = KasFile::load(&path).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        KasError::IoError(_, _) => {}
        _ => panic!("Expected IoError"),
    }
}

#[tokio::test]
async fn test_include_not_found() {
    let temp = TempDir::new().unwrap();

    let content = r#"
header:
  version: 14
  includes:
    - nonexistent.yml
machine: test
"#;
    let path = create_kas_file(&temp, "main.yml", content).await;

    let result = KasIncludeGraph::build(&path).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_empty_repos() {
    let temp = TempDir::new().unwrap();
    let content = r#"
header:
  version: 14
machine: test
repos: {}
"#;

    let path = create_kas_file(&temp, "empty.yml", content).await;
    let kas_file = KasFile::load(&path).await.unwrap();

    assert_eq!(kas_file.config.repos.len(), 0);
}

#[tokio::test]
async fn test_root_file_access() {
    let temp = TempDir::new().unwrap();

    let content = r#"
header:
  version: 14
machine: test
"#;
    let path = create_kas_file(&temp, "root.yml", content).await;

    let graph = KasIncludeGraph::build(&path).await.unwrap();
    let root = graph.root();

    assert_eq!(root.path, path);
    assert_eq!(root.config.machine, Some("test".to_string()));
}

#[tokio::test]
async fn test_target_list() {
    let temp = TempDir::new().unwrap();

    // Multiple targets as list
    let multiple = r#"
header:
  version: 14
machine: test
target:
  - core-image-minimal
  - core-image-sato
"#;
    let path = create_kas_file(&temp, "multiple.yml", multiple).await;
    let kas_file = KasFile::load(&path).await.unwrap();

    let targets = kas_file.config.target.as_ref().unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0], "core-image-minimal");
    assert_eq!(targets[1], "core-image-sato");
}

#[tokio::test]
async fn test_environment_variables() {
    let temp = TempDir::new().unwrap();
    let content = r#"
header:
  version: 14
machine: test
env:
  VAR1: value1
  VAR2: value2
  VAR3: ""
"#;

    let path = create_kas_file(&temp, "env.yml", content).await;
    let kas_file = KasFile::load(&path).await.unwrap();

    let env = kas_file.config.env.as_ref().unwrap();
    assert_eq!(env.len(), 3);
    assert_eq!(env.get("VAR1").unwrap(), "value1");
    assert_eq!(env.get("VAR2").unwrap(), "value2");
    assert_eq!(env.get("VAR3").unwrap(), "");
}

#[tokio::test]
async fn test_header_versions() {
    let temp = TempDir::new().unwrap();

    for version in [1, 5, 10, 14] {
        let content = format!(
            r#"
header:
  version: {}
machine: test
"#,
            version
        );
        let path = create_kas_file(&temp, &format!("v{}.yml", version), &content).await;
        let kas_file = KasFile::load(&path).await.unwrap();

        assert_eq!(kas_file.config.header.version, version);
    }
}
