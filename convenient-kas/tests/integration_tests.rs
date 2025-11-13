//! Additional edge case and integration tests to reach 60% coverage

use convenient_kas::config_generator::{ConfigError, ConfigGenerator};
use convenient_kas::include_graph::{KasConfig, KasError, KasFile, KasHeader, KasIncludeGraph};
use std::collections::HashMap;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test_include_graph_multiple_includes() {
    let temp = TempDir::new().unwrap();

    // Create multiple base files
    let base1 = r#"
header:
  version: 14
machine: qemux86
"#;
    fs::write(temp.path().join("base1.yml"), base1)
        .await
        .unwrap();

    let base2 = r#"
header:
  version: 14
distro: poky
"#;
    fs::write(temp.path().join("base2.yml"), base2)
        .await
        .unwrap();

    // Main file includes both
    let main = r#"
header:
  version: 14
  includes:
    - base1.yml
    - base2.yml
target:
  - core-image-minimal
"#;
    let main_path = temp.path().join("main.yml");
    fs::write(&main_path, main).await.unwrap();

    let graph = KasIncludeGraph::build(&main_path).await.unwrap();
    let merged = graph.merge_config();

    // All values should be present
    assert_eq!(merged.machine, Some("qemux86".to_string()));
    assert_eq!(merged.distro, Some("poky".to_string()));
    assert!(merged.target.is_some());
}

#[tokio::test]
async fn test_include_graph_deep_nesting() {
    let temp = TempDir::new().unwrap();

    // Create deeply nested includes
    let level3 = r#"
header:
  version: 14
machine: qemux86-64
"#;
    fs::write(temp.path().join("level3.yml"), level3)
        .await
        .unwrap();

    let level2 = r#"
header:
  version: 14
  includes:
    - level3.yml
distro: poky
"#;
    fs::write(temp.path().join("level2.yml"), level2)
        .await
        .unwrap();

    let level1 = r#"
header:
  version: 14
  includes:
    - level2.yml
target:
  - core-image-minimal
"#;
    let level1_path = temp.path().join("level1.yml");
    fs::write(&level1_path, level1).await.unwrap();

    let graph = KasIncludeGraph::build(&level1_path).await.unwrap();
    assert_eq!(graph.files().len(), 3);

    let merged = graph.merge_config();
    assert_eq!(merged.machine, Some("qemux86-64".to_string()));
    assert_eq!(merged.distro, Some("poky".to_string()));
    assert!(merged.target.is_some());
}

#[tokio::test]
async fn test_kas_file_includes_helper() {
    let temp = TempDir::new().unwrap();

    let content = r#"
header:
  version: 14
  includes:
    - base.yml
    - configs/debug.yml
machine: test
"#;
    let path = temp.path().join("test.yml");
    fs::write(&path, content).await.unwrap();

    let kas_file = KasFile::load(&path).await.unwrap();
    let includes = kas_file.includes();

    assert_eq!(includes.len(), 2);
    assert_eq!(
        includes[0],
        temp.path().join("base.yml")
    );
    assert_eq!(
        includes[1],
        temp.path().join("configs/debug.yml")
    );
}

#[tokio::test]
async fn test_empty_includes() {
    let temp = TempDir::new().unwrap();

    let content = r#"
header:
  version: 14
  includes: []
machine: test
"#;
    let path = temp.path().join("test.yml");
    fs::write(&path, content).await.unwrap();

    let kas_file = KasFile::load(&path).await.unwrap();
    let includes = kas_file.includes();

    assert_eq!(includes.len(), 0);
}

#[tokio::test]
async fn test_config_with_all_optional_fields_none() {
    let temp = TempDir::new().unwrap();

    let content = r#"
header:
  version: 14
machine: test
"#;
    let path = temp.path().join("minimal.yml");
    fs::write(&path, content).await.unwrap();

    let kas_file = KasFile::load(&path).await.unwrap();

    assert!(kas_file.config.distro.is_none());
    assert!(kas_file.config.target.is_none());
    assert!(kas_file.config.env.is_none());
    assert!(kas_file.config.local_conf_header.is_none());
    assert!(kas_file.config.bblayers_conf_header.is_none());
    assert!(kas_file.config.header.includes.is_none());
}

#[tokio::test]
async fn test_repo_with_all_ref_types() {
    let temp = TempDir::new().unwrap();

    let content = r#"
header:
  version: 14
machine: test
repos:
  repo1:
    url: https://example.com/repo1
    commit: abc123def456
  repo2:
    url: https://example.com/repo2
    tag: v1.0.0
  repo3:
    url: https://example.com/repo3
    branch: kirkstone
  repo4:
    url: https://example.com/repo4
    refspec: refs/heads/custom
"#;
    let path = temp.path().join("repos.yml");
    fs::write(&path, content).await.unwrap();

    let kas_file = KasFile::load(&path).await.unwrap();

    assert_eq!(kas_file.config.repos.len(), 4);
    assert!(kas_file.config.repos.get("repo1").unwrap().commit.is_some());
    assert!(kas_file.config.repos.get("repo2").unwrap().tag.is_some());
    assert!(kas_file.config.repos.get("repo3").unwrap().branch.is_some());
    assert!(kas_file.config.repos.get("repo4").unwrap().refspec.is_some());
}

#[tokio::test]
async fn test_merged_config_repos_merge() {
    let temp = TempDir::new().unwrap();

    let base = r#"
header:
  version: 14
machine: test
repos:
  poky:
    url: https://git.yoctoproject.org/poky
    branch: kirkstone
"#;
    fs::write(temp.path().join("base.yml"), base)
        .await
        .unwrap();

    let overlay = r#"
header:
  version: 14
  includes:
    - base.yml
repos:
  meta-oe:
    url: https://git.openembedded.org/meta-openembedded
    branch: kirkstone
"#;
    let overlay_path = temp.path().join("overlay.yml");
    fs::write(&overlay_path, overlay).await.unwrap();

    let graph = KasIncludeGraph::build(&overlay_path).await.unwrap();
    let merged = graph.merge_config();

    // Both repos should be present
    assert_eq!(merged.repos.len(), 2);
    assert!(merged.repos.contains_key("poky"));
    assert!(merged.repos.contains_key("meta-oe"));
}

#[tokio::test]
async fn test_merged_config_env_merge() {
    let temp = TempDir::new().unwrap();

    let base = r#"
header:
  version: 14
machine: test
env:
  VAR1: value1
  VAR2: value2
"#;
    fs::write(temp.path().join("base.yml"), base)
        .await
        .unwrap();

    let overlay = r#"
header:
  version: 14
  includes:
    - base.yml
env:
  VAR2: overridden
  VAR3: value3
"#;
    let overlay_path = temp.path().join("overlay.yml");
    fs::write(&overlay_path, overlay).await.unwrap();

    let graph = KasIncludeGraph::build(&overlay_path).await.unwrap();
    let merged = graph.merge_config();

    let env = merged.env.as_ref().unwrap();
    assert_eq!(env.len(), 3);
    assert_eq!(env.get("VAR1").unwrap(), "value1");
    assert_eq!(env.get("VAR2").unwrap(), "overridden"); // Overlay wins
    assert_eq!(env.get("VAR3").unwrap(), "value3");
}

#[tokio::test]
async fn test_config_generator_without_machine() {
    let temp = TempDir::new().unwrap();

    let config = KasConfig {
        header: KasHeader {
            version: 14,
            includes: None,
        },
        machine: None, // No machine
        distro: Some("poky".to_string()),
        target: None,
        repos: HashMap::new(),
        bblayers_conf_header: None,
        local_conf_header: None,
        env: None,
    };

    let generator = ConfigGenerator::new(temp.path(), config, HashMap::new());
    generator.generate_all().await.unwrap();

    let local_conf = fs::read_to_string(temp.path().join("conf/local.conf"))
        .await
        .unwrap();

    // Should still generate, just without MACHINE line
    assert!(local_conf.contains("local.conf"));
    assert!(local_conf.contains("poky"));
}

#[tokio::test]
async fn test_config_generator_empty_layer_paths() {
    let temp = TempDir::new().unwrap();

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

    let generator = ConfigGenerator::new(temp.path(), config, HashMap::new());
    generator.generate_all().await.unwrap();

    let bblayers = fs::read_to_string(temp.path().join("conf/bblayers.conf"))
        .await
        .unwrap();

    // Should have empty BBLAYERS
    assert!(bblayers.contains("BBLAYERS ?= \" \\"));
}

#[tokio::test]
async fn test_combined_checksum_stability() {
    let temp = TempDir::new().unwrap();

    let content = r#"
header:
  version: 14
machine: test
"#;
    let path = temp.path().join("test.yml");
    fs::write(&path, content).await.unwrap();

    let graph1 = KasIncludeGraph::build(&path).await.unwrap();
    let checksum1 = graph1.combined_checksum();

    // Load again - checksum should be identical
    let graph2 = KasIncludeGraph::build(&path).await.unwrap();
    let checksum2 = graph2.combined_checksum();

    assert_eq!(checksum1, checksum2);
}

#[tokio::test]
async fn test_files_method() {
    let temp = TempDir::new().unwrap();

    let base = r#"
header:
  version: 14
machine: test
"#;
    fs::write(temp.path().join("base.yml"), base)
        .await
        .unwrap();

    let main = r#"
header:
  version: 14
  includes:
    - base.yml
distro: poky
"#;
    let main_path = temp.path().join("main.yml");
    fs::write(&main_path, main).await.unwrap();

    let graph = KasIncludeGraph::build(&main_path).await.unwrap();
    let files = graph.files();

    assert_eq!(files.len(), 2);
    assert!(files.contains_key(&main_path));
    assert!(files.contains_key(&temp.path().join("base.yml")));
}

#[tokio::test]
async fn test_busybox_kas_example() {
    // Realistic busybox kas configuration test
    let temp = TempDir::new().unwrap();
    let kas_path = temp.path().join("busybox.yml");

    let content = r#"
header:
  version: 14

machine: qemux86-64
distro: poky

target:
  - busybox

repos:
  poky:
    url: https://git.yoctoproject.org/poky
    branch: kirkstone
    layers:
      meta:
      meta-poky:
      meta-yocto-bsp:

env:
  SSTATE_DIR: /shared/sstate-cache
  DL_DIR: /shared/downloads
  BB_NUMBER_THREADS: "8"
  PARALLEL_MAKE: "-j 8"

local_conf_header:
  build-settings: |
    PACKAGE_CLASSES = "package_rpm"
    INHERIT += "rm_work"

bblayers_conf_header:
  layer-settings: |
    BBMASK = ""
"#;

    fs::write(&kas_path, content).await.unwrap();

    // Test loading
    let kas_file = KasFile::load(&kas_path).await.unwrap();
    assert_eq!(kas_file.config.header.version, 14);
    assert_eq!(kas_file.config.machine, Some("qemux86-64".to_string()));
    assert_eq!(kas_file.config.distro, Some("poky".to_string()));
    assert_eq!(kas_file.config.target, Some(vec!["busybox".to_string()]));

    // Test repos
    assert_eq!(kas_file.config.repos.len(), 1);
    let poky = kas_file.config.repos.get("poky").unwrap();
    assert_eq!(poky.url, Some("https://git.yoctoproject.org/poky".to_string()));
    assert_eq!(poky.branch, Some("kirkstone".to_string()));
    assert_eq!(poky.layers.len(), 3);

    // Test environment
    let env = kas_file.config.env.as_ref().unwrap();
    assert_eq!(env.get("SSTATE_DIR").unwrap(), "/shared/sstate-cache");
    assert_eq!(env.get("DL_DIR").unwrap(), "/shared/downloads");
    assert_eq!(env.get("BB_NUMBER_THREADS").unwrap(), "8");

    // Test include graph
    let graph = KasIncludeGraph::build(&kas_path).await.unwrap();
    assert_eq!(graph.files().len(), 1);

    // Test checksum
    let checksum = graph.combined_checksum();
    assert_eq!(checksum.len(), 64); // SHA256 hex string
}
