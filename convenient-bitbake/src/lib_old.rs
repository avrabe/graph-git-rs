use std::{env, path::Path};

use serde::{Deserialize, Serialize};
use tracing::{info, span, warn, Level};
use tree_sitter::{Parser, Query, QueryCursor, Tree, TreeCursor};
use walkdir::{DirEntry, WalkDir};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct Bitbake {
    pub path: String,
    pub src_uris: Vec<String>,
}

impl Default for Bitbake {
    fn default() -> Self {
        Self::new(env::current_dir().unwrap().to_str().unwrap().to_string())
    }
}

impl Bitbake {
    pub fn new(path: String) -> Self {
        Bitbake {
            path,
            src_uris: Vec::<String>::new(),
        }
    }

    fn ends_with_bb_something(entry: &DirEntry) -> bool {
        let ret = entry
            .file_name()
            .to_str()
            .map(|s| s.ends_with(".bb") || s.ends_with(".bbappend"))
            .unwrap_or(true);
        let ret_dir = entry.path().is_dir();
        info!("{} ends with bb something: {}", entry.path().display(), ret);
        //info!("{} is dir: {}", entry.path().display(), ret_dir);
        ret || ret_dir
    }

    pub fn find_first_git_uri(src_uris: &str) -> Option<String> {
        for src_uri in src_uris.lines() {
            if src_uri.starts_with("git://") {
                let mut ret = src_uri.trim();
                // if we get a bitbake git uri, we need to remove the trailing slash
                if ret.ends_with("\\") {
                    ret = &ret[..ret.len() - 1];
                    ret = ret.trim();
                }
                return Some(ret.to_string());
            }
        }
        None
    }

    pub fn find_bitbake_manifest(path: &Path) -> Vec<Bitbake> {
        info!("Searching for BitBake manifests in {}", path.display());
        let mut bitbake_manifests = Vec::<Bitbake>::new();

        let walker = WalkDir::new(path).follow_links(true).into_iter();
        // !Bitbake::is_hidden(e) && Bitbake::is_file(e) && Bitbake::ends_with_bb_something(e)
        let walker = walker.filter_entry(|e: &DirEntry| Self::ends_with_bb_something(e));
        for entry in walker {
            match entry {
                Ok(entry) => {
                    if entry.path().is_dir() {
                        continue;
                    }
                    let bitbake_path = entry.path();
                    info!("Found BitBake manifest: {}", bitbake_path.display());
                    match std::fs::read_to_string(bitbake_path) {
                        Ok(c) => {
                            let src_uri = Self::parse_bitbake(
                                c.as_str(),
                                path.file_name().unwrap().to_str().unwrap(),
                            );
                            let relative_path = bitbake_path.strip_prefix(path).unwrap();
                            let mut bitbake =
                                Bitbake::new(relative_path.to_str().unwrap().to_string());
                            match src_uri {
                                Some(s) => {
                                    let git_uri = Self::find_first_git_uri(&s);
                                    match git_uri {
                                        Some(g) => {
                                            bitbake.src_uris.push(g);
                                        }
                                        None => {
                                            info!("No git uri found in {}", path.display());
                                        }
                                    }
                                }
                                None => {
                                    warn!("No SRC_URI found in {}", path.display());
                                }
                            }
                            if !bitbake.src_uris.is_empty() {
                                bitbake_manifests.push(bitbake);
                            }
                            warn!("Found BitBake manifest: {}", path.display());
                        }
                        Err(e) => {
                            warn!("Failed to read file {}: {}", path.display(), e);
                            //continue;
                        }
                    };
                }
                Err(e) => {
                    warn!("Error reading directory entry: {}", e);
                }
            };
        }

        bitbake_manifests
    }

    pub fn add(_left: usize, _right: usize) -> usize {
        let code = r#"
    BPN = "2"
    inherit cmake
    include foosrcrevinc
    include ${BPN}
    include foo-srcrev.inc
    include ${BPN}-crates.inc
    VAR = "value"

    SRC_URI = "git://git.yoctoproject.org/poky;protocol=https;branch=${BPN}"
    include ${BPN}-crates.inc

    do_configure() {
        cmake -DVAR=${VAR} ${S}
    }
"#;
        Self::parse_bitbake(code, "foo.bb");
        _left + _right
    }

    // TODO: Only return the first find.
    pub fn parse_bitbake(code: &str, filename: &str) -> Option<String> {
        let mut parser = Parser::new();
        parser
            .set_language(tree_sitter_bitbake::language())
            .expect("Error loading BitBake grammar");
        let tree = parser.parse(code, None).unwrap();
        let mut cursor = Tree::walk(&tree);

        Self::walk(&mut cursor, code.as_bytes(), filename);

        // Create a query to match all function declarations
        let query_source = "(variable_assignment (identifier) @name (literal) @value)";
        let query = Query::new(tree_sitter_bitbake::language(), query_source)
            .expect("Error creating query");
        let mut query_cursor = QueryCursor::new();
        let query_matches = query_cursor.matches(&query, tree.root_node(), code.as_bytes());
        let mut ret = None;
        // Print the function names
        for m in query_matches {
            let mut name = "";
            let mut literal = "";
            for c in m.captures {
                if c.index == 0 {
                    // The first capture is the name
                    name = c.node.utf8_text(code.as_bytes()).unwrap();
                }
                if c.index == 1 {
                    // The first capture is the name
                    literal = c.node.utf8_text(code.as_bytes()).unwrap();
                }
            }
            if name == "SRC_URI" {
                info!(
                    "{}:  Found variable: {} with literal: {}",
                    filename, name, literal
                );
                ret = literal.to_string().parse::<String>().ok();
            }
        }
        warn!("Found SRC_URI: {:?}", ret);
        ret
    }

    fn walk(cursor: &mut TreeCursor, text: &[u8], filename: &str) {
        let span = span!(Level::WARN, "walk", value = filename);
        let _ = span.enter();
        //info!("{} {:?}", filename, cursor.node().kind());
        let kind = cursor.node().kind();
        let utf8_text = cursor.node().utf8_text(text).unwrap();
        match (kind, utf8_text) {
            ("identifier", "SRC_URI") => {
                info!("{} {:?} -> {:?}", filename, kind, utf8_text);
                while cursor.goto_next_sibling() {
                    let kind = cursor.node().kind();
                    let utf8_text = cursor.node().utf8_text(text).unwrap();
                    if kind == "literal" {
                        info!("{} {:?} -> {:?}", filename, kind, utf8_text);
                    }
                }
            }
            (_, _) => {
                //debug!("{:?} {:?}", kind, utf8_text);
            }
        }

        if cursor.goto_first_child() {
            Self::walk(cursor, text, filename);

            while cursor.goto_next_sibling() {
                Self::walk(cursor, text, filename);
            }

            cursor.goto_parent();
        }
    }
}

#[cfg(test)]
mod tests {

    use std::path::PathBuf;

    use tracing_test::traced_test;

    use super::*;

    #[traced_test]
    #[test]
    fn it_works() {
        //let bitbake = Bitbake::new();
        let result = Bitbake::add(2, 2);
        assert_eq!(result, 4);
    }

    #[traced_test]
    #[test]
    fn bitbake_manifest_find() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let manifests = Bitbake::find_bitbake_manifest(d.as_path());
        assert!(logs_contain("Searching for BitBake manifests in"));
        assert!(logs_contain("Found BitBake manifest"));
        assert_eq!(manifests.len(), 1);
    }
}
