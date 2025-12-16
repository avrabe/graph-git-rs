//! Convenient Kas - Complete kas YAML parsing and repository management
//!
//! Provides:
//! - Full kas configuration parsing with include graph
//! - Repository cloning and management
//! - BitBake configuration generation
//! - Checksum tracking for cache invalidation

#![warn(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::pedantic
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::doc_markdown
)]

pub mod include_graph;
pub mod repository_manager;
pub mod config_generator;

// Re-export main types
pub use include_graph::{
    find_kas_files, KasConfig, KasError, KasFile, KasHeader, KasIncludeGraph, KasLayer, KasRepo,
};
pub use repository_manager::{RepoError, RepositoryManager};
pub use config_generator::{ConfigError, ConfigGenerator};

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use crate::{find_kas_files, KasConfig};

    #[test]
    fn kas_files_find() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let kas_files = find_kas_files(d.as_path());
        assert_eq!(kas_files.len(), 1);
    }

    #[test]
    fn kas_config_deserialize() {
        let manifest = r#"
header:
  version: 8
machine: qemuarm64
distro: container
target:
  - fmu-image
repos:
  meta-custom: {}
  poky:
    url: "https://git.yoctoproject.org/git/poky"
    refspec: master
    layers:
      meta: {}
      meta-poky: {}
  meta-openembedded:
    url: "https://github.com/openembedded/meta-openembedded.git"
    refspec: master
    layers:
      meta-oe: {}
local_conf_header:
  meta-custom: |
    IMAGE_FSTYPES = "container"
    INHERIT += "rm_work_and_downloads create-spdx"
  buildoptimize: |
    BB_NUMBER_THREADS ?= "${@oe.utils.cpu_count()*3}"
    PARALLEL_MAKE ?= "-j ${@oe.utils.cpu_count()*3}"
"#;
        let config: KasConfig = serde_yaml::from_str(manifest).unwrap();
        assert_eq!(config.repos.len(), 3);
        assert!(config.repos.contains_key("meta-custom"));
        assert_eq!(
            config.repos["poky"].refspec,
            Some("master".to_string())
        );
    }
}
