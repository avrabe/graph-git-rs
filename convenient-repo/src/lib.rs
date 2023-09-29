use serde::{Deserialize, Serialize};
use std::default::Default;
use std::str::FromStr;
use xmlem::display::Config;
use xmlem::Document;

#[derive(Default, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename = "manifest")]
pub struct Manifest {
    pub remote: Option<Vec<Remote>>,
    pub default: Option<ManifestDefault>,
    #[serde(rename = "manifest-server")]
    pub manifest_server: Option<ManifestServer>,
    #[serde(rename = "remove-project")]
    pub remove_project: Option<Vec<RemoveProject>>,
    pub project: Option<Vec<Project>>,
    #[serde(rename = "extend-project")]
    pub extend_project: Option<Vec<ExtendProject>>,
    #[serde(rename = "repo-hooks")]
    pub repo_hooks: Option<Vec<RepoHooks>>,
    pub include: Option<Vec<Include>>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct Remote {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@alias")]
    pub alias: Option<String>,
    #[serde(rename = "@fetch")]
    pub fetch: Option<String>,
    #[serde(rename = "@review")]
    pub review: Option<String>,
    #[serde(rename = "@revision")]
    pub revision: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct ManifestDefault {
    #[serde(rename = "@remote")]
    pub remote: Option<String>,
    #[serde(rename = "@revision")]
    pub revision: Option<String>,
    #[serde(rename = "@dest-branch")]
    pub dest_branch: Option<String>,
    #[serde(rename = "@sync-j")]
    pub sync_j: Option<String>,
    #[serde(rename = "@sync-c")]
    pub sync_c: Option<String>,
    #[serde(rename = "@sync-s")]
    pub sync_s: Option<String>,
    #[serde(rename = "@sync-tags")]
    pub sync_tags: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct ManifestServer {
    #[serde(rename = "@url")]
    pub url: String,
}
#[derive(Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoveProject {
    #[serde(rename = "@name")]
    pub name: String,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Project {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@path")]
    pub path: Option<String>,
    #[serde(rename = "@remote")]
    pub remote: Option<String>,
    #[serde(rename = "@groups")]
    pub groups: Option<String>,
    #[serde(rename = "@revision")]
    pub revision: Option<String>,
    #[serde(rename = "@dest-branch")]
    pub dest_branch: Option<String>,
    #[serde(rename = "@sync-c")]
    pub sync_c: Option<String>,
    #[serde(rename = "@sync-s")]
    pub sync_s: Option<String>,
    #[serde(rename = "@sync-tags")]
    pub sync_tags: Option<String>,
    #[serde(rename = "@upstream")]
    pub upstream: Option<String>,
    #[serde(rename = "@clone-depth")]
    pub clone_depth: Option<String>,
    #[serde(rename = "@force-path")]
    pub force_path: Option<String>,
    #[serde(rename = "annotation")]
    pub annotation: Option<Vec<Annotation>>,
    #[serde(rename = "project")]
    pub project: Option<Vec<Project>>,
    #[serde(rename = "copyfile")]
    pub copyfile: Option<Vec<Copyfile>>,
    #[serde(rename = "linkfile")]
    pub linkfile: Option<Vec<Linkfile>>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Annotation {
    #[serde(rename = "@name")]
    pub name: String,
    pub value: String,
    pub keep: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Copyfile {
    #[serde(rename = "@src")]
    pub src: String,
    #[serde(rename = "@dest")]
    pub dest: String,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Linkfile {
    #[serde(rename = "@src")]
    pub src: String,
    #[serde(rename = "@dest")]
    pub dest: String,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct RepoHooks {
    #[serde(rename = "@in-project")]
    pub in_project: String,
    #[serde(rename = "@enabled-list")]
    pub enabled_list: String,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtendProject {
    #[serde(rename = "@name")]
    pub name: String,
    pub path: Option<String>,
    pub remote: Option<String>,
    pub revision: Option<String>,
    pub groups: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct Include {
    #[serde(rename = "@name")]
    pub name: String,
}

pub fn to_string(manifest: &Manifest) -> String {
    let cleanup = vec![
        "<remote/>",
        "<default/>",
        "<manifest-server/>",
        "<remove-project/>",
        "<project/>",
        "<extend-project/>",
        "<repo-hooks/>",
        "<linkfile/>",
        "<copyfile/>",
        "<groups/>",
        "<include/>",
        "<upstream/>",
        "<annotation/>",
        "<alias/>",
        "<path/>",
        "<revision/>",
        "<fetch/>",
        "<review/>",
        "dest-branch=\"\"",
        "sync-j=\"\"",
        "sync-c=\"\"",
        "sync-s=\"\"",
        "sync-tags=\"\"",
        "clone-depth=\"\"",
        "force-path=\"\"",
        "remote=\"\"",
        "groups=\"\"",
        "project=\"\"",
        "revision=\"\"",
        "path=\"\"",
        "upstream=\"\"",
        "alias=\"\"",
    ];
    let mut result = quick_xml::se::to_string(&manifest).unwrap();
    for i in cleanup {
        result = result.replace(i, "");
    }
    result = result.replace("  ", " ");
    result = result.replace("  ", " ");
    result = result.replace("  ", " ");
    result = result.replace(" >", ">");
    result = result.replace(" >", ">");
    let doc = Document::from_str(&result).unwrap();
    let config = Config {
        is_pretty: true,
        max_line_length: 200,
        ..Default::default()
    };
    doc.to_string_pretty_with_config(&config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::{self, de::from_str}; //, EventReader, ParserConfig};
    use std::{fs, path::PathBuf};

    #[test]
    fn default_remote_name() {
        let src = r#"<manifest><remote name="aptiv"/></manifest>"#;
        let should_be = Manifest {
            remote: Some(vec![Remote {
                name: "aptiv".into(),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let item: Manifest = from_str(src).unwrap();
        assert_eq!(item, should_be);

        //let reserialized_item = to_string(&item).unwrap();
        //assert_eq!(src, reserialized_item);
    }

    #[test]
    fn default_remove_project() {
        let src =
            r#"<manifest><remove-project name="foo"/><remove-project name="bar"/></manifest>"#;
        let should_be = Manifest {
            remove_project: Some(vec![
                RemoveProject { name: "foo".into() },
                RemoveProject { name: "bar".into() },
            ]),
            ..Default::default()
        };
        let item: Manifest = from_str(src).unwrap();
        assert_eq!(item, should_be);

        //let reserialized_item = to_string(&item).unwrap();
        //assert_eq!(src, reserialized_item);
    }

    #[test]
    fn default_reserialize_project() {
        //let src = r#"<manifest></manifest>"#;
        let src = r#"<manifest/>"#;

        //let should_be = Manifest {
        //    remove_project: Some(vec![RemoveProject {
        //        name: "foo".into(),
        //    },RemoveProject {
        //        name: "bar".into(),
        //    } ]),
        //    ..Default::default()
        //};
        let item: Manifest = from_str(src).unwrap();
        //assert_eq!(item, should_be);
        print!("{:?}", item);
        let reserialized_item = to_string(&item);
        let reserialized_item = reserialized_item.trim();
        assert_eq!(src, reserialized_item);
    }

    #[test]
    fn default_read_manifest() {
        let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        d.push("example.xml");
        let src = fs::read_to_string(d).unwrap();
        //let should_be = Manifest {
        //    remove_project: Some(vec![RemoveProject {
        //        name: "foo".into(),
        //    },RemoveProject {
        //        name: "bar".into(),
        //    } ]),
        //    ..Default::default()
        //};
        let _item: Manifest = from_str(&src).unwrap();
        //assert_eq!(item, should_be);

        //let reserialized_item = to_string(&item).unwrap();
        //assert_eq!(src, reserialized_item);
    }
}
