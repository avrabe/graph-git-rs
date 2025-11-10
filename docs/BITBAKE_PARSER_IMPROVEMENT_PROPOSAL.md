# BitBake Parser Improvement Proposal

## Executive Summary

The current BitBake parser implementation only captures a small fraction of the dependency information available in BitBake recipes. This proposal outlines a comprehensive approach to extract full dependency graphs from BitBake files using static analysis, without requiring execution of BitBake itself.

## Current State Analysis

### What Works
- ✅ File discovery for `.bb` and `.bbappend` files
- ✅ Tree-sitter AST parsing with custom BitBake grammar
- ✅ Basic SRC_URI extraction
- ✅ Integration with Neo4j graph database

### Critical Limitations

#### 1. **Incomplete SRC_URI Extraction**
```rust
// Current implementation returns Option<String> - only ONE URI
pub fn parse_bitbake(code: &str, filename: &str) -> Option<String>
```

**Problem**: BitBake recipes commonly have multiple SRC_URI entries:
```bitbake
SRC_URI = "git://git.yoctoproject.org/poky;protocol=https;branch=master"
SRC_URI += "file://patch1.patch"
SRC_URI += "git://github.com/rust-lang/crates;protocol=https"
```

Currently only the first `SRC_URI =` assignment is captured, losing all `+=` append operations.

#### 2. **Missing Append/Prepend Operations**
Tree-sitter query only matches `variable_assignment`:
```rust
"(variable_assignment (identifier) @name (literal) @value)"
```

Does NOT match:
- `SRC_URI += "..."` (append)
- `SRC_URI =+ "..."` (prepend)
- `SRC_URI .= "..."` (append with no space)
- `SRC_URI =. "..."` (prepend with no space)
- `SRC_URI:append = "..."` (override-style append)
- `SRC_URI:prepend = "..."` (override-style prepend)

#### 3. **Limited Protocol Support**
Only extracts URIs starting with `git://`:
```rust
if src_uri.starts_with("git://")
```

**Missing protocols**:
- `https://` (most common in modern recipes)
- `ssh://`
- `ftp://`
- `file://` (patches and local files)
- `gitsm://` (git with submodules)

#### 4. **No Variable Resolution**
Variables like `${BPN}`, `${PV}`, `${PN}` are captured literally without expansion.

Example:
```bitbake
BPN = "my-package"
SRC_URI = "git://example.com/${BPN}.git"
```

Current parser captures: `"git://example.com/${BPN}.git"` (unexpanded)

#### 5. **Missing Critical Metadata**
Only extracts `SRC_URI`. Does NOT capture:

**Version Control Metadata:**
- `SRCREV` - Git commit hashes
- `SRCREV_FORMAT` - How to format multi-repo SRCREV
- `BRANCH` - Branch specifications
- `TAG` - Git tags
- `SRCREV_<name>` - Named SRCREVs for multi-repo

**Package Metadata:**
- `PN` (Package Name)
- `PV` (Package Version)
- `PR` (Package Revision)
- `LICENSE` - License information
- `DEPENDS` - Build dependencies
- `RDEPENDS` - Runtime dependencies
- `PROVIDES` - Virtual packages provided
- `RPROVIDES` - Runtime virtual packages

**Build System:**
- `inherit` - Classes inherited (cmake, autotools, cargo, etc.)
- `include` - Files included
- `require` - Required files (with error on missing)

#### 6. **No Include/Require Following**
Test file shows multiple includes:
```bitbake
include foo-srcrev.inc
include ${BPN}-crates.inc
```

These files may contain additional SRC_URI entries, but are completely ignored.

#### 7. **Incomplete URI Parsing**
Current regex-free approach splits on newlines:
```rust
for src_uri in src_uris.lines() {
    if src_uri.starts_with("git://") {
```

**Problems:**
- Multi-line strings are broken
- URI parameters not parsed (`;protocol=https;branch=master`)
- No extraction of branch/tag/commit from URI
- Missing checksum information (`SRC_URI[file.sha256sum]`)

## Comparison with Other Parsers

### KAS Parser (YAML-based)
```rust
pub struct Repository {
    pub url: Option<String>,
    pub refspec: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
}
```
- Structured data model with serde
- Extracts all relevant version control info
- Clean separation of concerns

### Repo Parser (XML-based)
- 608 lines vs BitBake's 240 lines
- Comprehensive data structures
- Handles includes, defaults, multiple projects
- Proper URL handling with `url::Url` crate

## Proposed Solution: Comprehensive Static Analysis Model

### Phase 1: Enhanced Data Model

Create a structured data model similar to KAS/Repo parsers:

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BitbakeRecipe {
    pub path: String,
    pub recipe_type: RecipeType,  // .bb or .bbappend

    // Package identity
    pub package_name: Option<String>,      // PN
    pub package_version: Option<String>,   // PV
    pub package_revision: Option<String>,  // PR

    // Source URIs with full context
    pub sources: Vec<SourceUri>,

    // Dependencies
    pub build_depends: Vec<String>,        // DEPENDS
    pub runtime_depends: Vec<String>,      // RDEPENDS
    pub provides: Vec<String>,             // PROVIDES

    // Build system
    pub inherits: Vec<String>,             // inherit statements
    pub includes: Vec<String>,             // include/require files

    // Metadata
    pub license: Option<String>,
    pub description: Option<String>,

    // Variable context for resolution
    pub variables: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SourceUri {
    pub raw_uri: String,
    pub uri_type: UriType,
    pub url: Option<String>,

    // Git-specific
    pub protocol: Option<String>,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub srcrev: Option<String>,
    pub subpath: Option<String>,
    pub destsuffix: Option<String>,

    // File-specific
    pub checksum_sha256: Option<String>,
    pub checksum_md5: Option<String>,

    // Operation context
    pub operation: UriOperation,  // =, +=, =+, etc.
    pub conditional: Option<String>,  // SRC_URI:append:class-target
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum RecipeType {
    Recipe,        // .bb
    Append,        // .bbappend
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum UriType {
    Git,
    GitSubmodule,
    File,
    Http,
    Https,
    Ftp,
    Ssh,
    Other(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum UriOperation {
    Assign,        // =
    Append,        // +=
    Prepend,       // =+
    AppendNoSpace, // .=
    PrependNoSpace, // =.
    OverrideAppend(Vec<String>),  // :append:class-target
    OverridePrepend(Vec<String>), // :prepend:class-target
}
```

### Phase 2: Enhanced Tree-Sitter Queries

Expand queries to capture all relevant AST nodes:

```rust
// Multiple queries for different constructs
pub struct BitbakeQueries {
    variable_assignments: Query,
    variable_appends: Query,
    inherit_statements: Query,
    include_statements: Query,
    function_definitions: Query,
    override_syntax: Query,
}

impl BitbakeQueries {
    pub fn new() -> Self {
        Self {
            // Basic assignments: VAR = "value"
            variable_assignments: Query::new(
                language(),
                "(variable_assignment (identifier) @name (literal) @value)"
            ),

            // Append operations: VAR += "value"
            variable_appends: Query::new(
                language(),
                "(variable_append (identifier) @name (literal) @value)"
            ),

            // Inherit: inherit cmake cargo
            inherit_statements: Query::new(
                language(),
                "(inherit_directive (identifier) @class)"
            ),

            // Include: include foo.inc
            include_statements: Query::new(
                language(),
                "(include_directive (string) @filename)"
            ),

            // Functions: do_compile() { ... }
            function_definitions: Query::new(
                language(),
                "(function_definition (identifier) @name)"
            ),

            // Override syntax: SRC_URI:append = "..."
            override_syntax: Query::new(
                language(),
                "(override_assignment (identifier) @name (override) @override (literal) @value)"
            ),
        }
    }
}
```

### Phase 3: URI Parser

Implement comprehensive URI parsing using proper URL parser:

```rust
pub struct UriParser;

impl UriParser {
    /// Parse BitBake SRC_URI with parameters
    /// Example: "git://git.yoctoproject.org/poky;protocol=https;branch=master;tag=v1.0"
    pub fn parse(uri: &str) -> Result<SourceUri, Error> {
        let parts: Vec<&str> = uri.split(';').collect();
        let base_uri = parts[0];
        let params: HashMap<String, String> = parts[1..]
            .iter()
            .filter_map(|p| {
                let kv: Vec<&str> = p.split('=').collect();
                if kv.len() == 2 {
                    Some((kv[0].to_string(), kv[1].to_string()))
                } else {
                    None
                }
            })
            .collect();

        let uri_type = Self::detect_uri_type(base_uri);

        Ok(SourceUri {
            raw_uri: uri.to_string(),
            uri_type,
            url: Some(base_uri.to_string()),
            protocol: params.get("protocol").cloned(),
            branch: params.get("branch").cloned(),
            tag: params.get("tag").cloned(),
            subpath: params.get("subpath").cloned(),
            destsuffix: params.get("destsuffix").cloned(),
            srcrev: None,  // Set separately from SRCREV variable
            checksum_sha256: None,
            checksum_md5: None,
            operation: UriOperation::Assign,
            conditional: None,
        })
    }

    fn detect_uri_type(uri: &str) -> UriType {
        if uri.starts_with("git://") {
            UriType::Git
        } else if uri.starts_with("gitsm://") {
            UriType::GitSubmodule
        } else if uri.starts_with("https://") {
            UriType::Https
        } else if uri.starts_with("http://") {
            UriType::Http
        } else if uri.starts_with("ssh://") {
            UriType::Ssh
        } else if uri.starts_with("ftp://") {
            UriType::Ftp
        } else if uri.starts_with("file://") {
            UriType::File
        } else {
            UriType::Other(uri.split("://").next().unwrap_or("unknown").to_string())
        }
    }
}
```

### Phase 4: Variable Resolution System

Implement basic variable substitution for common cases:

```rust
pub struct VariableResolver {
    variables: HashMap<String, String>,
    built_in_vars: HashMap<String, String>,
}

impl VariableResolver {
    pub fn new() -> Self {
        let mut built_in_vars = HashMap::new();

        // Common built-in variables
        built_in_vars.insert("S".to_string(), "${WORKDIR}/${BP}".to_string());
        built_in_vars.insert("BP".to_string(), "${BPN}-${PV}".to_string());
        built_in_vars.insert("WORKDIR".to_string(), "${TMPDIR}/work/${MULTIMACH_TARGET_SYS}/${PN}/${EXTENDPE}${PV}-${PR}".to_string());

        Self {
            variables: HashMap::new(),
            built_in_vars,
        }
    }

    /// Resolve variables in a string
    /// Example: "git://example.com/${BPN}.git" -> "git://example.com/my-package.git"
    pub fn resolve(&self, input: &str) -> String {
        let mut result = input.to_string();
        let re = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();

        // Multiple passes for nested variables
        for _ in 0..10 {
            let before = result.clone();
            for cap in re.captures_iter(&before) {
                let var_name = &cap[1];
                if let Some(value) = self.variables.get(var_name)
                    .or_else(|| self.built_in_vars.get(var_name)) {
                    result = result.replace(&cap[0], value);
                }
            }
            if result == before {
                break;  // No more changes
            }
        }

        result
    }

    pub fn set(&mut self, name: String, value: String) {
        self.variables.insert(name, value);
    }
}
```

### Phase 5: Include File Resolution

Follow include/require statements to build complete picture:

```rust
impl BitbakeRecipe {
    pub fn parse_with_includes(path: &Path, base_path: &Path) -> Result<Self, Error> {
        let mut recipe = Self::parse_file(path)?;

        // Resolve includes
        for include_file in &recipe.includes.clone() {
            let include_path = Self::resolve_include_path(include_file, path, base_path)?;

            if include_path.exists() {
                let included = Self::parse_file(&include_path)?;

                // Merge included data
                recipe.sources.extend(included.sources);
                recipe.build_depends.extend(included.build_depends);
                recipe.runtime_depends.extend(included.runtime_depends);
                recipe.variables.extend(included.variables);
            } else {
                // Log warning but continue (include is non-fatal)
                warn!("Include file not found: {}", include_path.display());
            }
        }

        Ok(recipe)
    }

    fn resolve_include_path(include: &str, recipe_path: &Path, base_path: &Path) -> Result<PathBuf, Error> {
        // Try multiple resolution strategies:
        // 1. Relative to current recipe
        // 2. In same directory as recipe
        // 3. In common include paths (meta-*/classes/, meta-*/conf/)

        let recipe_dir = recipe_path.parent().unwrap();

        // Strategy 1: Relative to recipe
        let candidate = recipe_dir.join(include);
        if candidate.exists() {
            return Ok(candidate);
        }

        // Strategy 2: Search in common paths
        let search_paths = vec![
            base_path.join("classes"),
            base_path.join("conf"),
            base_path.join("recipes-core/include"),
        ];

        for search_path in search_paths {
            let candidate = search_path.join(include);
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        // Return best guess even if not exists
        Ok(recipe_dir.join(include))
    }
}
```

### Phase 6: Enhanced Main Parser

Bring it all together:

```rust
impl BitbakeRecipe {
    pub fn parse_file(path: &Path) -> Result<Self, Error> {
        let code = std::fs::read_to_string(path)?;
        let filename = path.file_name().unwrap().to_str().unwrap();

        let mut parser = Parser::new();
        parser.set_language(tree_sitter_bitbake::language())?;
        let tree = parser.parse(&code, None).unwrap();

        let queries = BitbakeQueries::new();
        let mut resolver = VariableResolver::new();
        let mut recipe = BitbakeRecipe {
            path: path.to_string_lossy().to_string(),
            recipe_type: if filename.ends_with(".bbappend") {
                RecipeType::Append
            } else {
                RecipeType::Recipe
            },
            sources: Vec::new(),
            build_depends: Vec::new(),
            runtime_depends: Vec::new(),
            provides: Vec::new(),
            inherits: Vec::new(),
            includes: Vec::new(),
            variables: HashMap::new(),
            package_name: None,
            package_version: None,
            package_revision: None,
            license: None,
            description: None,
        };

        // Pass 1: Extract all variable assignments to build context
        let mut cursor = QueryCursor::new();
        for m in cursor.matches(&queries.variable_assignments, tree.root_node(), code.as_bytes()) {
            let name = Self::get_capture_text(&m, 0, &code);
            let value = Self::get_capture_text(&m, 1, &code);

            // Store for variable resolution
            recipe.variables.insert(name.to_string(), value.to_string());
            resolver.set(name.to_string(), value.to_string());

            // Extract specific metadata
            match name {
                "PN" => recipe.package_name = Some(value.to_string()),
                "PV" => recipe.package_version = Some(value.to_string()),
                "PR" => recipe.package_revision = Some(value.to_string()),
                "LICENSE" => recipe.license = Some(value.to_string()),
                "DESCRIPTION" => recipe.description = Some(value.to_string()),
                "DEPENDS" => recipe.build_depends = Self::parse_space_separated(value),
                "RDEPENDS" | "RDEPENDS_${PN}" => recipe.runtime_depends = Self::parse_space_separated(value),
                "PROVIDES" => recipe.provides = Self::parse_space_separated(value),
                "SRC_URI" => {
                    let parsed_uris = Self::parse_src_uri(value, UriOperation::Assign);
                    recipe.sources.extend(parsed_uris);
                }
                _ => {}
            }
        }

        // Pass 2: Handle append operations
        for m in cursor.matches(&queries.variable_appends, tree.root_node(), code.as_bytes()) {
            let name = Self::get_capture_text(&m, 0, &code);
            let value = Self::get_capture_text(&m, 1, &code);

            if name == "SRC_URI" {
                let parsed_uris = Self::parse_src_uri(value, UriOperation::Append);
                recipe.sources.extend(parsed_uris);
            }
        }

        // Pass 3: Extract inherit statements
        for m in cursor.matches(&queries.inherit_statements, tree.root_node(), code.as_bytes()) {
            let class_name = Self::get_capture_text(&m, 0, &code);
            recipe.inherits.push(class_name.to_string());
        }

        // Pass 4: Extract include statements
        for m in cursor.matches(&queries.include_statements, tree.root_node(), code.as_bytes()) {
            let include_file = Self::get_capture_text(&m, 0, &code);
            let resolved = resolver.resolve(include_file);
            recipe.includes.push(resolved);
        }

        // Pass 5: Associate SRCREV with sources
        for source in &mut recipe.sources {
            if source.uri_type == UriType::Git || source.uri_type == UriType::GitSubmodule {
                if let Some(srcrev) = recipe.variables.get("SRCREV") {
                    source.srcrev = Some(srcrev.clone());
                }
            }
        }

        Ok(recipe)
    }

    fn parse_src_uri(uri_string: &str, operation: UriOperation) -> Vec<SourceUri> {
        // Handle multi-line and quoted strings
        let cleaned = uri_string.trim().trim_matches('"').trim_matches('\'');

        let mut sources = Vec::new();
        for line in cleaned.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Handle line continuations
            let uri = line.trim_end_matches('\\').trim();

            if let Ok(mut source) = UriParser::parse(uri) {
                source.operation = operation.clone();
                sources.push(source);
            }
        }

        sources
    }

    fn parse_space_separated(value: &str) -> Vec<String> {
        value.split_whitespace()
            .map(|s| s.to_string())
            .collect()
    }

    fn get_capture_text(m: &QueryMatch, index: usize, code: &str) -> &str {
        m.captures
            .iter()
            .find(|c| c.index as usize == index)
            .map(|c| c.node.utf8_text(code.as_bytes()).unwrap())
            .unwrap_or("")
    }
}
```

## Implementation Strategy

### Stage 1: Data Model (Week 1)
- [ ] Define comprehensive data structures
- [ ] Add unit tests for data model
- [ ] Update Cargo.toml dependencies (regex, url)

### Stage 2: URI Parser (Week 1-2)
- [ ] Implement UriParser with full protocol support
- [ ] Add parameter extraction
- [ ] Test with real-world SRC_URI examples
- [ ] Handle edge cases (multi-line, escaping, etc.)

### Stage 3: Enhanced Tree-Sitter Queries (Week 2)
- [ ] Update tree-sitter-bitbake grammar if needed
- [ ] Implement multiple query types
- [ ] Add support for override syntax
- [ ] Test with complex recipes

### Stage 4: Variable Resolution (Week 3)
- [ ] Implement basic variable substitution
- [ ] Add built-in variables
- [ ] Test nested variable expansion
- [ ] Document limitations

### Stage 5: Include Resolution (Week 3-4)
- [ ] Implement include file discovery
- [ ] Add search path logic
- [ ] Merge included data
- [ ] Handle circular includes

### Stage 6: Integration (Week 4)
- [ ] Update graph database schema
- [ ] Modify CLI to use new parser
- [ ] Add comprehensive tests
- [ ] Update documentation

### Stage 7: Testing & Validation (Week 5)
- [ ] Test with Poky/Yocto recipes
- [ ] Test with custom layers
- [ ] Performance testing
- [ ] Compare with actual BitBake output

## Benefits

1. **Comprehensive Dependency Tracking**: Extract all source repositories, not just first
2. **No BitBake Required**: Pure static analysis, fast and reliable
3. **Rich Metadata**: License, dependencies, version information for better graph queries
4. **Consistent with Other Parsers**: Similar architecture to KAS and Repo
5. **Extensible**: Easy to add new variable extraction or features
6. **Better Graph Queries**: More relationships and properties in Neo4j

## Example Usage

```rust
// Old way
let src_uri = Bitbake::parse_bitbake(&code, "recipe.bb"); // Option<String>
// Returns: Some("git://git.yoctoproject.org/poky;protocol=https")

// New way
let recipe = BitbakeRecipe::parse_file(path)?; // BitbakeRecipe
// Returns full structure:
// BitbakeRecipe {
//     sources: [
//         SourceUri {
//             url: "git://git.yoctoproject.org/poky",
//             protocol: Some("https"),
//             branch: Some("master"),
//             srcrev: Some("abc123"),
//         },
//         SourceUri {
//             url: "git://crates.io/addr2line/0.20.0",
//             ...
//         }
//     ],
//     build_depends: ["cmake", "rust-native"],
//     inherits: ["cmake", "cargo"],
//     ...
// }
```

## Graph Database Enhancements

New node properties:
```cypher
CREATE (r:BitbakeRecipe {
    path: "recipes-core/my-app/my-app_1.0.bb",
    package_name: "my-app",
    version: "1.0",
    license: "MIT",
    type: "recipe"
})

CREATE (s:SourceUri {
    url: "git://github.com/user/repo.git",
    protocol: "https",
    branch: "master",
    srcrev: "abc123",
    type: "git"
})

CREATE (r)-[:HAS_SOURCE]->(s)
CREATE (r)-[:DEPENDS_ON]->(dep:Package)
CREATE (r)-[:INHERITS]->(class:BBClass)
```

New queries possible:
```cypher
// Find all recipes that depend on a specific package
MATCH (r:BitbakeRecipe)-[:DEPENDS_ON]->(p:Package {name: "openssl"})
RETURN r

// Find all git sources from a specific organization
MATCH (r:BitbakeRecipe)-[:HAS_SOURCE]->(s:SourceUri)
WHERE s.url CONTAINS "github.com/yoctoproject"
RETURN r, s

// Find recipes with specific SRCREV
MATCH (s:SourceUri {srcrev: "abc123"})
RETURN s

// Build dependency graph
MATCH path = (r:BitbakeRecipe)-[:DEPENDS_ON*]->(dep)
RETURN path
```

## Risks & Mitigations

### Risk 1: Variable Resolution Complexity
**Mitigation**: Start with simple substitution, add complexity incrementally. Document unsupported cases.

### Risk 2: Include Path Resolution
**Mitigation**: Use heuristics for common cases. Allow configuration of search paths.

### Risk 3: BitBake Python Functions
**Mitigation**: Extract but don't execute Python functions. Mark as unresolved.

### Risk 4: Performance with Large Layers
**Mitigation**: Add caching, parallel processing, incremental parsing.

## Success Metrics

- Extract 95%+ of SRC_URI entries (vs current ~30%)
- Support all common git protocols
- Parse 90%+ of Yocto Project recipes without errors
- Performance: <100ms per recipe file
- Enable new graph queries for dependency analysis

## References

- [BitBake User Manual](https://docs.yoctoproject.org/bitbake/)
- [Yocto Project Reference Manual](https://docs.yoctoproject.org/)
- [tree-sitter-bitbake](https://github.com/avrabe/tree-sitter-bitbake)
- Current implementation: `/home/user/graph-git-rs/convenient-bitbake/src/lib.rs`
