# Bitzel Bootstrap Architecture

## Overview

Complete bootstrap system that uses kas YAML configuration to set up BitBake-compatible builds with the Bitzel executor (Bazel-inspired BitBake replacement).

## Architecture

```
┌──────────────────┐
│   kas YAML       │  ← User provides kas project configuration
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  KasBootstrap    │  ← Parses kas YAML, clones repos
└────────┬─────────┘
         │
         ├─────────────────┐
         │                 │
         ▼                 ▼
┌──────────────┐   ┌──────────────┐
│  Git Clone   │   │ Config Gen   │
│  Manager     │   │  (local.conf,│
│              │   │  layer.conf) │
└──────┬───────┘   └──────┬───────┘
       │                  │
       └─────────┬────────┘
                 ▼
         ┌──────────────────┐
         │  BitBake Parser  │  ← Parse .bb recipes
         └────────┬─────────┘
                  │
                  ▼
         ┌──────────────────┐
         │  RecipeGraph     │  ← Build dependency graph
         └────────┬─────────┘
                  │
                  ▼
         ┌──────────────────┐
         │  TaskGraphBuilder│  ← Generate execution DAG
         └────────┬─────────┘
                  │
                  ▼
         ┌──────────────────┐
         │ InteractiveExecutor│  ← Execute with monitoring
         │  (Bitzel Core)    │
         └───────────────────┘
```

## Components

### 1. KasBootstrap (New)

**Purpose**: Main orchestrator that parses kas YAML and coordinates bootstrap

**Responsibilities**:
- Parse kas YAML files (using existing convenient-kas crate)
- Resolve repository dependencies
- Clone/update git repositories
- Generate BitBake configuration files
- Discover and parse recipes
- Build recipe/task graphs
- Execute builds with Bitzel

**Key Methods**:
```rust
pub struct KasBootstrap {
    kas_file: PathBuf,
    build_dir: PathBuf,
    config: KasConfig,
}

impl KasBootstrap {
    pub fn new(kas_file: impl AsRef<Path>) -> Result<Self>;
    pub fn setup_repositories(&self) -> Result<RepositorySetup>;
    pub fn generate_config(&self) -> Result<BitBakeConfig>;
    pub fn discover_recipes(&self) -> Result<RecipeCollection>;
    pub fn build(&self, target: &str) -> Result<BuildOutput>;
}
```

### 2. RepositoryManager (New)

**Purpose**: Clone and manage git repositories specified in kas YAML

**Responsibilities**:
- Clone repositories with specific refspecs
- Handle patches and overlays
- Manage repo.conf for Google Repo compatibility
- Support shallow clones for CI

**Key Methods**:
```rust
pub struct RepositoryManager {
    repos_dir: PathBuf,
    cache_dir: Option<PathBuf>,
}

impl RepositoryManager {
    pub fn clone_repo(&self, repo: &KasRepo) -> Result<PathBuf>;
    pub fn apply_patches(&self, repo_path: &Path, patches: &[String]) -> Result<()>;
    pub fn update_repo(&self, repo_path: &Path, refspec: &str) -> Result<()>;
}
```

### 3. ConfigGenerator (New)

**Purpose**: Generate BitBake configuration from kas setup

**Responsibilities**:
- Generate local.conf with machine, distro, and custom variables
- Generate bblayers.conf with layer paths
- Handle layer priorities
- Support templates and includes

**Key Methods**:
```rust
pub struct ConfigGenerator {
    build_dir: PathBuf,
    kas_config: KasConfig,
}

impl ConfigGenerator {
    pub fn generate_local_conf(&self) -> Result<String>;
    pub fn generate_bblayers_conf(&self, repos: &[PathBuf]) -> Result<String>;
    pub fn write_configs(&self) -> Result<()>;
}
```

### 4. RecipeDiscovery (Enhanced)

**Purpose**: Discover and parse recipes from layers

**Responsibilities**:
- Walk layer directories following BitBake conventions
- Parse .bb and .bbappend files
- Build class dependency tree
- Handle recipe variants (machine-specific, etc.)

**Key Methods**:
```rust
pub struct RecipeDiscovery {
    layers: Vec<PathBuf>,
    parser: RecipeExtractor,
}

impl RecipeDiscovery {
    pub fn discover_all(&self) -> Result<Vec<RecipeFile>>;
    pub fn parse_recipes(&self) -> Result<RecipeGraph>;
}
```

### 5. BitzelExecutor (Existing + Enhancements)

**Purpose**: Execute BitBake recipes using Bazel-inspired executor

**Enhancements Needed**:
- Support for shared state (sstate-cache)
- Download cache integration
- Support for BitBake-specific environment (${WORKDIR}, ${D}, etc.)
- Package splitting (do_package, do_package_write_*)

## Kas YAML Structure

Example kas file we need to support:

```yaml
header:
  version: 14
  includes:
    - common.yml

machine: qemux86-64
distro: poky

repos:
  poky:
    url: https://git.yoctoproject.org/git/poky
    refspec: kirkstone
    layers:
      meta:
      meta-poky:
      meta-yocto-bsp:

  meta-openembedded:
    url: https://github.com/openembedded/meta-openembedded.git
    refspec: kirkstone
    layers:
      meta-oe:
      meta-python:
      meta-networking:

local_conf_header:
  custom: |
    CONF_VERSION = "2"
    DL_DIR = "${TOPDIR}/downloads"
    SSTATE_DIR = "${TOPDIR}/sstate-cache"

target: core-image-minimal
```

## Implementation Plan

### Phase 1: Repository Management (Week 1)
- [ ] Implement RepositoryManager
- [ ] Add git clone with refspec support
- [ ] Add patch application
- [ ] Test with poky repository

### Phase 2: Configuration Generation (Week 1)
- [ ] Implement ConfigGenerator
- [ ] Generate local.conf from kas
- [ ] Generate bblayers.conf from layer list
- [ ] Support variable substitution

### Phase 3: Recipe Discovery (Week 2)
- [ ] Implement RecipeDiscovery
- [ ] Walk layers following BitBake conventions
- [ ] Handle recipe priorities and overrides
- [ ] Build complete recipe database

### Phase 4: Bootstrap Integration (Week 2)
- [ ] Implement KasBootstrap orchestrator
- [ ] Connect all components
- [ ] Add progress reporting
- [ ] Create end-to-end example

### Phase 5: Bitzel Enhancements (Week 3)
- [ ] Add sstate-cache support
- [ ] Implement download cache
- [ ] Add package splitting support
- [ ] Optimize for incremental builds

### Phase 6: Testing & Validation (Week 3)
- [ ] Test with core-image-minimal
- [ ] Test with meta-openembedded
- [ ] Verify incremental builds
- [ ] Performance benchmarking vs BitBake

## Directory Structure

```
build/
├── conf/
│   ├── local.conf           # Generated from kas
│   └── bblayers.conf        # Generated from kas
├── downloads/               # DL_DIR
├── sstate-cache/            # SSTATE_DIR
├── tmp/
│   ├── deploy/
│   │   ├── images/
│   │   └── ipk/
│   └── work/
│       └── <machine>/
│           └── <recipe>/
│               └── <version>/
├── bitzel-cache/           # Bitzel CAS + action cache
│   ├── cas/
│   │   └── sha256/
│   └── action-cache/
└── repos/
    ├── poky/
    ├── meta-openembedded/
    └── ...
```

## Bootstrap Flow

1. **Parse Kas YAML**
   ```rust
   let bootstrap = KasBootstrap::new("kas.yml")?;
   ```

2. **Setup Repositories**
   ```rust
   let repos = bootstrap.setup_repositories()?;
   // Clones poky, meta-openembedded, etc.
   ```

3. **Generate Configuration**
   ```rust
   bootstrap.generate_config()?;
   // Creates build/conf/local.conf and bblayers.conf
   ```

4. **Discover Recipes**
   ```rust
   let recipes = bootstrap.discover_recipes()?;
   // Walks layers, parses .bb files
   ```

5. **Build Task Graph**
   ```rust
   let task_graph = bootstrap.build_task_graph("core-image-minimal")?;
   // Resolves dependencies, creates execution DAG
   ```

6. **Execute Build**
   ```rust
   let output = bootstrap.execute_interactive(task_graph)?;
   // Runs tasks with Bitzel executor, monitoring, caching
   ```

## Advantages Over BitBake

1. **Faster Incremental Builds**
   - Content-addressable caching
   - Fine-grained task dependencies
   - Parallel execution by default

2. **Better Observability**
   - Real-time task monitoring
   - JSON export for analysis
   - Interactive debugging

3. **Reproducible Builds**
   - Hermetic sandboxing
   - Input content hashing
   - Deterministic task execution

4. **Modern Tooling**
   - Written in Rust
   - Type-safe configuration
   - Comprehensive error messages

## Compatibility Notes

### BitBake Compatibility
- Parses BitBake recipe syntax
- Supports BitBake variables (DEPENDS, RDEPENDS, etc.)
- Compatible with existing layers
- Uses same directory structure

### Deviations
- Uses content-addressable storage instead of stamps
- Different internal task representation
- Enhanced sandboxing
- Native parallel execution

## Future Enhancements

1. **Remote Execution** (Bazel-style)
2. **Build Farm Integration**
3. **Cross-Compilation Optimization**
4. **Package Feed Generation**
5. **SDK Generation**
6. **eSDK Support**

## Testing Strategy

### Unit Tests
- Each component independently
- Mock kas configurations
- Test error handling

### Integration Tests
- End-to-end bootstrap
- Multi-layer projects
- Dependency resolution

### Real-World Tests
- Build core-image-minimal
- Build with meta-openembedded
- Build custom images
- Compare with BitBake results

## Performance Goals

- **Initial Build**: Within 10% of BitBake
- **Incremental Build**: 2-5x faster than BitBake
- **Cache Hit Rate**: >95% for typical workflows
- **Parallel Efficiency**: Near-linear scaling to 16 cores

## References

- [Kas Documentation](https://kas.readthedocs.io/)
- [BitBake User Manual](https://docs.yoctoproject.org/bitbake/)
- [Bazel Build System](https://bazel.build/)
- [Buck2 Build System](https://buck2.build/)
