use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use tree_sitter::{Parser, Query, QueryCursor, Tree, TreeCursor};
use walkdir::{DirEntry, WalkDir};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct Bitbake {}

impl Default for Bitbake {
    fn default() -> Self {
        Self::new()
    }
}

impl Bitbake {
    pub fn new() -> Self {
        Bitbake {}
    }

    fn _is_hidden(entry: &DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    }

    fn _ends_with_bb_something(entry: &DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.ends_with(".bb") || s.ends_with(".bbappend"))
            .unwrap_or(true)
    }

    fn _is_file(entry: &DirEntry) -> bool {
        entry.file_type().is_file()
    }

    pub fn find_bitbake_manifest(path: &Path) -> Vec<Bitbake> {
        info!("Searching for BitBake manifests in {}", path.display());
        let bitbake_manifests = Vec::<Bitbake>::new();

        let walker = WalkDir::new(path).follow_links(true).into_iter();
        // !Bitbake::is_hidden(e) && Bitbake::is_file(e) && Bitbake::ends_with_bb_something(e)
        for entry in walker.filter_entry(|_e| true) {
            info!("Found entry: {:?}", entry);
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    info!("Found BitBake manifest: {}", path.display());
                    match std::fs::read_to_string(path) {
                        Ok(c) => {
                            Self::parse_bitbake(
                                c.as_str(),
                                path.file_name().unwrap().to_str().unwrap(),
                            );
                        }
                        Err(e) => {
                            warn!("Failed to read file {}: {}", path.display(), e);
                            continue;
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

    pub fn parse_bitbake(code: &str, filename: &str) {
        let mut parser = Parser::new();
        parser
            .set_language(tree_sitter_bitbake::language())
            .expect("Error loading BitBake grammar");
        let tree = parser.parse(code, None).unwrap();
        let mut cursor = Tree::walk(&tree);

        Self::walk(&mut cursor, code.as_bytes(), filename);
        info!("{} {:?}", filename, tree);

        // Create a query to match all function declarations
        let query_source = "(variable_assignment (identifier) @name (literal) @value)";
        let query = Query::new(tree_sitter_bitbake::language(), query_source)
            .expect("Error creating query");
        let mut query_cursor = QueryCursor::new();
        let query_matches = query_cursor.matches(&query, tree.root_node(), code.as_bytes());

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
            }
        }
    }

    fn walk(cursor: &mut TreeCursor, text: &[u8], filename: &str) {
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
    fn kas_manifest_find() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let manifests = Bitbake::find_bitbake_manifest(d.as_path());
        assert_eq!(manifests.len(), 0);
    }
}
