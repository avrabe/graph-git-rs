//! Enhanced Kas YAML parsing with include graph and checksums
//!
//! Provides complete kas configuration parsing including:
//! - Include dependency resolution
//! - File checksum tracking for cache invalidation
//! - Merged configuration generation
//! - Repository and layer management

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};

/// Complete kas configuration (header + content)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KasConfig {
    /// Kas file header with version and includes
    pub header: KasHeader,
    /// Target machine (e.g., "qemux86-64")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    /// Distribution to build (e.g., "poky")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distro: Option<String>,
    /// Build targets/recipes (e.g., ["core-image-minimal"])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Vec<String>>,
    /// Repository configurations
    #[serde(default)]
    pub repos: HashMap<String, KasRepo>,
    /// Custom headers for bblayers.conf
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bblayers_conf_header: Option<HashMap<String, String>>,
    /// Custom headers for local.conf
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_conf_header: Option<HashMap<String, String>>,
    /// Environment variables to set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

/// Kas file header with version and includes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KasHeader {
    /// Kas format version (current: 14)
    pub version: u32,
    /// List of kas files to include
    #[serde(skip_serializing_if = "Option::is_none")]
    pub includes: Option<Vec<String>>,
}

/// Repository configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KasRepo {
    /// Git repository URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Git refspec to checkout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refspec: Option<String>,
    /// Git branch to checkout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Git commit SHA to checkout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Git tag to checkout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Local path to repository (alternative to URL)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Layers within this repository
    #[serde(default)]
    pub layers: HashMap<String, KasLayer>,
    /// Patches to apply to this repository
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patches: Option<HashMap<String, Vec<String>>>,
}

/// Layer configuration within a repository
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KasLayer {
    /// Relative path to layer within repository
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Kas file with metadata
#[derive(Debug, Clone)]
pub struct KasFile {
    /// Path to the kas file
    pub path: PathBuf,
    /// Parsed configuration
    pub config: KasConfig,
    /// SHA256 checksum for cache invalidation
    pub checksum: String,
}

impl KasFile {
    /// Load and parse kas file with checksum
    pub async fn load(path: impl AsRef<Path>) -> Result<Self, KasError> {
        let path = path.as_ref();
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| KasError::IoError(path.to_path_buf(), e.to_string()))?;

        let checksum = Self::calculate_checksum(&content);
        let config: KasConfig = serde_yaml::from_str(&content)
            .map_err(|e| KasError::ParseError(path.to_path_buf(), e.to_string()))?;

        Ok(Self {
            path: path.to_path_buf(),
            config,
            checksum,
        })
    }

    /// Calculate SHA256 checksum of content
    fn calculate_checksum(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Get all include paths from this kas file
    pub fn includes(&self) -> Vec<PathBuf> {
        self.config
            .header
            .includes
            .as_ref()
            .map(|includes| {
                includes
                    .iter()
                    .map(|inc| self.resolve_include_path(inc))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve include path relative to this kas file
    fn resolve_include_path(&self, include: &str) -> PathBuf {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        parent.join(include)
    }
}

/// Kas include dependency graph
#[derive(Debug)]
pub struct KasIncludeGraph {
    /// All kas files indexed by path
    files: HashMap<PathBuf, KasFile>,
    /// Include dependencies (path -> included paths)
    dependencies: HashMap<PathBuf, Vec<PathBuf>>,
    /// Root kas file path
    root: PathBuf,
}

impl KasIncludeGraph {
    /// Build include graph starting from root kas file
    pub async fn build(root_path: impl AsRef<Path>) -> Result<Self, KasError> {
        let root_path = root_path.as_ref().to_path_buf();
        let mut files = HashMap::new();
        let mut dependencies = HashMap::new();
        let mut visited = HashSet::new();

        Self::build_recursive(&root_path, &mut files, &mut dependencies, &mut visited).await?;

        Ok(Self {
            files,
            dependencies,
            root: root_path,
        })
    }

    fn build_recursive<'a>(
        path: &'a PathBuf,
        files: &'a mut HashMap<PathBuf, KasFile>,
        dependencies: &'a mut HashMap<PathBuf, Vec<PathBuf>>,
        visited: &'a mut HashSet<PathBuf>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), KasError>> + 'a>> {
        Box::pin(async move {
            if visited.contains(path) {
                return Ok(());
            }

            visited.insert(path.clone());

            let kas_file = KasFile::load(path).await?;
            let includes = kas_file.includes();

            dependencies.insert(path.clone(), includes.clone());

            // Recursively process includes
            for include_path in includes {
                Self::build_recursive(&include_path, files, dependencies, visited).await?;
            }

            files.insert(path.clone(), kas_file);

            Ok(())
        })
    }

    /// Get topologically sorted kas files (dependencies first)
    pub fn sorted_files(&self) -> Vec<&KasFile> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();

        self.visit_sorted(&self.root, &mut visited, &mut result);

        result
    }

    fn visit_sorted<'a>(
        &'a self,
        path: &PathBuf,
        visited: &mut HashSet<PathBuf>,
        result: &mut Vec<&'a KasFile>,
    ) {
        if visited.contains(path) {
            return;
        }

        visited.insert(path.clone());

        // Visit dependencies first
        if let Some(deps) = self.dependencies.get(path) {
            for dep in deps {
                self.visit_sorted(dep, visited, result);
            }
        }

        // Add this file after its dependencies
        if let Some(file) = self.files.get(path) {
            result.push(file);
        }
    }

    /// Merge all kas configs in dependency order
    pub fn merge_config(&self) -> KasConfig {
        let mut merged = KasConfig {
            header: KasHeader {
                version: 14, // Use latest version
                includes: None,
            },
            machine: None,
            distro: None,
            target: None,
            repos: HashMap::new(),
            bblayers_conf_header: None,
            local_conf_header: None,
            env: None,
        };

        // Merge in dependency order (includes first, root last)
        for file in self.sorted_files() {
            Self::merge_into(&mut merged, &file.config);
        }

        merged
    }

    fn merge_into(base: &mut KasConfig, overlay: &KasConfig) {
        // Machine/distro/target: overlay wins
        if overlay.machine.is_some() {
            base.machine = overlay.machine.clone();
        }
        if overlay.distro.is_some() {
            base.distro = overlay.distro.clone();
        }
        if overlay.target.is_some() {
            base.target = overlay.target.clone();
        }

        // Repos: merge repositories
        for (name, repo) in &overlay.repos {
            base.repos.insert(name.clone(), repo.clone());
        }

        // Headers: merge maps
        Self::merge_header_map(&mut base.bblayers_conf_header, &overlay.bblayers_conf_header);
        Self::merge_header_map(&mut base.local_conf_header, &overlay.local_conf_header);
        Self::merge_header_map(&mut base.env, &overlay.env);
    }

    fn merge_header_map(
        base: &mut Option<HashMap<String, String>>,
        overlay: &Option<HashMap<String, String>>,
    ) {
        if let Some(overlay_map) = overlay {
            let base_map = base.get_or_insert_with(HashMap::new);
            for (key, value) in overlay_map {
                base_map.insert(key.clone(), value.clone());
            }
        }
    }

    /// Get combined checksum of all kas files in graph
    pub fn combined_checksum(&self) -> String {
        let mut hasher = Sha256::new();

        for file in self.sorted_files() {
            hasher.update(file.checksum.as_bytes());
        }

        format!("{:x}", hasher.finalize())
    }

    /// Get all kas files
    pub fn files(&self) -> &HashMap<PathBuf, KasFile> {
        &self.files
    }

    /// Get root kas file
    ///
    /// # Panics
    ///
    /// Panics if the root file is not found in the graph (should never happen if graph was built successfully)
    pub fn root(&self) -> &KasFile {
        self.files
            .get(&self.root)
            .expect("root file must exist in graph")
    }
}

/// Kas error types
#[derive(Debug, thiserror::Error)]
pub enum KasError {
    /// File system I/O error
    #[error("IO error reading {0}: {1}")]
    IoError(PathBuf, String),

    /// YAML parsing error
    #[error("Parse error in {0}: {1}")]
    ParseError(PathBuf, String),

    /// Include file not found
    #[error("Include not found: {0}")]
    IncludeNotFound(PathBuf),

    /// Circular include dependency detected
    #[error("Circular include detected: {0}")]
    CircularInclude(String),

    /// Invalid repository configuration
    #[error("Invalid repository configuration: {0}")]
    InvalidRepo(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kas_config_parse() {
        let yaml = r#"
header:
  version: 14
  includes:
    - common.yml

machine: qemux86-64
distro: poky
target:
  - core-image-minimal

repos:
  poky:
    url: https://git.yoctoproject.org/git/poky
    refspec: kirkstone
    layers:
      meta:
      meta-poky:

local_conf_header:
  custom: |
    DL_DIR = "${TOPDIR}/downloads"
"#;

        let config: KasConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.header.version, 14);
        assert_eq!(config.machine, Some("qemux86-64".to_string()));
        assert_eq!(config.repos.len(), 1);
    }

    #[test]
    fn test_checksum_calculation() {
        let content = "test content";
        let checksum = KasFile::calculate_checksum(content);
        assert_eq!(checksum.len(), 64); // SHA256 = 64 hex chars
    }
}
