# BitBake Parser Implementation Roadmap

## Executive Summary

Based on comprehensive analysis of the BitBake specification and the current implementation, this document provides a complete roadmap for implementing a full-featured static analyzer that can extract comprehensive dependency information from BitBake metadata without requiring BitBake runtime execution.

## Current State vs. Spec Requirements

### Gap Analysis

| Feature | Current | Required | Priority |
|---------|---------|----------|----------|
| File types supported | .bb, .bbappend | .bb, .bbappend, .inc, .conf, .bbclass | **HIGH** |
| SRC_URI extraction | First only | All entries | **CRITICAL** |
| Operators supported | `=` only | `=`, `:=`, `+=`, `=+`, `.=`, `=.`, `?=`, `??=` | **HIGH** |
| Override syntax | Not supported | `:append`, `:prepend`, `:remove`, `:override` | **MEDIUM** |
| Git protocols | `git://` only | All (https, ssh, gitsm, etc.) | **HIGH** |
| Include/require | Not followed | Parse recursively | **HIGH** |
| Inherit | Not tracked | Track and index classes | **MEDIUM** |
| SRCREV | Not extracted | Link to SRC_URI | **HIGH** |
| Dependencies | Not extracted | DEPENDS, RDEPENDS, RPROVIDES, etc. | **HIGH** |
| Variable resolution | None | Basic substitution | **MEDIUM** |
| Checksums | Not extracted | md5sum, sha256sum | **LOW** |
| Layer structure | Not considered | Layer-aware parsing | **MEDIUM** |

## Architecture Overview

### Component Structure

```
convenient-bitbake/
├── src/
│   ├── lib.rs              # Public API
│   ├── models/             # Data models
│   │   ├── mod.rs
│   │   ├── recipe.rs       # Recipe (.bb)
│   │   ├── append.rs       # Append (.bbappend)
│   │   ├── config.rs       # Config (.conf)
│   │   ├── class.rs        # Class (.bbclass)
│   │   ├── include.rs      # Include (.inc)
│   │   ├── source.rs       # SourceUri model
│   │   └── layer.rs        # Layer structure
│   ├── parsers/            # Parsing logic
│   │   ├── mod.rs
│   │   ├── ast.rs          # Tree-sitter AST walker
│   │   ├── queries.rs      # Tree-sitter queries
│   │   ├── uri.rs          # SRC_URI parser
│   │   ├── variable.rs     # Variable resolver
│   │   └── operators.rs    # Operator handling
│   ├── discovery/          # File discovery
│   │   ├── mod.rs
│   │   ├── layer.rs        # Layer discovery
│   │   └── files.rs        # File finder
│   ├── graph/              # Graph building
│   │   ├── mod.rs
│   │   ├── builder.rs      # Graph construction
│   │   └── relationships.rs # Relationship types
│   └── error.rs            # Error types
└── tests/
    ├── fixtures/           # Test BitBake files
    │   ├── layers/
    │   │   └── meta-test/
    │   │       ├── conf/
    │   │       │   └── layer.conf
    │   │       ├── classes/
    │   │       │   └── test.bbclass
    │   │       └── recipes-test/
    │   │           └── myapp/
    │   │               ├── myapp_1.0.bb
    │   │               ├── myapp.inc
    │   │               └── myapp_1.0.bbappend
    └── integration/
        ├── parsing_tests.rs
        ├── uri_tests.rs
        └── layer_tests.rs
```

## Implementation Phases

### Phase 1: Foundation (Week 1-2)

**Goal**: Establish comprehensive data models and error handling

#### 1.1 Data Models

**Priority: CRITICAL**

Create complete data structures for all BitBake file types:

```rust
// models/recipe.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitbakeRecipe {
    pub file_path: PathBuf,
    pub recipe_type: RecipeType,
    pub layer: Option<String>,

    // Package identity
    pub package_name: Option<String>,      // PN
    pub package_version: Option<String>,   // PV
    pub package_revision: Option<String>,  // PR
    pub base_package_name: Option<String>, // BPN (without version)

    // Metadata
    pub summary: Option<String>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub section: Option<String>,
    pub license: Option<String>,
    pub license_files: Vec<LicenseFile>,

    // Sources
    pub sources: Vec<SourceUri>,
    pub source_dir: Option<String>,        // S variable

    // Dependencies
    pub build_depends: Vec<Dependency>,    // DEPENDS
    pub runtime_depends: Vec<Dependency>,  // RDEPENDS
    pub runtime_recommends: Vec<Dependency>, // RRECOMMENDS
    pub runtime_suggests: Vec<Dependency>, // RSUGGESTS
    pub provides: Vec<String>,             // PROVIDES/RPROVIDES
    pub conflicts: Vec<String>,            // RCONFLICTS
    pub replaces: Vec<String>,             // RREPLACES

    // Build system
    pub inherits: Vec<ClassInherit>,
    pub includes: Vec<IncludeDirective>,
    pub requires: Vec<RequireDirective>,

    // Variables (for resolution and graph metadata)
    pub variables: HashMap<String, VariableAssignment>,

    // Functions (detection only, not execution)
    pub shell_functions: Vec<ShellFunction>,
    pub python_functions: Vec<PythonFunction>,

    // Parsing metadata
    pub parse_errors: Vec<ParseError>,
    pub parse_warnings: Vec<ParseWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecipeType {
    Recipe,        // .bb
    Append,        // .bbappend
}

// models/source.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceUri {
    pub raw: String,
    pub scheme: UriScheme,
    pub url: String,

    // Git-specific
    pub protocol: Option<GitProtocol>,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub rev: Option<String>,
    pub srcrev: Option<String>,        // From SRCREV variable
    pub nobranch: bool,
    pub subpath: Option<String>,
    pub destsuffix: Option<String>,

    // File-specific
    pub apply: Option<bool>,           // For patches
    pub striplevel: Option<u32>,

    // HTTP-specific
    pub downloadfilename: Option<String>,

    // Common parameters
    pub name: Option<String>,
    pub unpack: Option<bool>,
    pub subdir: Option<String>,

    // Checksums
    pub md5sum: Option<String>,
    pub sha256sum: Option<String>,
    pub sha1sum: Option<String>,

    // Operation context
    pub operation: VariableOperation,
    pub overrides: Vec<String>,        // e.g., ["append", "qemuarm"]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UriScheme {
    File,
    Http,
    Https,
    Ftp,
    Git,
    GitSubmodule,
    Svn,
    Cvs,
    P4,
    Repo,
    Crate,
    Npm,
    Azure,
    GoogleStorage,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GitProtocol {
    Git,
    Http,
    Https,
    Ssh,
    Rsync,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VariableOperation {
    Assign,              // =
    ImmediateExpand,     // :=
    SoftDefault,         // ?=
    WeakDefault,         // ??=
    Append,              // +=
    Prepend,             // =+
    AppendNoSpace,       // .=
    PrependNoSpace,      // =.
    OverrideAppend(Vec<String>),  // :append:machine
    OverridePrepend(Vec<String>), // :prepend:machine
    OverrideRemove(Vec<String>),  // :remove:machine
}

// models/config.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitbakeConfig {
    pub file_path: PathBuf,
    pub config_type: ConfigType,
    pub variables: HashMap<String, VariableAssignment>,
    pub includes: Vec<IncludeDirective>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigType {
    Layer,          // layer.conf
    Machine,        // machine/*.conf
    Distribution,   // distro/*.conf
    Local,          // local.conf
    BitBake,        // bitbake.conf
    Other,
}

// models/class.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitbakeClass {
    pub file_path: PathBuf,
    pub class_name: String,
    pub class_scope: ClassScope,
    pub variables: HashMap<String, VariableAssignment>,
    pub shell_functions: Vec<ShellFunction>,
    pub python_functions: Vec<PythonFunction>,
    pub inherits: Vec<ClassInherit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClassScope {
    Global,         // classes-global/
    Recipe,         // classes-recipe/
    Both,           // classes/
}

// models/layer.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub path: PathBuf,
    pub name: String,
    pub priority: Option<i32>,
    pub depends: Vec<String>,
    pub compatible_series: Vec<String>,
    pub pattern: Option<String>,

    pub config: Option<BitbakeConfig>,  // layer.conf
    pub classes: Vec<BitbakeClass>,
    pub recipes: Vec<BitbakeRecipe>,
    pub machines: Vec<BitbakeConfig>,
    pub distros: Vec<BitbakeConfig>,
}
```

#### 1.2 Error Handling

```rust
// error.rs
#[derive(Debug, thiserror::Error)]
pub enum BitbakeError {
    #[error("Failed to parse file {path}: {source}")]
    ParseError {
        path: PathBuf,
        source: Box<dyn std::error::Error>,
    },

    #[error("Required file not found: {0}")]
    RequiredFileNotFound(PathBuf),

    #[error("Invalid SRC_URI syntax: {0}")]
    InvalidUri(String),

    #[error("Tree-sitter error: {0}")]
    TreeSitterError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Variable resolution error: {0}")]
    VariableError(String),
}

pub type Result<T> = std::result::Result<T, BitbakeError>;
```

**Deliverables**:
- [ ] Complete data model implementation
- [ ] Error type definitions
- [ ] Unit tests for data structures
- [ ] Documentation

### Phase 2: Enhanced Parsing Engine (Week 2-3)

**Goal**: Implement comprehensive tree-sitter based parsing

#### 2.1 Tree-Sitter Queries

**Priority: CRITICAL**

```rust
// parsers/queries.rs
pub struct BitbakeQueries {
    variable_assign: Query,
    variable_immediate: Query,
    variable_default: Query,
    variable_append: Query,
    variable_prepend: Query,
    variable_flag: Query,
    override_append: Query,
    override_prepend: Query,
    override_remove: Query,
    inherit_stmt: Query,
    include_stmt: Query,
    require_stmt: Query,
    shell_function: Query,
    python_function: Query,
    export_stmt: Query,
}

impl BitbakeQueries {
    pub fn new() -> Result<Self> {
        let language = tree_sitter_bitbake::language();

        Ok(Self {
            variable_assign: Query::new(
                language,
                r#"(variable_assignment
                    (identifier) @name
                    "=" @op
                    (literal) @value)"#
            )?,

            variable_append: Query::new(
                language,
                r#"(variable_assignment
                    (identifier) @name
                    "+=" @op
                    (literal) @value)"#
            )?,

            override_append: Query::new(
                language,
                r#"(override_assignment
                    (identifier) @name
                    (override)+ @overrides
                    ":append" @op
                    (literal) @value)"#
            )?,

            // ... more queries
        })
    }
}
```

#### 2.2 Multi-Pass Parser

```rust
// parsers/ast.rs
pub struct AstParser {
    parser: Parser,
    queries: BitbakeQueries,
}

impl AstParser {
    pub fn parse_recipe(&mut self, path: &Path) -> Result<BitbakeRecipe> {
        let code = std::fs::read_to_string(path)?;
        let tree = self.parser.parse(&code, None)
            .ok_or_else(|| BitbakeError::TreeSitterError("Parse failed".into()))?;

        let mut recipe = BitbakeRecipe::new(path);

        // Pass 1: Extract all variable assignments (build context)
        self.extract_variables(&tree, &code, &mut recipe)?;

        // Pass 2: Extract SRC_URI entries
        self.extract_source_uris(&tree, &code, &mut recipe)?;

        // Pass 3: Extract dependencies
        self.extract_dependencies(&tree, &code, &mut recipe)?;

        // Pass 4: Extract inherit/include statements
        self.extract_directives(&tree, &code, &mut recipe)?;

        // Pass 5: Extract functions (for indexing)
        self.extract_functions(&tree, &code, &mut recipe)?;

        Ok(recipe)
    }

    fn extract_variables(&self, tree: &Tree, code: &str, recipe: &mut BitbakeRecipe) -> Result<()> {
        let mut cursor = QueryCursor::new();

        // Handle all operator types
        for query_type in &[
            (&self.queries.variable_assign, VariableOperation::Assign),
            (&self.queries.variable_append, VariableOperation::Append),
            // ... more operator types
        ] {
            for m in cursor.matches(query_type.0, tree.root_node(), code.as_bytes()) {
                let name = self.get_capture_text(&m, "name", code)?;
                let value = self.get_capture_text(&m, "value", code)?;

                let assignment = VariableAssignment {
                    name: name.to_string(),
                    value: value.to_string(),
                    operation: query_type.1.clone(),
                    overrides: Vec::new(),
                };

                recipe.variables.insert(name.to_string(), assignment);

                // Extract specific known variables
                match name {
                    "PN" => recipe.package_name = Some(value.to_string()),
                    "PV" => recipe.package_version = Some(value.to_string()),
                    "BPN" => recipe.base_package_name = Some(value.to_string()),
                    "SUMMARY" => recipe.summary = Some(value.to_string()),
                    "DESCRIPTION" => recipe.description = Some(value.to_string()),
                    "HOMEPAGE" => recipe.homepage = Some(value.to_string()),
                    "LICENSE" => recipe.license = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        Ok(())
    }
}
```

#### 2.3 SRC_URI Parser

**Priority: CRITICAL**

```rust
// parsers/uri.rs
pub struct UriParser {
    git_regex: Regex,
    param_regex: Regex,
}

impl UriParser {
    pub fn new() -> Self {
        Self {
            git_regex: Regex::new(r"^(git(?:sm)?://[^;]+)(.*)$").unwrap(),
            param_regex: Regex::new(r";([^=;]+)=([^;]+)").unwrap(),
        }
    }

    pub fn parse(&self, uri: &str, operation: VariableOperation) -> Result<SourceUri> {
        let uri = uri.trim().trim_matches('"').trim_matches('\'');

        // Detect scheme
        let scheme = Self::detect_scheme(uri);

        // Split URI and parameters
        let (base_uri, params_str) = if let Some(idx) = uri.find(';') {
            (&uri[..idx], &uri[idx+1..])
        } else {
            (uri, "")
        };

        // Parse parameters
        let params = self.parse_parameters(params_str)?;

        let mut source = SourceUri {
            raw: uri.to_string(),
            scheme,
            url: base_uri.to_string(),
            operation,
            ..Default::default()
        };

        // Apply scheme-specific parsing
        match &source.scheme {
            UriScheme::Git | UriScheme::GitSubmodule => {
                self.parse_git_params(&mut source, &params)?;
            }
            UriScheme::Http | UriScheme::Https | UriScheme::Ftp => {
                self.parse_http_params(&mut source, &params)?;
            }
            UriScheme::File => {
                self.parse_file_params(&mut source, &params)?;
            }
            _ => {}
        }

        Ok(source)
    }

    fn detect_scheme(uri: &str) -> UriScheme {
        if uri.starts_with("git://") {
            UriScheme::Git
        } else if uri.starts_with("gitsm://") {
            UriScheme::GitSubmodule
        } else if uri.starts_with("https://") {
            UriScheme::Https
        } else if uri.starts_with("http://") {
            UriScheme::Http
        } else if uri.starts_with("ftp://") {
            UriScheme::Ftp
        } else if uri.starts_with("file://") {
            UriScheme::File
        } else if uri.starts_with("svn://") {
            UriScheme::Svn
        } else if uri.starts_with("crate://") {
            UriScheme::Crate
        } else if uri.starts_with("npm://") {
            UriScheme::Npm
        } else if uri.starts_with("az://") {
            UriScheme::Azure
        } else if uri.starts_with("gs://") {
            UriScheme::GoogleStorage
        } else {
            UriScheme::Other(
                uri.split("://").next().unwrap_or("unknown").to_string()
            )
        }
    }

    fn parse_git_params(&self, source: &mut SourceUri, params: &HashMap<String, String>) -> Result<()> {
        source.protocol = params.get("protocol")
            .and_then(|p| match p.as_str() {
                "git" => Some(GitProtocol::Git),
                "http" => Some(GitProtocol::Http),
                "https" => Some(GitProtocol::Https),
                "ssh" => Some(GitProtocol::Ssh),
                "rsync" => Some(GitProtocol::Rsync),
                "file" => Some(GitProtocol::File),
                _ => None,
            });

        source.branch = params.get("branch").cloned();
        source.tag = params.get("tag").cloned();
        source.rev = params.get("rev").cloned();
        source.nobranch = params.get("nobranch").map(|v| v == "1").unwrap_or(false);
        source.subpath = params.get("subpath").cloned();
        source.destsuffix = params.get("destsuffix").cloned();
        source.name = params.get("name").cloned();

        Ok(())
    }
}
```

**Deliverables**:
- [ ] Complete query implementation for all operators
- [ ] Multi-pass AST parser
- [ ] Comprehensive URI parser supporting all schemes
- [ ] Unit tests with real-world examples
- [ ] Integration tests

### Phase 3: File Discovery and Layer Support (Week 3-4)

**Goal**: Implement layer-aware file discovery

```rust
// discovery/layer.rs
pub struct LayerDiscovery {
    base_path: PathBuf,
}

impl LayerDiscovery {
    pub fn discover_layers(&self) -> Result<Vec<Layer>> {
        let mut layers = Vec::new();

        // Find all layer.conf files
        for entry in WalkDir::new(&self.base_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() == "layer.conf"
                && entry.path().parent().and_then(|p| p.file_name()) == Some(OsStr::new("conf"))
            {
                let layer_path = entry.path().parent().unwrap().parent().unwrap();
                let layer = self.parse_layer(layer_path)?;
                layers.push(layer);
            }
        }

        Ok(layers)
    }

    pub fn discover_files_in_layer(&self, layer: &Layer) -> Result<LayerFiles> {
        let mut files = LayerFiles::default();

        // Discover recipes
        files.recipes = self.find_files(&layer.path, &["**/*.bb"])?;

        // Discover appends
        files.appends = self.find_files(&layer.path, &["**/*.bbappend"])?;

        // Discover includes
        files.includes = self.find_files(&layer.path, &["**/*.inc"])?;

        // Discover classes
        files.classes = self.find_files(&layer.path, &[
            "classes/*.bbclass",
            "classes-global/*.bbclass",
            "classes-recipe/*.bbclass",
        ])?;

        // Discover configs
        files.machine_configs = self.find_files(&layer.path, &["conf/machine/*.conf"])?;
        files.distro_configs = self.find_files(&layer.path, &["conf/distro/*.conf"])?;

        Ok(files)
    }
}
```

**Deliverables**:
- [ ] Layer discovery implementation
- [ ] File type classification
- [ ] Layer priority handling
- [ ] Tests with multi-layer structure

### Phase 4: Variable Resolution (Week 4)

**Goal**: Implement basic variable substitution

```rust
// parsers/variable.rs
pub struct VariableResolver {
    variables: HashMap<String, String>,
    overrides: Vec<String>,
    max_depth: usize,
}

impl VariableResolver {
    pub fn new() -> Self {
        let mut resolver = Self {
            variables: HashMap::new(),
            overrides: Vec::new(),
            max_depth: 10,
        };

        // Add common built-ins
        resolver.add_builtins();
        resolver
    }

    fn add_builtins(&mut self) {
        // Built-in variables that are commonly used
        self.variables.insert("WORKDIR".to_string(), "${TMPDIR}/work/${MULTIMACH_TARGET_SYS}/${PN}/${PV}-${PR}".to_string());
        self.variables.insert("S".to_string(), "${WORKDIR}/${BP}".to_string());
        self.variables.insert("BP".to_string(), "${BPN}-${PV}".to_string());
        self.variables.insert("B".to_string(), "${S}".to_string());
    }

    pub fn resolve(&self, input: &str) -> String {
        let var_regex = Regex::new(r"\$\{([^}]+)\}").unwrap();
        let mut result = input.to_string();

        for depth in 0..self.max_depth {
            let before = result.clone();

            for cap in var_regex.captures_iter(&before.clone()) {
                let full_match = &cap[0];
                let var_name = &cap[1];

                if let Some(value) = self.variables.get(var_name) {
                    result = result.replace(full_match, value);
                }
            }

            if result == before {
                break;  // No more substitutions possible
            }
        }

        result
    }

    pub fn load_from_recipe(&mut self, recipe: &BitbakeRecipe) {
        for (name, assignment) in &recipe.variables {
            self.variables.insert(name.clone(), assignment.value.clone());
        }
    }
}
```

**Deliverables**:
- [ ] Basic variable resolution
- [ ] Circular reference detection
- [ ] Built-in variable definitions
- [ ] Tests for complex substitutions

### Phase 5: Include/Require Resolution (Week 5)

**Goal**: Follow include chains and merge metadata

```rust
// parsers/ast.rs
impl AstParser {
    pub fn parse_with_includes(&mut self, path: &Path, base_path: &Path) -> Result<BitbakeRecipe> {
        let mut recipe = self.parse_recipe(path)?;
        let mut resolver = VariableResolver::new();
        resolver.load_from_recipe(&recipe);

        // Recursively resolve includes
        let mut processed = HashSet::new();
        processed.insert(path.to_path_buf());

        self.resolve_includes_recursive(&mut recipe, &resolver, base_path, &mut processed)?;

        Ok(recipe)
    }

    fn resolve_includes_recursive(
        &mut self,
        recipe: &mut BitbakeRecipe,
        resolver: &VariableResolver,
        base_path: &Path,
        processed: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        let includes: Vec<_> = recipe.includes.iter()
            .map(|inc| resolver.resolve(&inc.path))
            .collect();

        for include_pattern in includes {
            let include_paths = self.resolve_include_path(&include_pattern, &recipe.file_path, base_path)?;

            for include_path in include_paths {
                if processed.contains(&include_path) {
                    continue;  // Avoid circular includes
                }

                if !include_path.exists() {
                    // include is non-fatal, require is fatal
                    continue;
                }

                processed.insert(include_path.clone());

                let included = self.parse_recipe(&include_path)?;

                // Merge data from included file
                recipe.sources.extend(included.sources);
                recipe.build_depends.extend(included.build_depends);
                recipe.runtime_depends.extend(included.runtime_depends);
                recipe.variables.extend(included.variables);
                recipe.inherits.extend(included.inherits);

                // Recursively resolve includes in the included file
                self.resolve_includes_recursive(recipe, resolver, base_path, processed)?;
            }
        }

        Ok(())
    }
}
```

**Deliverables**:
- [ ] Include resolution logic
- [ ] Circular include detection
- [ ] Search path handling
- [ ] Tests with complex include chains

### Phase 6: Graph Database Integration (Week 5-6)

**Goal**: Update graph schema and integration

```rust
// graph/builder.rs
pub struct BitbakeGraphBuilder {
    client: Neo4jClient,
}

impl BitbakeGraphBuilder {
    pub async fn build_recipe_graph(&self, recipe: &BitbakeRecipe, commit_oid: &str) -> Result<()> {
        // Create recipe node
        let recipe_node = self.create_recipe_node(recipe, commit_oid).await?;

        // Create source URI nodes and relationships
        for source in &recipe.sources {
            let source_node = self.create_source_node(source).await?;
            self.create_relationship(&recipe_node, &source_node, "HAS_SOURCE").await?;

            // If git source, try to find or create repository node
            if matches!(source.scheme, UriScheme::Git | UriScheme::GitSubmodule) {
                if let Some(repo_node) = self.find_or_create_repo(&source.url).await? {
                    self.create_relationship(&source_node, &repo_node, "REFERS_TO").await?;
                }
            }
        }

        // Create dependency relationships
        for dep in &recipe.build_depends {
            self.create_dependency(&recipe_node, dep, "BUILD_DEPENDS").await?;
        }

        for dep in &recipe.runtime_depends {
            self.create_dependency(&recipe_node, dep, "RUNTIME_DEPENDS").await?;
        }

        // Create class inheritance relationships
        for inherit in &recipe.inherits {
            if let Some(class_node) = self.find_class(&inherit.class_name).await? {
                self.create_relationship(&recipe_node, &class_node, "INHERITS").await?;
            }
        }

        // Link to commit
        self.create_relationship(&commit_node, &recipe_node, "CONTAINS").await?;

        Ok(())
    }
}
```

**New Cypher Queries**:
```cypher
// Create BitbakeRecipe node with rich metadata
CREATE (r:BitbakeRecipe:Manifest {
    path: $path,
    type: 'bitbake',
    recipe_type: $recipe_type,
    package_name: $package_name,
    version: $version,
    license: $license,
    layer: $layer
})

// Create SourceUri node
CREATE (s:SourceUri {
    url: $url,
    scheme: $scheme,
    branch: $branch,
    tag: $tag,
    srcrev: $srcrev,
    protocol: $protocol
})

// Create relationships
CREATE (r:BitbakeRecipe)-[:HAS_SOURCE {operation: $operation}]->(s:SourceUri)
CREATE (s:SourceUri)-[:REFERS_TO]->(repo:Repository)
CREATE (r:BitbakeRecipe)-[:BUILD_DEPENDS]->(dep:Package)
CREATE (r:BitbakeRecipe)-[:RUNTIME_DEPENDS]->(dep:Package)
CREATE (r:BitbakeRecipe)-[:INHERITS]->(c:BBClass)
CREATE (r:BitbakeRecipe)-[:INCLUDES]->(inc:Include)
CREATE (commit:Commit)-[:CONTAINS]->(r:BitbakeRecipe)
```

**Deliverables**:
- [ ] Updated graph schema
- [ ] Graph builder implementation
- [ ] Cypher query library
- [ ] Integration with existing CLI

### Phase 7: Testing and Validation (Week 6-7)

**Goal**: Comprehensive testing with real-world recipes

```rust
// tests/integration/yocto_recipes_test.rs
#[test]
fn test_parse_poky_recipes() {
    // Clone poky or use existing
    let poky_path = setup_poky_repo();

    let discovery = LayerDiscovery::new(&poky_path);
    let layers = discovery.discover_layers().unwrap();

    assert!(layers.len() > 0);

    for layer in layers {
        let files = discovery.discover_files_in_layer(&layer).unwrap();

        let mut parser = AstParser::new().unwrap();
        for recipe_path in &files.recipes {
            let recipe = parser.parse_with_includes(recipe_path, &poky_path);

            match recipe {
                Ok(r) => {
                    // Validate parsed data
                    assert!(r.package_name.is_some() || r.sources.len() > 0);

                    // Validate source URIs
                    for source in &r.sources {
                        validate_source_uri(source);
                    }
                },
                Err(e) => {
                    eprintln!("Failed to parse {}: {}", recipe_path.display(), e);
                }
            }
        }
    }
}
```

**Test Coverage**:
- [ ] Unit tests for all parsers
- [ ] URI parser with 50+ real examples
- [ ] Variable resolution edge cases
- [ ] Include resolution with circular refs
- [ ] Integration test with Poky
- [ ] Integration test with meta-openembedded
- [ ] Performance benchmarks

## Performance Considerations

### Optimization Strategies

1. **Parallel Processing**: Parse recipes in parallel using rayon
2. **Caching**: Cache parsed include files
3. **Lazy Loading**: Only parse classes when referenced
4. **Incremental Parsing**: Only re-parse changed files

```rust
use rayon::prelude::*;

pub fn parse_layer_parallel(layer: &Layer) -> Result<Vec<BitbakeRecipe>> {
    layer.recipe_files
        .par_iter()
        .map(|path| {
            let mut parser = AstParser::new()?;
            parser.parse_with_includes(path, &layer.path)
        })
        .collect()
}
```

## Migration Strategy

### Backwards Compatibility

Keep old API functional during transition:

```rust
// Old API (deprecated)
impl Bitbake {
    #[deprecated(note = "Use BitbakeRecipe::parse instead")]
    pub fn parse_bitbake(code: &str, filename: &str) -> Option<String> {
        // Wrapper around new implementation
        let mut parser = AstParser::new().ok()?;
        let recipe = parser.parse_recipe_from_string(code, filename).ok()?;
        recipe.sources.first().map(|s| s.url.clone())
    }
}

// New API
impl BitbakeRecipe {
    pub fn parse(path: &Path) -> Result<Self> {
        let mut parser = AstParser::new()?;
        parser.parse_with_includes(path, path.parent().unwrap())
    }
}
```

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| SRC_URI Extraction | 95%+ | % of all SRC_URI entries found vs manual count |
| Protocol Support | 100% | All documented schemes supported |
| Parse Success Rate | 90%+ | % of recipes parsed without errors |
| Performance | <100ms/file | Average parse time per recipe |
| Graph Completeness | 90%+ | % of relationships captured |
| Test Coverage | 80%+ | Line coverage |

## Resources Required

- **Development Time**: 6-7 weeks
- **Testing Infrastructure**: Yocto Project Poky repository
- **Dependencies**: regex, url, rayon, thiserror
- **Documentation**: API docs, architecture docs, migration guide

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Tree-sitter grammar incomplete | High | Contribute fixes upstream, fallback regex |
| Variable resolution too complex | Medium | Document limitations, incremental approach |
| Performance issues with large layers | Medium | Implement caching and parallel processing |
| Breaking changes to existing code | High | Maintain backwards compatibility |
| Testing with real recipes reveals edge cases | Medium | Iterative approach, comprehensive test suite |

## Next Steps

1. **Immediate**: Review and approve this roadmap
2. **Week 1**: Begin Phase 1 (Data Models)
3. **Weekly**: Progress reviews and adjustments
4. **Week 7**: Final validation and documentation
5. **Week 8**: Merge and release

## References

- [BitBake Specification](./BITBAKE_SPECIFICATION.md)
- [Original Improvement Proposal](./BITBAKE_PARSER_IMPROVEMENT_PROPOSAL.md)
- [BitBake User Manual](https://docs.yoctoproject.org/bitbake/)
- [tree-sitter-bitbake](https://github.com/avrabe/tree-sitter-bitbake)
