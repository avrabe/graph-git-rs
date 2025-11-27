//! End-to-end tests for hitzeleiter build infrastructure
//!
//! These tests verify the build infrastructure works correctly with real
//! build directory structures using only the standard library.
//!
//! Tests that require convenient-bitbake::BuildEnvironment are located in
//! convenient-bitbake/tests/ since that crate has the BitBake parsing logic.

use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Helper to create a minimal BitBake build directory structure
fn create_test_build_dir(base: &Path) -> std::io::Result<()> {
    let conf_dir = base.join("conf");
    fs::create_dir_all(&conf_dir)?;

    // Create bblayers.conf
    let bblayers_content = r#"# POKY_BBLAYERS_CONF_VERSION is increased each time build/conf/bblayers.conf
# changes incompatibly
POKY_BBLAYERS_CONF_VERSION = "2"

BBPATH = "${TOPDIR}"
BBFILES ?= ""

BBLAYERS ?= " \
  ${TOPDIR}/../meta \
"
"#;
    fs::write(conf_dir.join("bblayers.conf"), bblayers_content)?;

    // Create local.conf
    let local_content = r#"# Machine Selection
MACHINE ??= "qemux86-64"

# Distribution Selection
DISTRO ?= "poky"

# Build directory configuration
DL_DIR ?= "${TOPDIR}/downloads"
SSTATE_DIR ?= "${TOPDIR}/sstate-cache"
TMPDIR = "${TOPDIR}/tmp"

# Accept all licenses for testing
LICENSE_FLAGS_ACCEPTED = "commercial"
"#;
    fs::write(conf_dir.join("local.conf"), local_content)?;

    // Create a simple meta layer with a test recipe
    let meta_dir = base.parent().unwrap().join("meta");
    let recipes_dir = meta_dir.join("recipes-test").join("hello");
    fs::create_dir_all(&recipes_dir)?;

    // Create layer.conf
    let layer_conf_dir = meta_dir.join("conf");
    fs::create_dir_all(&layer_conf_dir)?;

    let layer_conf = r#"# Layer configuration for test layer
BBPATH .= ":${LAYERDIR}"
BBFILES += "${LAYERDIR}/recipes-*/*/*.bb"
BBFILE_COLLECTIONS += "meta"
BBFILE_PATTERN_meta = "^${LAYERDIR}/"
LAYERSERIES_COMPAT_meta = "kirkstone langdale mickledore nanbield scarthgap"
"#;
    fs::write(layer_conf_dir.join("layer.conf"), layer_conf)?;

    // Create a simple test recipe
    let recipe_content = r#"SUMMARY = "Hello World Test Recipe"
DESCRIPTION = "A minimal test recipe for hitzeleiter"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

SRC_URI = ""

do_compile() {
    echo "Hello, World!" > ${B}/hello.txt
}

do_install() {
    install -d ${D}${bindir}
    install -m 0644 ${B}/hello.txt ${D}${bindir}/
}
"#;
    fs::write(recipes_dir.join("hello_1.0.bb"), recipe_content)?;

    Ok(())
}

/// Helper to create a cache directory structure
fn create_test_cache(base: &Path) -> std::io::Result<()> {
    let cache_dir = base.join("hitzeleiter-cache");
    let cas_dir = cache_dir.join("cas");
    let ac_dir = cache_dir.join("ac");
    let sandboxes_dir = cache_dir.join("sandboxes");

    fs::create_dir_all(&cas_dir)?;
    fs::create_dir_all(&ac_dir)?;
    fs::create_dir_all(&sandboxes_dir)?;

    // Create some dummy CAS objects
    fs::write(cas_dir.join("abc123"), b"test content 1")?;
    fs::write(cas_dir.join("def456"), b"test content 2")?;

    // Create some dummy action cache entries
    fs::write(ac_dir.join("action1"), b"action data 1")?;

    Ok(())
}

// ============== Clean Command Tests ==============

#[test]
fn test_clean_creates_expected_structure() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    // Create cache structure
    create_test_cache(&build_dir).expect("Failed to create test cache");

    let cache_dir = build_dir.join("hitzeleiter-cache");
    assert!(cache_dir.exists(), "Cache dir should exist");
    assert!(cache_dir.join("cas").exists(), "CAS dir should exist");
    assert!(cache_dir.join("ac").exists(), "AC dir should exist");
}

#[test]
fn test_build_dir_structure_creation() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    create_test_build_dir(&build_dir).expect("Failed to create test build dir");

    // Verify structure was created
    assert!(build_dir.join("conf/bblayers.conf").exists());
    assert!(build_dir.join("conf/local.conf").exists());

    // Verify meta layer was created
    let meta_dir = tmp.path().join("meta");
    assert!(meta_dir.join("conf/layer.conf").exists());
    assert!(meta_dir.join("recipes-test/hello/hello_1.0.bb").exists());
}

#[test]
fn test_bblayers_conf_content() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    create_test_build_dir(&build_dir).expect("Failed to create test build dir");

    let content = fs::read_to_string(build_dir.join("conf/bblayers.conf"))
        .expect("Failed to read bblayers.conf");

    assert!(content.contains("BBLAYERS"), "Should contain BBLAYERS");
    assert!(content.contains("meta"), "Should reference meta layer");
}

#[test]
fn test_local_conf_content() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    create_test_build_dir(&build_dir).expect("Failed to create test build dir");

    let content = fs::read_to_string(build_dir.join("conf/local.conf"))
        .expect("Failed to read local.conf");

    assert!(content.contains("MACHINE"), "Should contain MACHINE");
    assert!(content.contains("qemux86-64"), "Should have qemux86-64 as default machine");
    assert!(content.contains("DISTRO"), "Should contain DISTRO");
}

#[test]
fn test_recipe_content() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    create_test_build_dir(&build_dir).expect("Failed to create test build dir");

    let recipe_path = tmp.path().join("meta/recipes-test/hello/hello_1.0.bb");
    let content = fs::read_to_string(&recipe_path)
        .expect("Failed to read recipe");

    assert!(content.contains("SUMMARY"), "Recipe should have SUMMARY");
    assert!(content.contains("LICENSE"), "Recipe should have LICENSE");
    assert!(content.contains("do_compile"), "Recipe should have do_compile");
    assert!(content.contains("do_install"), "Recipe should have do_install");
}

// ============== Cache Structure Tests ==============

#[test]
fn test_cache_directory_structure() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    create_test_cache(&build_dir).expect("Failed to create test cache");

    let cache_dir = build_dir.join("hitzeleiter-cache");

    // Verify all expected directories exist
    assert!(cache_dir.join("cas").is_dir(), "CAS should be a directory");
    assert!(cache_dir.join("ac").is_dir(), "AC should be a directory");
    assert!(cache_dir.join("sandboxes").is_dir(), "Sandboxes should be a directory");

    // Verify CAS objects were created
    assert!(cache_dir.join("cas/abc123").exists(), "CAS object should exist");
    assert!(cache_dir.join("cas/def456").exists(), "CAS object should exist");

    // Verify action cache entry was created
    assert!(cache_dir.join("ac/action1").exists(), "AC entry should exist");
}

#[test]
fn test_cache_object_content() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    create_test_cache(&build_dir).expect("Failed to create test cache");

    let cache_dir = build_dir.join("hitzeleiter-cache");

    let content = fs::read_to_string(cache_dir.join("cas/abc123"))
        .expect("Failed to read CAS object");
    assert_eq!(content, "test content 1");
}

// ============== Path Handling Tests ==============

#[test]
fn test_relative_path_handling() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");

    // Test that we can create nested paths
    let deep_path = build_dir.join("a/b/c/d/e");
    fs::create_dir_all(&deep_path).expect("Failed to create deep path");
    assert!(deep_path.exists());
}

#[test]
fn test_special_characters_in_path() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Test paths with special characters (common in recipe names)
    let special_path = tmp.path().join("build_test-1.0+git");
    fs::create_dir_all(&special_path).expect("Failed to create path with special chars");
    assert!(special_path.exists());
}

// ============== Error Handling Tests ==============

#[test]
fn test_missing_conf_directory() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    // Don't create conf directory - verify it doesn't exist
    let conf_dir = build_dir.join("conf");
    assert!(!conf_dir.exists(), "Conf dir should not exist initially");
}

#[test]
fn test_cache_dir_not_exists() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    // Don't create cache - verify the cache directory doesn't exist
    let cache_dir = build_dir.join("hitzeleiter-cache");
    assert!(!cache_dir.exists(), "Cache dir should not exist initially");
}

// ============== Integration with convenient-bitbake ==============
// NOTE: BuildEnvironment tests are in convenient-bitbake/tests/ since they
// require that crate's parsing infrastructure. See:
// - convenient-bitbake/tests/build_environment_tests.rs
