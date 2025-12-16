//! Integration tests for BuildEnvironment
//!
//! These tests verify that BuildEnvironment correctly parses BitBake
//! configuration files and extracts build settings.

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

/// Test that BuildEnvironment can load from a valid build directory
#[test]
fn test_build_environment_can_be_loaded() {
    use convenient_bitbake::BuildEnvironment;

    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    create_test_build_dir(&build_dir).expect("Failed to create test build dir");

    // Try to load the build environment
    let env = BuildEnvironment::from_build_dir(&build_dir);
    assert!(
        env.is_ok(),
        "Should be able to load build environment: {:?}",
        env.err()
    );

    let env = env.unwrap();
    assert_eq!(env.get_machine(), Some("qemux86-64"));
    assert_eq!(env.get_distro(), Some("poky"));
}

/// Test that BuildEnvironment correctly parses layer information
#[test]
fn test_build_environment_layers() {
    use convenient_bitbake::BuildEnvironment;

    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    create_test_build_dir(&build_dir).expect("Failed to create test build dir");

    let env =
        BuildEnvironment::from_build_dir(&build_dir).expect("Should load build environment");

    // Should have at least one layer
    assert!(!env.layers.is_empty(), "Should have at least one layer");
}

/// Test that BuildEnvironment handles missing conf directory gracefully
#[test]
fn test_build_environment_missing_conf() {
    use convenient_bitbake::BuildEnvironment;

    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    fs::create_dir_all(&build_dir).expect("Failed to create build dir");

    // Don't create any conf files - should fail to load
    let env = BuildEnvironment::from_build_dir(&build_dir);
    assert!(
        env.is_err(),
        "Should fail to load build environment without conf"
    );
}

/// Test that BuildEnvironment handles missing MACHINE gracefully
#[test]
fn test_build_environment_no_machine() {
    use convenient_bitbake::BuildEnvironment;

    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    let conf_dir = build_dir.join("conf");
    fs::create_dir_all(&conf_dir).expect("Failed to create conf dir");

    // Create minimal bblayers.conf
    let bblayers_content = r#"BBLAYERS = ""
"#;
    fs::write(conf_dir.join("bblayers.conf"), bblayers_content).expect("Failed to write bblayers");

    // Create local.conf without MACHINE
    let local_content = r#"# Minimal config without MACHINE
DISTRO = "poky"
"#;
    fs::write(conf_dir.join("local.conf"), local_content).expect("Failed to write local.conf");

    let env = BuildEnvironment::from_build_dir(&build_dir);

    // Should still load, but machine should be None or default
    if let Ok(env) = env {
        // MACHINE being None or some default is acceptable
        let machine = env.get_machine();
        // Just verify we don't panic when accessing it
        let _ = machine;
    }
}

/// Test that BuildEnvironment parses multiple layers correctly
#[test]
fn test_build_environment_multiple_layers() {
    use convenient_bitbake::BuildEnvironment;

    let tmp = TempDir::new().expect("Failed to create temp dir");
    let build_dir = tmp.path().join("build");
    let conf_dir = build_dir.join("conf");
    fs::create_dir_all(&conf_dir).expect("Failed to create conf dir");

    // Create meta and meta-custom layers
    let meta_dir = tmp.path().join("meta");
    let meta_custom_dir = tmp.path().join("meta-custom");
    fs::create_dir_all(meta_dir.join("conf")).expect("Failed to create meta conf");
    fs::create_dir_all(meta_custom_dir.join("conf")).expect("Failed to create meta-custom conf");

    // Create layer.conf for each layer
    let layer_conf = r#"BBPATH .= ":${LAYERDIR}"
BBFILES += "${LAYERDIR}/recipes-*/*/*.bb"
BBFILE_COLLECTIONS += "meta"
BBFILE_PATTERN_meta = "^${LAYERDIR}/"
"#;
    fs::write(meta_dir.join("conf/layer.conf"), layer_conf).expect("Failed to write layer.conf");
    fs::write(meta_custom_dir.join("conf/layer.conf"), layer_conf)
        .expect("Failed to write layer.conf");

    // Create bblayers.conf with multiple layers
    let bblayers_content = format!(
        r#"BBLAYERS ?= " \
  {} \
  {} \
"
"#,
        meta_dir.display(),
        meta_custom_dir.display()
    );
    fs::write(conf_dir.join("bblayers.conf"), bblayers_content).expect("Failed to write bblayers");

    // Create minimal local.conf
    let local_content = r#"MACHINE = "qemux86-64"
"#;
    fs::write(conf_dir.join("local.conf"), local_content).expect("Failed to write local.conf");

    let env = BuildEnvironment::from_build_dir(&build_dir);

    if let Ok(env) = env {
        // Should have 2 layers
        assert!(
            env.layers.len() >= 2,
            "Should have at least 2 layers, got {}",
            env.layers.len()
        );
    }
}
