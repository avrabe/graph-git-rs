use std::collections::HashMap;

use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct KasManifest {
    pub repos: HashMap<String, Option<Repository>>
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Repository {
    url: Option<String>,
    refspec: Option<String>,
}


#[cfg(test)]
mod tests {
    use crate::KasManifest;

    #[test]
    fn it_works() {
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
        assert_eq!(kas_manifest.repos["poky"].as_ref().unwrap().refspec, Some("master".to_string()));
    }
}
