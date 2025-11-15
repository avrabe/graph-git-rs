//! BitBake configuration file parsers
//!
//! Parsers for bblayers.conf and local.conf that extract configuration
//! needed to set up a build environment.

use crate::BitbakeRecipe;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Configuration from bblayers.conf
#[derive(Debug, Clone)]
pub struct BbLayersConfig {
    /// List of layer paths from BBLAYERS
    pub bblayers: Vec<PathBuf>,
    /// BBPATH value (usually "${TOPDIR}")
    pub bbpath: Option<String>,
    /// BBFILES patterns
    pub bbfiles: Vec<String>,
    /// All variables from the file
    pub variables: HashMap<String, String>,
}

impl BbLayersConfig {
    /// Parse a bblayers.conf file
    pub fn parse<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref();
        info!("Parsing bblayers.conf: {:?}", path);

        let recipe = BitbakeRecipe::parse_file(path)?;

        // Extract BBLAYERS
        let bblayers_str = recipe
            .variables
            .get("BBLAYERS")
            .ok_or("BBLAYERS variable not found in bblayers.conf")?;

        // Parse BBLAYERS - it's a space-separated list of paths
        let bblayers: Vec<PathBuf> = bblayers_str
            .split_whitespace()
            .filter(|s| !s.is_empty() && *s != "\\")
            .map(PathBuf::from)
            .collect();

        debug!("Found {} layers in BBLAYERS", bblayers.len());

        // Extract BBPATH
        let bbpath = recipe.variables.get("BBPATH").cloned();

        // Extract BBFILES
        let bbfiles = recipe
            .variables
            .get("BBFILES")
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();

        Ok(BbLayersConfig {
            bblayers,
            bbpath,
            bbfiles,
            variables: recipe.variables,
        })
    }

    /// Get layer paths (may contain unexpanded variables)
    pub fn get_bblayers(&self) -> &[PathBuf] {
        &self.bblayers
    }
}

/// Configuration from local.conf
#[derive(Debug, Clone)]
pub struct LocalConfig {
    /// MACHINE setting
    pub machine: Option<String>,
    /// DISTRO setting
    pub distro: Option<String>,
    /// Downloads directory (DL_DIR)
    pub dl_dir: Option<PathBuf>,
    /// Build output directory (TMPDIR)
    pub tmpdir: Option<PathBuf>,
    /// Shared state cache directory (SSTATE_DIR)
    pub sstate_dir: Option<PathBuf>,
    /// Number of parallel bitbake threads
    pub bb_number_threads: Option<usize>,
    /// Parallel make setting
    pub parallel_make: Option<String>,
    /// Package format (PACKAGE_CLASSES)
    pub package_classes: Option<Vec<String>>,
    /// All variables from the file
    pub variables: HashMap<String, String>,
}

impl LocalConfig {
    /// Parse a local.conf file
    pub fn parse<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref();
        info!("Parsing local.conf: {:?}", path);

        let recipe = BitbakeRecipe::parse_file(path)?;

        // Extract common variables
        let machine = recipe.variables.get("MACHINE").cloned();
        let distro = recipe.variables.get("DISTRO").cloned();

        let dl_dir = recipe.variables.get("DL_DIR").map(PathBuf::from);
        let tmpdir = recipe.variables.get("TMPDIR").map(PathBuf::from);
        let sstate_dir = recipe.variables.get("SSTATE_DIR").map(PathBuf::from);

        let bb_number_threads = recipe
            .variables
            .get("BB_NUMBER_THREADS")
            .and_then(|s| s.parse().ok());

        let parallel_make = recipe.variables.get("PARALLEL_MAKE").cloned();

        let package_classes = recipe
            .variables
            .get("PACKAGE_CLASSES")
            .map(|s| s.split_whitespace().map(String::from).collect());

        debug!(
            "Local config: MACHINE={:?}, DISTRO={:?}",
            machine, distro
        );

        Ok(LocalConfig {
            machine,
            distro,
            dl_dir,
            tmpdir,
            sstate_dir,
            bb_number_threads,
            parallel_make,
            package_classes,
            variables: recipe.variables,
        })
    }
}

/// Variable expander for ${VAR} syntax
#[derive(Debug, Clone)]
pub struct VariableExpander {
    variables: HashMap<String, String>,
}

impl VariableExpander {
    /// Create a new variable expander
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    /// Set a variable value
    pub fn set(&mut self, key: String, value: String) {
        self.variables.insert(key, value);
    }

    /// Get a variable value
    pub fn get(&self, key: &str) -> Option<&String> {
        self.variables.get(key)
    }

    /// Expand variables in a string
    ///
    /// Replaces ${VAR} with variable values. Supports nested expansion.
    pub fn expand(&self, input: &str) -> String {
        let mut result = input.to_string();
        let mut changed = true;
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 100; // Prevent infinite loops

        // Iterate until no more substitutions (handles nested ${${VAR}})
        while changed && iterations < MAX_ITERATIONS {
            iterations += 1;
            changed = false;
            let before = result.clone();

            // Find all ${...} patterns
            let mut pos = 0;
            while let Some(start) = result[pos..].find("${") {
                let abs_start = pos + start;
                if let Some(end_pos) = result[abs_start + 2..].find('}') {
                    let abs_end = abs_start + 2 + end_pos;
                    let var_name = &result[abs_start + 2..abs_end];

                    if let Some(value) = self.variables.get(var_name) {
                        result.replace_range(abs_start..=abs_end, value);
                        changed = true;
                        // Don't update pos - start over from beginning after substitution
                        break;
                    } else {
                        // Unknown variable, leave as-is and continue
                        pos = abs_end + 1;
                    }
                } else {
                    // No closing }, move past this ${
                    pos = abs_start + 2;
                }
            }

            // Safety check
            if result == before {
                break;
            }
        }

        if iterations >= MAX_ITERATIONS {
            tracing::warn!(
                "Variable expansion hit max iterations for: {}",
                input
            );
        }

        result
    }

    /// Expand variables in a path
    pub fn expand_path(&self, path: &Path) -> PathBuf {
        let path_str = path.to_string_lossy();
        let expanded = self.expand(&path_str);
        PathBuf::from(expanded)
    }

    /// Expand a list of paths
    pub fn expand_paths(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        paths.iter().map(|p| self.expand_path(p)).collect()
    }
}

impl Default for VariableExpander {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_variable_expander() {
        let mut expander = VariableExpander::new();
        expander.set("TOPDIR".to_string(), "/home/user/build".to_string());
        expander.set("MACHINE".to_string(), "qemux86-64".to_string());

        assert_eq!(
            expander.expand("${TOPDIR}/downloads"),
            "/home/user/build/downloads"
        );
        assert_eq!(
            expander.expand("${TOPDIR}/tmp/${MACHINE}"),
            "/home/user/build/tmp/qemux86-64"
        );
        assert_eq!(
            expander.expand("no variables here"),
            "no variables here"
        );
    }

    #[test]
    fn test_nested_expansion() {
        let mut expander = VariableExpander::new();
        expander.set("A".to_string(), "B".to_string());
        expander.set("B".to_string(), "value".to_string());

        // ${${A}} should expand to ${B} then to "value"
        assert_eq!(expander.expand("${${A}}"), "value");
    }

    #[test]
    fn test_bblayers_conf_parse() {
        let temp_dir = TempDir::new().unwrap();
        let conf_path = temp_dir.path().join("bblayers.conf");

        let content = r#"
POKY_BBLAYERS_CONF_VERSION = "2"
BBPATH = "${TOPDIR}"
BBFILES ?= ""

BBLAYERS ?= " \
  /path/to/meta \
  /path/to/meta-poky \
  /path/to/meta-yocto-bsp \
  "
"#;

        fs::write(&conf_path, content).unwrap();

        let config = BbLayersConfig::parse(&conf_path).unwrap();

        assert_eq!(config.bblayers.len(), 3);
        assert_eq!(config.bblayers[0], PathBuf::from("/path/to/meta"));
        assert_eq!(config.bbpath, Some("${TOPDIR}".to_string()));
    }

    #[test]
    fn test_local_conf_parse() {
        let temp_dir = TempDir::new().unwrap();
        let conf_path = temp_dir.path().join("local.conf");

        let content = r#"
MACHINE ??= "qemux86-64"
DISTRO ?= "poky"
DL_DIR ?= "${TOPDIR}/downloads"
TMPDIR = "${TOPDIR}/tmp"
SSTATE_DIR ?= "${TOPDIR}/sstate-cache"
PACKAGE_CLASSES ?= "package_rpm"
BB_NUMBER_THREADS ?= "8"
PARALLEL_MAKE ?= "-j 8"
"#;

        fs::write(&conf_path, content).unwrap();

        let config = LocalConfig::parse(&conf_path).unwrap();

        assert_eq!(config.machine, Some("qemux86-64".to_string()));
        assert_eq!(config.distro, Some("poky".to_string()));
        assert_eq!(
            config.dl_dir,
            Some(PathBuf::from("${TOPDIR}/downloads"))
        );
        assert_eq!(config.bb_number_threads, Some(8));
    }

    #[test]
    fn test_expand_path() {
        let mut expander = VariableExpander::new();
        expander.set("TOPDIR".to_string(), "/home/user/build".to_string());

        let path = PathBuf::from("${TOPDIR}/downloads");
        let expanded = expander.expand_path(&path);

        assert_eq!(expanded, PathBuf::from("/home/user/build/downloads"));
    }
}
