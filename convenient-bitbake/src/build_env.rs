//! Build environment abstraction for BitBake
//!
//! Represents a complete BitBake build environment loaded from a build directory
//! containing conf/bblayers.conf and conf/local.conf.

use crate::{BbLayersConfig, BuildContext, LayerConfig, LocalConfig, VariableExpander};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Represents a BitBake build environment
#[derive(Debug)]
pub struct BuildEnvironment {
    /// Build directory (TOPDIR)
    pub topdir: PathBuf,

    /// OE-Core root (OEROOT) - detected from layers
    pub oeroot: Option<PathBuf>,

    /// Downloads directory (DL_DIR)
    pub dl_dir: PathBuf,

    /// Build output directory (TMPDIR)
    pub tmpdir: PathBuf,

    /// Shared state cache (SSTATE_DIR)
    pub sstate_dir: PathBuf,

    /// Configuration directory
    pub confdir: PathBuf,

    /// All layers with expanded paths
    pub layers: Vec<PathBuf>,

    /// Build configuration from local.conf
    pub local_config: LocalConfig,

    /// Layer configuration from bblayers.conf
    pub bblayers_config: BbLayersConfig,

    /// Variable expander with all standard variables
    expander: VariableExpander,
}

impl BuildEnvironment {
    /// Create a BuildEnvironment from a build directory
    ///
    /// # Arguments
    ///
    /// * `topdir` - Path to build directory (must contain conf/bblayers.conf and conf/local.conf)
    ///
    /// # Returns
    ///
    /// A fully initialized BuildEnvironment with expanded paths
    pub fn from_build_dir<P: AsRef<Path>>(topdir: P) -> Result<Self, String> {
        let topdir = topdir.as_ref().canonicalize()
            .map_err(|e| format!("Failed to canonicalize TOPDIR: {}", e))?;

        info!("Loading build environment from: {:?}", topdir);

        // Check that topdir exists and is a directory
        if !topdir.is_dir() {
            return Err(format!("TOPDIR is not a directory: {:?}", topdir));
        }

        let confdir = topdir.join("conf");
        if !confdir.is_dir() {
            return Err(format!("conf/ directory not found in {:?}", topdir));
        }

        // Parse configuration files
        let bblayers_conf = confdir.join("bblayers.conf");
        if !bblayers_conf.exists() {
            return Err(format!(
                "bblayers.conf not found: {:?}",
                bblayers_conf
            ));
        }

        let local_conf = confdir.join("local.conf");
        if !local_conf.exists() {
            return Err(format!("local.conf not found: {:?}", local_conf));
        }

        let bblayers_config = BbLayersConfig::parse(&bblayers_conf)?;
        let local_config = LocalConfig::parse(&local_conf)?;

        // Create variable expander with standard variables
        let mut expander = VariableExpander::new();
        expander.set("TOPDIR".to_string(), topdir.to_string_lossy().to_string());

        // Add all variables from local.conf to expander
        for (key, value) in &local_config.variables {
            expander.set(key.clone(), value.clone());
        }

        // Expand layer paths
        let layers = expander.expand_paths(&bblayers_config.bblayers);

        debug!("Expanded {} layer paths", layers.len());

        // Detect OEROOT from layers (look for meta/conf/bitbake.conf)
        let oeroot = Self::detect_oeroot(&layers);
        if let Some(ref oeroot_path) = oeroot {
            expander.set("OEROOT".to_string(), oeroot_path.to_string_lossy().to_string());
            expander.set("COREBASE".to_string(), oeroot_path.to_string_lossy().to_string());
            debug!("Detected OEROOT: {:?}", oeroot_path);
        }

        // Set LAYERDIR for each layer (will be set per-layer during parsing)
        // For now we just track that we need to do this

        // Determine directories with expansion
        let dl_dir = if let Some(ref dl_dir) = local_config.dl_dir {
            expander.expand_path(dl_dir)
        } else {
            topdir.join("downloads")
        };

        let tmpdir = if let Some(ref tmpdir) = local_config.tmpdir {
            expander.expand_path(tmpdir)
        } else {
            topdir.join("tmp")
        };

        let sstate_dir = if let Some(ref sstate_dir) = local_config.sstate_dir {
            expander.expand_path(sstate_dir)
        } else {
            topdir.join("sstate-cache")
        };

        info!("Build environment initialized:");
        info!("  TOPDIR:     {:?}", topdir);
        info!("  DL_DIR:     {:?}", dl_dir);
        info!("  TMPDIR:     {:?}", tmpdir);
        info!("  SSTATE_DIR: {:?}", sstate_dir);
        info!("  Layers:     {} configured", layers.len());

        Ok(BuildEnvironment {
            topdir,
            oeroot,
            dl_dir,
            tmpdir,
            sstate_dir,
            confdir,
            layers,
            local_config,
            bblayers_config,
            expander,
        })
    }

    /// Detect OEROOT by looking for openembedded-core layer
    fn detect_oeroot(layers: &[PathBuf]) -> Option<PathBuf> {
        for layer in layers {
            // Check if this layer is the "meta" layer from openembedded-core
            // It should contain conf/bitbake.conf
            let bitbake_conf = layer.join("conf/bitbake.conf");
            if bitbake_conf.exists() {
                // OEROOT is the parent of meta/
                if let Some(parent) = layer.parent() {
                    return Some(parent.to_path_buf());
                }
            }
        }
        None
    }

    /// Expand a variable reference
    pub fn expand(&self, input: &str) -> String {
        self.expander.expand(input)
    }

    /// Expand a path
    pub fn expand_path(&self, path: &Path) -> PathBuf {
        self.expander.expand_path(path)
    }

    /// Get all layers with absolute paths
    pub fn get_layers(&self) -> &[PathBuf] {
        &self.layers
    }

    /// Create a BuildContext from this environment
    ///
    /// This loads all layer.conf files and sets up the build context
    /// with proper priorities and configuration.
    pub fn create_build_context(&self) -> Result<BuildContext, String> {
        info!("Creating BuildContext from environment");

        let mut build_context = BuildContext::new();

        // Set MACHINE and DISTRO from local.conf
        if let Some(ref machine) = self.local_config.machine {
            build_context.set_machine(machine.clone());
        }
        if let Some(ref distro) = self.local_config.distro {
            build_context.set_distro(distro.clone());
        }

        // Add all variables from local.conf to global variables
        for (key, value) in &self.local_config.variables {
            build_context
                .global_variables
                .insert(key.clone(), self.expander.expand(value));
        }

        // Add all layers
        for layer_path in &self.layers {
            let layer_conf = layer_path.join("conf/layer.conf");
            if layer_conf.exists() {
                match build_context.add_layer_from_conf(&layer_conf) {
                    Ok(()) => {
                        debug!("Loaded layer: {:?}", layer_path);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse layer.conf for {:?}: {}",
                            layer_path,
                            e
                        );
                        // Create a default layer config
                        let default_layer = LayerConfig {
                            layer_dir: layer_path.clone(),
                            collection: layer_path
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            priority: 5, // Default priority
                            version: None,
                            depends: vec![],
                            series_compat: vec![],
                            variables: HashMap::new(),
                        };
                        build_context.add_layer(default_layer);
                    }
                }
            } else {
                tracing::warn!(
                    "Layer missing layer.conf: {:?}",
                    layer_path
                );
            }
        }

        // Verify layer dependencies
        build_context.verify_dependencies()?;

        info!(
            "BuildContext created with {} layers",
            build_context.layers.len()
        );

        Ok(build_context)
    }

    /// Get MACHINE setting
    pub fn get_machine(&self) -> Option<&str> {
        self.local_config.machine.as_deref()
    }

    /// Get DISTRO setting
    pub fn get_distro(&self) -> Option<&str> {
        self.local_config.distro.as_deref()
    }

    /// Get a variable value
    pub fn get_var(&self, key: &str) -> Option<String> {
        // Check local_config variables first
        if let Some(value) = self.local_config.variables.get(key) {
            return Some(self.expander.expand(value));
        }

        // Check expander
        self.expander.get(key).map(|s| s.clone())
    }

    /// Set a variable value
    pub fn set_var(&mut self, key: String, value: String) {
        self.expander.set(key.clone(), value.clone());
        self.local_config.variables.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_build_env(temp_dir: &Path) -> PathBuf {
        let build_dir = temp_dir.join("build");
        let conf_dir = build_dir.join("conf");
        fs::create_dir_all(&conf_dir).unwrap();

        // Create bblayers.conf
        let bblayers_conf = conf_dir.join("bblayers.conf");
        fs::write(
            &bblayers_conf,
            r#"
POKY_BBLAYERS_CONF_VERSION = "2"
BBPATH = "${TOPDIR}"
BBFILES ?= ""

BBLAYERS ?= " \
  /path/to/meta \
  /path/to/meta-poky \
  "
"#,
        )
        .unwrap();

        // Create local.conf
        let local_conf = conf_dir.join("local.conf");
        fs::write(
            &local_conf,
            r#"
MACHINE ??= "qemux86-64"
DISTRO ?= "poky"
DL_DIR ?= "${TOPDIR}/downloads"
TMPDIR = "${TOPDIR}/tmp"
SSTATE_DIR ?= "${TOPDIR}/sstate-cache"
"#,
        )
        .unwrap();

        build_dir
    }

    #[test]
    fn test_build_environment_from_build_dir() {
        let temp_dir = TempDir::new().unwrap();
        let build_dir = create_test_build_env(temp_dir.path());

        let env = BuildEnvironment::from_build_dir(&build_dir).unwrap();

        assert_eq!(env.layers.len(), 2);
        assert_eq!(env.get_machine(), Some("qemux86-64"));
        assert_eq!(env.get_distro(), Some("poky"));
        assert!(env.dl_dir.ends_with("downloads"));
        assert!(env.tmpdir.ends_with("tmp"));
    }

    #[test]
    fn test_variable_expansion_in_paths() {
        let temp_dir = TempDir::new().unwrap();
        let build_dir = create_test_build_env(temp_dir.path());

        let env = BuildEnvironment::from_build_dir(&build_dir).unwrap();

        // DL_DIR should have ${TOPDIR} expanded
        let topdir_str = env.topdir.to_string_lossy();
        assert!(env.dl_dir.to_string_lossy().contains(&*topdir_str));
    }

    #[test]
    fn test_get_and_set_var() {
        let temp_dir = TempDir::new().unwrap();
        let build_dir = create_test_build_env(temp_dir.path());

        let mut env = BuildEnvironment::from_build_dir(&build_dir).unwrap();

        assert_eq!(env.get_var("MACHINE"), Some("qemux86-64".to_string()));

        env.set_var("MY_VAR".to_string(), "my_value".to_string());
        assert_eq!(env.get_var("MY_VAR"), Some("my_value".to_string()));
    }

    #[test]
    fn test_expand_with_topdir() {
        let temp_dir = TempDir::new().unwrap();
        let build_dir = create_test_build_env(temp_dir.path());

        let env = BuildEnvironment::from_build_dir(&build_dir).unwrap();

        let expanded = env.expand("${TOPDIR}/my/path");
        assert!(expanded.ends_with("/my/path"));
        assert!(!expanded.contains("${TOPDIR}"));
    }
}
