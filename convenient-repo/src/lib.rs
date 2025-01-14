use serde::{Deserialize, Serialize};
use std::default::Default;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{debug, info, warn};
use url::Url;
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

pub struct ProjectsIterator<'a> {
    manifest: &'a Manifest,
    index: usize,
}

#[derive(Debug)]
pub struct ConvenientProject {
    pub name: String,
    pub git_uri: String,
    pub relative_path: Option<bool>,
    pub revision: String,
    pub dest_branch: Option<String>,
}

impl ConvenientProject {
    pub fn git_url(&self, manifest_git_url: String) -> String {
        let mut url = Url::parse(&manifest_git_url).unwrap();
        if self.relative_path.unwrap_or(false) {
            if self.git_uri.ends_with('/') {
                url = url.join(self.git_uri.as_str()).unwrap();
            } else {
                url = url
                    .join(format!("{}/{}", self.git_uri, self.name).as_str())
                    .unwrap();
            }
            info!("New url for {}: {} # {}", self.name, url, self.git_uri);

            url.to_string()
        } else {
            url = url
                .join(format!("{}/{}", self.git_uri, self.name).as_str())
                .unwrap();
            info!("New url for {}: {} # {}", self.name, url, self.git_uri);
            url.to_string()
        }
    }

    fn could_be_sha256(s: &str) -> bool {
        if s.len() != 64 {
            return false;
        }

        for c in s.chars() {
            if !c.is_ascii_hexdigit() {
                return false;
            }
        }

        true
    }

    fn could_be_sha1(s: &str) -> bool {
        if s.len() != 40 {
            return false;
        }

        for c in s.chars() {
            if !c.is_ascii_hexdigit() {
                return false;
            }
        }

        true
    }

    pub fn dest_branch(&self) -> String {
        let dest_branch = match self.dest_branch {
            Some(ref dest_branch) => dest_branch.clone(),
            None => self.revision.clone(),
        };
        if dest_branch.starts_with("refs/tags/") {
            return dest_branch.replace("refs/tags/", "");
        }
        if dest_branch.starts_with("refs/heads/") {
            return dest_branch.replace("refs/heads/", "");
        }
        dest_branch
    }

    pub fn is_dest_branch_a_commit(&self) -> bool {
        let dest_branch = match self.dest_branch {
            Some(ref dest_branch) => dest_branch.clone(),
            None => self.revision.clone(),
        };
        Self::could_be_sha1(&dest_branch) || Self::could_be_sha256(&dest_branch)
    }

    pub fn is_dest_branch_a_tag(&self) -> bool {
        let dest_branch = match self.dest_branch {
            Some(ref dest_branch) => dest_branch.clone(),
            None => self.revision.clone(),
        };
        dest_branch.starts_with("refs/tags/")
    }
}

impl<'a> ProjectsIterator<'a> {
    pub fn new(manifest: &'a Manifest) -> Self {
        ProjectsIterator { manifest, index: 0 }
    }
    fn get_git_uri(&mut self, remote_name: &str) -> Option<String> {
        match self.manifest.remote {
            Some(ref list) => {
                for remote in list {
                    if remote.name == remote_name {
                        return Some(remote.fetch.clone().unwrap_or_else(|| "TODO".to_string()));
                    }
                }
                None
            }
            None => None,
        }
    }
    fn get_remote_or_default_or_todo(&mut self, remote_name: &Option<String>) -> String {
        match remote_name {
            Some(name) => name.to_string(),
            None => match self.manifest.default {
                Some(ref default) => match default.remote {
                    Some(ref name) => name.clone(),
                    None => "TODO".to_string(),
                },
                None => "TODO".to_string(),
            },
        }
    }

    fn get_revision_or_default_or_todo(&mut self, remote: &Project) -> String {
        match remote.revision {
            Some(ref revision) => revision.clone(),
            None => match self.manifest.default {
                Some(ref default) => match default.revision {
                    Some(ref revision) => revision.clone(),
                    None => "TODO".to_string(),
                },
                None => "TODO".to_string(),
            },
        }
    }

    fn get_dest_branch_or_default_or_todo(&mut self, remote: &Project) -> Option<String> {
        match remote.dest_branch {
            Some(ref dest_branch) => Some(dest_branch.clone()),
            None => match self.manifest.default {
                Some(ref default) => default.dest_branch.as_ref().cloned(),
                None => None,
            },
        }
    }

    fn is_relative(&mut self, remote: String) -> Option<bool> {
        let url = Url::parse(&remote);
        match url {
            Ok(_uri) => Some(false),
            Err(e) => match e {
                url::ParseError::RelativeUrlWithoutBase => Some(true),
                _ => None,
            },
        }
    }
}

impl<'a> Iterator for ProjectsIterator<'a> {
    type Item = Arc<ConvenientProject>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.manifest.project {
            Some(ref list) => {
                if self.index < list.len() {
                    let p: &Project = &list[self.index];
                    let name = p.name.clone();
                    let remote = self.get_remote_or_default_or_todo(&p.remote);
                    let git_uri = self
                        .get_git_uri(&remote)
                        .unwrap_or_else(|| "TODO".to_string());
                    let relative_path = self.is_relative(git_uri.clone());
                    let revision = self.get_revision_or_default_or_todo(p);
                    let dest_branch = self.get_dest_branch_or_default_or_todo(p);
                    let result = ConvenientProject {
                        name,
                        git_uri,
                        relative_path,
                        revision,
                        dest_branch,
                    };
                    let result = Some(Arc::new(result));
                    self.index += 1;
                    result
                } else {
                    None
                }
            }
            None => None,
        }
    }
}

impl Manifest {
    pub fn iter(&self) -> ProjectsIterator {
        ProjectsIterator {
            manifest: self,
            index: 0,
        }
    }
}

/// Converts the given Manifest struct to a string by:
/// - Serializing the struct to XML using quick_xml::se::to_string
/// - Removing unwanted empty XML tags from the cleanup vector
/// - Replacing double spaces with single spaces
/// - Parsing the string into a Document and pretty printing it
///
/// # Arguments
///
/// * `manifest` - The Manifest struct to convert to a string
///
/// # Returns
///
/// The pretty printed XML string representation of the Manifest
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
    let config = Config::default_pretty();
    doc.to_string_pretty_with_config(&config)
}

pub fn find_repo_manifest(path: &Path) -> Vec<Manifest> {
    let mut kas_manifests = Vec::<Manifest>::new();

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
                match quick_xml::de::from_str(&contents) {
                    Ok(manifest) => {
                        kas_manifests.push(manifest);
                        info!("Found repo manifest: {}", path.display());
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

#[cfg(test)]
/**
 * Tests for parsing the manifest XML.
 *
 * - default_remote_name: Tests parsing a manifest with a single remote. Verifies remote name is parsed correctly.
 * - default_remove_project: Tests parsing a manifest with multiple remove-project elements. Verifies names are parsed correctly.
 * - default_reserialize_project: Tests parsing and re-serializing an empty manifest. Verifies round trip parsing and serialization works.
 * - default_read_manifest: Tests parsing manifest from example.xml file. Verifies parsing from file works.
 */
mod tests {
    use super::*;
    use quick_xml::{self, de::from_str};
    use std::{fs, path::PathBuf};
    use tracing_test::traced_test; //, EventReader, ParserConfig};

    #[test]
    fn default_remote_name() {
        let src = r#"<manifest><remote name="mine"/></manifest>"#;
        let should_be = Manifest {
            remote: Some(vec![Remote {
                name: "mine".into(),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let item: Manifest = from_str(src).unwrap();
        assert_eq!(item, should_be);
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

    #[traced_test]
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
        let item: Manifest = from_str(&src).unwrap();
        for i in item.iter() {
            info!(
                "{:?}, {}, {}",
                i,
                i.git_url("http://foo/gogo/".to_string()),
                i.dest_branch()
            );
        }
        let len = item.iter().collect::<Vec<Arc<ConvenientProject>>>().len();
        assert_eq!(len, 1300);

        //let reserialized_item = to_string(&item).unwrap();
        //assert_eq!(src, reserialized_item);
    }

    #[test]
    fn test_find_manifest() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let result = find_repo_manifest(&d);
        assert_eq!(result.len(), 1);

        //let reserialized_item = to_string(&item).unwrap();
        //assert_eq!(src, reserialized_item);
    }
    #[test]
    fn valid_sha256() {
        let valid = "af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf";
        assert!(ConvenientProject::could_be_sha256(valid));
    }

    #[test]
    fn invalid_short_sha256() {
        let invalid = "af2bdbe1";
        assert!(!ConvenientProject::could_be_sha256(invalid));
    }

    #[test]
    fn invalid_chars_sha256() {
        let invalid = "zg2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf";
        assert!(!ConvenientProject::could_be_sha256(invalid));
    }

    #[test]
    fn valid_sha1() {
        let valid = "cbe01a154b16c19ceedee213e65df225f8f72b0f";
        assert!(ConvenientProject::could_be_sha1(valid));
    }

    #[test]
    fn invalid_short_sha1() {
        let invalid = "af2bdbe";
        assert!(!ConvenientProject::could_be_sha1(invalid));
    }

    #[test]
    fn invalid_chars_sha1() {
        let invalid = "zyt01a154b16c19ceedee213e65df225f8f72b0f";
        assert!(!ConvenientProject::could_be_sha1(invalid));
    }
}
