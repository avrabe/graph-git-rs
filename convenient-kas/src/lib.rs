use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct Kas {
    pub path: String,
    pub manifest: KasManifest,
}
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct KasManifest {
    pub repos: HashMap<String, Option<Repository>>,
}

// TODO: Way to enable either refspec or branch, commit
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct Repository {
    pub url: Option<String>,
    pub refspec: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
}

impl KasManifest {
    /// Finds KasManifest files recursively in the given path.
    ///
    /// # Arguments
    ///
    /// * `path` - The root path to search for KasManifest files.
    ///
    /// # Returns
    ///
    /// A vector containing any found KasManifest files deserialized from YAML.
    ///
    /// # Errors
    ///
    /// Any IO or parsing errors are logged via `warn!` and skipped.
    pub fn find_kas_manifest(path: &Path) -> Vec<Kas> {
        let mut kas_manifests = Vec::<Kas>::new();

        for entry in path.read_dir().unwrap() {
            match entry {
                Ok(entry) => {
                    if entry.file_type().unwrap().is_dir() {
                        continue;
                    }

                    if !entry.path().is_file() {
                        continue;
                    }

                    let path = entry.path();

                    if !path.is_file() {
                        continue;
                    }

                    let contents = match std::fs::read_to_string(&path) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("Failed to read file {}: {}", path.display(), e);
                            continue;
                        }
                    };
                    match serde_yaml::from_str::<KasManifest>(&contents) {
                        Ok(manifest) => {
                            let kas = Kas {
                                path: path.file_name().unwrap().to_str().unwrap().to_string(),
                                manifest,
                            };
                            kas_manifests.push(kas)
                        }
                        Err(e) => debug!("Failed to parse {}: {}", path.display(), e),
                    }
                }
                Err(e) => {
                    warn!("Error reading directory entry: {}", e);
                }
            };
        }
        kas_manifests
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::KasManifest;

    #[test]
    fn kas_manifest_find() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let manifests = KasManifest::find_kas_manifest(d.as_path());
        assert_eq!(manifests.len(), 1);
    }

    #[test]
    fn kas_manifest_deserialize() {
        let manifest = "# Every file needs to contain a header, that provides kas with information
        # about the context of this file.
        header:
          # The `version` entry in the header describes for which configuration
          # format version this file was created for. It is used by kas to figure
          # out if it is compatible with this file. The version is an integer that
          # is increased on every format change.
          version: 8
        # The machine as it is written into the `local.conf` of bitbake.
        machine: qemuarm64
        # The distro name as it is written into the `local.conf` of bitbake.
        distro: container
        target: fmu-image
        repos:
          # This entry includes the repository where the config file is located
          # to the bblayers.conf:
          meta-custom:
          # Here we include a list of layers from the poky repository to the
          # bblayers.conf:
          poky:
            url: \"https://git.yoctoproject.org/git/poky\"
            refspec: master
            layers:
              meta:
              meta-poky:
          meta-openembedded:
            url: \"https://github.com/openembedded/meta-openembedded.git\"
            refspec: master 
            layers:
              meta-oe:
        local_conf_header:
          meta-custom: |
            IMAGE_FSTYPES = \"container\"
            INHERIT += \"rm_work_and_downloads create-spdx\"
          buildoptimize: |
            BB_NUMBER_THREADS ?= \"${@oe.utils.cpu_count()*3}\"
            PARALLEL_MAKE ?= \"-j ${@oe.utils.cpu_count()*3}\"
        ";
        let kas_manifest: KasManifest = serde_yaml::from_str(&manifest).unwrap();
        assert_eq!(kas_manifest.repos.len(), 3);
        assert_eq!(kas_manifest.repos["meta-custom"], None);
        assert_eq!(
            kas_manifest.repos["poky"].as_ref().unwrap().refspec,
            Some("master".to_string())
        );
    }
}
