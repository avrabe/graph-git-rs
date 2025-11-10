// Convenient BitBake parser - Rowan-based resilient implementation
// This module provides parsing and analysis of BitBake files (.bb, .bbappend, .inc)

pub mod syntax_kind;
pub mod lexer;
pub mod parser;

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::{Path, PathBuf}};
use tracing::{info, warn};
use walkdir::{DirEntry, WalkDir};

// Re-export public types
pub use parser::{parse, Parse, ParseError};
pub use syntax_kind::{SyntaxKind, SyntaxNode};

// === Data Models ===

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Default)]
pub struct Bitbake {
    pub path: String,
    pub src_uris: Vec<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct BitbakeRecipe {
    pub file_path: PathBuf,
    pub recipe_type: RecipeType,

    // Package metadata
    pub package_name: Option<String>,
    pub package_version: Option<String>,
    pub summary: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,

    // Sources
    pub sources: Vec<SourceUri>,

    // Dependencies
    pub build_depends: Vec<String>,
    pub runtime_depends: Vec<String>,

    // Build system
    pub inherits: Vec<String>,
    pub includes: Vec<IncludeDirective>,

    // All variables
    pub variables: HashMap<String, String>,

    // Parse information
    pub parse_errors: Vec<String>,
    pub parse_warnings: Vec<String>,
}

impl Default for BitbakeRecipe {
    fn default() -> Self {
        Self {
            file_path: PathBuf::new(),
            recipe_type: RecipeType::Recipe,
            package_name: None,
            package_version: None,
            summary: None,
            homepage: None,
            license: None,
            sources: Vec::new(),
            build_depends: Vec::new(),
            runtime_depends: Vec::new(),
            inherits: Vec::new(),
            includes: Vec::new(),
            variables: HashMap::new(),
            parse_errors: Vec::new(),
            parse_warnings: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum RecipeType {
    Recipe,
    Append,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct SourceUri {
    pub raw: String,
    pub scheme: UriScheme,
    pub url: String,
    pub protocol: Option<String>,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub srcrev: Option<String>,
    pub nobranch: bool,
    pub destsuffix: Option<String>,
}

impl Default for SourceUri {
    fn default() -> Self {
        Self {
            raw: String::new(),
            scheme: UriScheme::Other("unknown".to_string()),
            url: String::new(),
            protocol: None,
            branch: None,
            tag: None,
            srcrev: None,
            nobranch: false,
            destsuffix: None,
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum UriScheme {
    File,
    Http,
    Https,
    Git,
    GitSubmodule,
    Crate,
    Other(String),
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct IncludeDirective {
    pub path: String,
    pub required: bool,
}

// === Implementation ===

impl BitbakeRecipe {
    pub fn new(file_path: PathBuf) -> Self {
        let recipe_type = if file_path.extension().and_then(|s| s.to_str()) == Some("bbappend") {
            RecipeType::Append
        } else {
            RecipeType::Recipe
        };

        Self {
            file_path,
            recipe_type,
            ..Default::default()
        }
    }

    /// Parse a BitBake file from disk
    pub fn parse_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        Self::parse_string(&content, path)
    }

    /// Parse BitBake content from string
    pub fn parse_string(content: &str, path: &Path) -> Result<Self, String> {
        let parse = parser::parse(content);
        let mut recipe = BitbakeRecipe::new(path.to_path_buf());

        // Store parse errors
        for error in &parse.errors {
            recipe.parse_errors.push(error.message.clone());
        }

        // Extract data from CST
        let root = parse.syntax();
        extract_from_cst(&root, &mut recipe);

        Ok(recipe)
    }
}

/// Extract data from Concrete Syntax Tree
fn extract_from_cst(node: &SyntaxNode, recipe: &mut BitbakeRecipe) {
    for child in node.descendants() {
        match child.kind() {
            SyntaxKind::VARIABLE_ASSIGNMENT => {
                extract_variable_assignment(&child, recipe);
            }
            SyntaxKind::INHERIT_STMT => {
                extract_inherit(&child, recipe);
            }
            SyntaxKind::INCLUDE_STMT => {
                extract_include(&child, recipe, false);
            }
            SyntaxKind::REQUIRE_STMT => {
                extract_include(&child, recipe, true);
            }
            _ => {}
        }
    }

    // Post-processing: parse SRC_URI values
    if let Some(src_uri_str) = recipe.variables.get("SRC_URI") {
        match parse_src_uri_value(src_uri_str) {
            Ok(sources) => recipe.sources.extend(sources),
            Err(e) => recipe.parse_warnings.push(format!("Failed to parse SRC_URI: {}", e)),
        }
    }

    // Extract SRCREV and associate with git sources
    if let Some(srcrev) = recipe.variables.get("SRCREV").cloned() {
        for source in &mut recipe.sources {
            if matches!(source.scheme, UriScheme::Git | UriScheme::GitSubmodule) {
                source.srcrev = Some(srcrev.clone());
            }
        }
    }

    // Extract known metadata variables
    if let Some(pn) = recipe.variables.get("PN") {
        recipe.package_name = Some(pn.clone());
    }
    if let Some(pv) = recipe.variables.get("PV") {
        recipe.package_version = Some(pv.clone());
    }
    if let Some(summary) = recipe.variables.get("SUMMARY") {
        recipe.summary = Some(summary.clone());
    }
    if let Some(homepage) = recipe.variables.get("HOMEPAGE") {
        recipe.homepage = Some(homepage.clone());
    }
    if let Some(license) = recipe.variables.get("LICENSE") {
        recipe.license = Some(license.clone());
    }

    // Extract DEPENDS
    if let Some(depends_str) = recipe.variables.get("DEPENDS") {
        recipe.build_depends = depends_str.split_whitespace()
            .map(|s| s.to_string())
            .collect();
    }
}

fn extract_variable_assignment(node: &SyntaxNode, recipe: &mut BitbakeRecipe) {
    let mut var_name = None;
    let mut var_value = None;

    for child in node.children() {
        match child.kind() {
            SyntaxKind::VARIABLE_NAME => {
                // Get the identifier text
                let tokens: Vec<_> = child.descendants_with_tokens()
                    .filter_map(|elem| elem.as_token())
                    .collect();
                var_name = tokens.iter()
                    .find(|token| token.kind() == SyntaxKind::IDENT)
                    .map(|token| token.text().to_string());
            }
            SyntaxKind::VARIABLE_VALUE => {
                // Concatenate all text in value
                let tokens: Vec<_> = child.descendants_with_tokens()
                    .filter_map(|elem| elem.as_token())
                    .collect();
                var_value = Some(
                    tokens.iter()
                        .filter(|token| !token.kind().is_trivia())
                        .map(|token| token.text())
                        .collect::<String>()
                );
            }
            _ => {}
        }
    }

    if let (Some(name), Some(value)) = (var_name, var_value) {
        let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
        recipe.variables.insert(name, value);
    }
}

fn extract_inherit(node: &SyntaxNode, recipe: &mut BitbakeRecipe) {
    for token in node.descendants_with_tokens() {
        if let Some(token) = token.as_token() {
            if token.kind() == SyntaxKind::IDENT &&
               token.text() != "inherit" {
                recipe.inherits.push(token.text().to_string());
            }
        }
    }
}

fn extract_include(node: &SyntaxNode, recipe: &mut BitbakeRecipe, required: bool) {
    for token in node.descendants_with_tokens() {
        if let Some(token) = token.as_token() {
            if matches!(token.kind(), SyntaxKind::IDENT | SyntaxKind::STRING | SyntaxKind::VAR_EXPANSION) &&
               token.text() != "include" && token.text() != "require" {
                let path = token.text().trim_matches('"').trim_matches('\'').to_string();
                recipe.includes.push(IncludeDirective { path, required });
            }
        }
    }
}

/// Parse SRC_URI value which can contain multiple URIs
fn parse_src_uri_value(value: &str) -> Result<Vec<SourceUri>, String> {
    let mut sources = Vec::new();

    // Handle multi-line strings with backslash continuation
    let cleaned = value.replace("\\\n", " ").replace("\\", "");

    // Split on whitespace but respect quotes
    let mut current_uri = String::new();
    let mut in_quotes = false;

    for ch in cleaned.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current_uri.push(ch);
            }
            ' ' | '\t' | '\n' if !in_quotes => {
                if !current_uri.trim().is_empty() {
                    if let Ok(source) = parse_single_uri(&current_uri.trim()) {
                        sources.push(source);
                    }
                    current_uri.clear();
                }
            }
            _ => {
                current_uri.push(ch);
            }
        }
    }

    // Don't forget the last URI
    if !current_uri.trim().is_empty() {
        if let Ok(source) = parse_single_uri(&current_uri.trim()) {
            sources.push(source);
        }
    }

    Ok(sources)
}

/// Parse a single URI with parameters
fn parse_single_uri(uri: &str) -> Result<SourceUri, String> {
    let uri = uri.trim().trim_matches('"').trim_matches('\'');

    // Split URI and parameters at first semicolon
    let parts: Vec<&str> = uri.splitn(2, ';').collect();
    let base_uri = parts[0];
    let params_str = parts.get(1).unwrap_or(&"");

    // Detect scheme
    let scheme = detect_uri_scheme(base_uri);

    // Parse parameters
    let mut source = SourceUri {
        raw: uri.to_string(),
        scheme: scheme.clone(),
        url: base_uri.to_string(),
        ..Default::default()
    };

    // Parse parameters (key=value pairs separated by semicolons)
    for param in params_str.split(';') {
        let kv: Vec<&str> = param.splitn(2, '=').collect();
        if kv.len() == 2 {
            let key = kv[0].trim();
            let value = kv[1].trim();

            match key {
                "protocol" => source.protocol = Some(value.to_string()),
                "branch" => source.branch = Some(value.to_string()),
                "tag" => source.tag = Some(value.to_string()),
                "nobranch" => source.nobranch = value == "1",
                "destsuffix" => source.destsuffix = Some(value.to_string()),
                _ => {}
            }
        }
    }

    Ok(source)
}

fn detect_uri_scheme(uri: &str) -> UriScheme {
    if uri.starts_with("git://") {
        UriScheme::Git
    } else if uri.starts_with("gitsm://") {
        UriScheme::GitSubmodule
    } else if uri.starts_with("https://") {
        UriScheme::Https
    } else if uri.starts_with("http://") {
        UriScheme::Http
    } else if uri.starts_with("file://") {
        UriScheme::File
    } else if uri.starts_with("crate://") {
        UriScheme::Crate
    } else {
        UriScheme::Other(
            uri.split("://").next().unwrap_or("unknown").to_string()
        )
    }
}

// === Layer discovery functions ===

impl Bitbake {
    pub fn new(path: String) -> Self {
        Bitbake {
            path,
            src_uris: Vec::new(),
        }
    }

    fn ends_with_bb_something(entry: &DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.ends_with(".bb") || s.ends_with(".bbappend"))
            .unwrap_or(false)
            || entry.path().is_dir()
    }

    pub fn find_bitbake_manifest(path: &Path) -> Vec<Bitbake> {
        info!("Searching for BitBake manifests in {}", path.display());
        let mut bitbake_manifests = Vec::new();

        let walker = WalkDir::new(path).follow_links(true).into_iter();
        let walker = walker.filter_entry(|e: &DirEntry| Self::ends_with_bb_something(e));

        for entry in walker {
            match entry {
                Ok(entry) => {
                    if entry.path().is_dir() {
                        continue;
                    }

                    let bitbake_path = entry.path();
                    info!("Found BitBake manifest: {}", bitbake_path.display());

                    match BitbakeRecipe::parse_file(bitbake_path) {
                        Ok(recipe) => {
                            let relative_path = bitbake_path.strip_prefix(path)
                                .unwrap_or(bitbake_path)
                                .to_str()
                                .unwrap()
                                .to_string();

                            let mut bitbake = Bitbake::new(relative_path);

                            // Extract git URIs
                            for source in &recipe.sources {
                                if matches!(source.scheme, UriScheme::Git | UriScheme::GitSubmodule | UriScheme::Https | UriScheme::Http) {
                                    bitbake.src_uris.push(source.url.clone());
                                }
                            }

                            if !bitbake.src_uris.is_empty() {
                                bitbake_manifests.push(bitbake);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse {}: {}", bitbake_path.display(), e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Error reading directory entry: {}", e);
                }
            }
        }

        bitbake_manifests
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_parse_simple_assignment() {
        let input = r#"FOO = "bar""#;
        let recipe = BitbakeRecipe::parse_string(input, Path::new("test.bb")).unwrap();

        assert_eq!(recipe.variables.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_parse_src_uri() {
        let input = r#"SRC_URI = "git://github.com/user/repo.git;protocol=https;branch=main""#;
        let recipe = BitbakeRecipe::parse_string(input, Path::new("test.bb")).unwrap();

        assert_eq!(recipe.sources.len(), 1);
        assert_eq!(recipe.sources[0].scheme, UriScheme::Git);
        assert_eq!(recipe.sources[0].protocol, Some("https".to_string()));
        assert_eq!(recipe.sources[0].branch, Some("main".to_string()));
    }

    #[test]
    fn test_parse_multiple_src_uri() {
        let input = r#"SRC_URI = "git://github.com/user/repo1.git file://patch.patch crate://crates.io/foo""#;
        let recipe = BitbakeRecipe::parse_string(input, Path::new("test.bb")).unwrap();

        assert!(recipe.sources.len() >= 2, "Should find multiple URIs");
    }

    #[test]
    fn test_parse_inherit() {
        let input = "inherit cmake cargo";
        let recipe = BitbakeRecipe::parse_string(input, Path::new("test.bb")).unwrap();

        assert!(recipe.inherits.contains(&"cmake".to_string()));
        assert!(recipe.inherits.contains(&"cargo".to_string()));
    }

    #[test]
    fn test_parse_include() {
        let input = "include ${BPN}-crates.inc";
        let recipe = BitbakeRecipe::parse_string(input, Path::new("test.bb")).unwrap();

        assert_eq!(recipe.includes.len(), 1);
        assert!(recipe.includes[0].path.contains("crates.inc"));
    }

    #[test]
    fn test_meta_fmu_recipe_parsing() {
        let content = r#"
SUMMARY = "fmu-rs is a Rust implementation"
HOMEPAGE = "https://github.com/avrabe/fmu-rs.git"
LICENSE = "MIT"

inherit cargo cargo-update-recipe-crates

SRC_URI = "git://github.com/avrabe/fmu-rs;protocol=https;nobranch=1;branch=main"
include ${BPN}-crates.inc

S = "${WORKDIR}/git"

include ${BPN}-srcrev.inc
PV:append = ".AUTOINC+${SRCPV}"
DEPENDS:append = " ostree openssl pkgconfig-native "
        "#;

        let recipe = BitbakeRecipe::parse_string(content, Path::new("fmu-rs_0.2.0.bb")).unwrap();

        assert_eq!(recipe.summary, Some("fmu-rs is a Rust implementation".to_string()));
        assert_eq!(recipe.license, Some("MIT".to_string()));
        assert!(recipe.inherits.len() >= 2);
        assert!(recipe.sources.len() >= 1);
        assert!(recipe.includes.len() >= 2);
    }
}
