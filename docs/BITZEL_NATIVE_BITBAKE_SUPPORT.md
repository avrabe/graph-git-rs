# Bitzel Native BitBake Support Architecture

## Overview

This document describes how to add native BitBake configuration support to bitzel, enabling it to work with standard Yocto/OpenEmbedded build directories without requiring KAS configuration files.

## Current State

### What Bitzel Already Has ✅

1. **Layer Management** (`layer_context.rs`)
   - LayerConfig parsing from layer.conf
   - Layer priority handling
   - BBFILE_COLLECTIONS, BBFILE_PRIORITY, LAYERDEPENDS
   - Layer dependency verification
   - Recipe discovery within layers
   - Bbappend application in priority order

2. **Build Context** (`layer_context.rs`)
   - Global variables management
   - MACHINE and DISTRO configuration
   - Configuration file loading and merging
   - Override resolver integration
   - Recipe parsing with full context

3. **Recipe Parsing** (`recipe_parser.rs`)
   - Full BitBake recipe syntax support
   - Variable expansion
   - Include/require directive handling
   - Task extraction
   - Dependency resolution

4. **Task Execution** (`executor/`)
   - Sandboxed execution with Linux namespaces
   - Content-addressable caching
   - Dependency graph building
   - Parallel task execution
   - Network policy support (for do_fetch)

5. **Python Execution** (`python_executor.rs`, `python_bridge.rs`)
   - RustPython-based Python task execution
   - bb.data DataStore implementation
   - bb.fetch2 module for source fetching
   - bb.utils module for utilities

### What's Missing ❌

1. **Configuration File Parsers**
   - bblayers.conf parser
   - local.conf parser
   - Variable expansion for ${TOPDIR}, ${OEROOT}, ${LAYERDIR}

2. **Build Directory Setup**
   - Standard build/ directory structure
   - TOPDIR, TMPDIR, DL_DIR configuration
   - Environment variable handling

3. **Native BitBake Mode**
   - Alternative entry point without KAS
   - Direct reading of build/conf/ directory
   - Layer auto-discovery from BBLAYERS

## Architecture Design

### 1. Configuration Parsers Module

Create `convenient-bitbake/src/bitbake_config.rs`:

```rust
/// Parse bblayers.conf and extract BBLAYERS variable
pub struct BbLayersConfig {
    pub bblayers: Vec<PathBuf>,
    pub bbpath: Option<String>,
    pub bbfiles: Vec<String>,
    pub variables: HashMap<String, String>,
}

impl BbLayersConfig {
    pub fn parse<P: AsRef<Path>>(path: P) -> Result<Self, String>;
    pub fn get_bblayers(&self) -> Vec<PathBuf>;
}

/// Parse local.conf and extract build settings
pub struct LocalConfig {
    pub machine: Option<String>,
    pub distro: Option<String>,
    pub dl_dir: Option<PathBuf>,
    pub tmpdir: Option<PathBuf>,
    pub sstate_dir: Option<PathBuf>,
    pub bb_number_threads: Option<usize>,
    pub parallel_make: Option<String>,
    pub variables: HashMap<String, String>,
}

impl LocalConfig {
    pub fn parse<P: AsRef<Path>>(path: P) -> Result<Self, String>;
}

/// Variable expander for ${VAR} syntax
pub struct VariableExpander {
    variables: HashMap<String, String>,
}

impl VariableExpander {
    pub fn new() -> Self;
    pub fn set(&mut self, key: String, value: String);
    pub fn expand(&self, input: &str) -> String;
    pub fn expand_path(&self, input: &Path) -> PathBuf;
}
```

### 2. Build Environment Module

Create `convenient-bitbake/src/build_env.rs`:

```rust
/// Represents a BitBake build environment
pub struct BuildEnvironment {
    /// Build directory (TOPDIR)
    pub topdir: PathBuf,
    /// OE-Core root (OEROOT) - optional
    pub oeroot: Option<PathBuf>,
    /// Downloads directory (DL_DIR)
    pub dl_dir: PathBuf,
    /// Build output directory (TMPDIR)
    pub tmpdir: PathBuf,
    /// Shared state cache (SSTATE_DIR)
    pub sstate_dir: PathBuf,
    /// Configuration directory
    pub confdir: PathBuf,
    /// All layers
    pub layers: Vec<PathBuf>,
    /// Build configuration
    pub local_config: LocalConfig,
    /// Layer configuration
    pub bblayers_config: BbLayersConfig,
    /// Variable expander
    expander: VariableExpander,
}

impl BuildEnvironment {
    /// Create from a build directory
    pub fn from_build_dir<P: AsRef<Path>>(topdir: P) -> Result<Self, String>;

    /// Set up variable expander with standard variables
    fn setup_variables(&mut self);

    /// Expand a path using build environment variables
    pub fn expand_path(&self, path: &str) -> PathBuf;

    /// Get all layers with absolute paths
    pub fn get_layers(&self) -> Vec<PathBuf>;

    /// Create BuildContext from this environment
    pub fn create_build_context(&self) -> Result<BuildContext, String>;
}
```

### 3. Bitzel Command-Line Interface

Update `bitzel/src/main.rs` to support two modes:

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bitzel")]
#[command(about = "Bazel-inspired build orchestrator for BitBake/Yocto")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build using KAS configuration (existing mode)
    Kas {
        /// Path to KAS configuration file
        #[arg(short, long, default_value = "kas.yml")]
        config: PathBuf,

        /// Build directory
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        /// Target to build
        target: Option<String>,
    },

    /// Build using native BitBake configuration (new mode)
    Build {
        /// Build directory (should contain conf/bblayers.conf and conf/local.conf)
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        /// Target to build (default from local.conf or specify recipe)
        target: Option<String>,
    },

    /// Initialize a new build directory
    Init {
        /// Build directory to create
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        /// Template configuration directory
        #[arg(short, long)]
        templateconf: Option<PathBuf>,
    },
}
```

### 4. Implementation Plan

#### Phase 1: Configuration Parsers

1. **bblayers.conf Parser**
   - Reuse existing BitbakeRecipe parser
   - Extract `BBLAYERS` variable
   - Support variable expansion in paths
   - Handle `BBPATH`, `BBFILES` variables

2. **local.conf Parser**
   - Reuse existing BitbakeRecipe parser
   - Extract standard variables (MACHINE, DISTRO, DL_DIR, etc.)
   - Handle conditional assignments (?=, ??=, =, +=)
   - Store all variables for later use

3. **Variable Expander**
   - Implement ${VAR} expansion
   - Support nested expansions: ${${VAR}}
   - Handle special variables:
     - `${TOPDIR}` - build directory
     - `${OEROOT}` - OE-Core root
     - `${LAYERDIR}` - current layer directory
     - `${COREBASE}` - alias for OEROOT
   - Path expansion and normalization

#### Phase 2: Build Environment

1. **BuildEnvironment Construction**
   - Read conf/bblayers.conf
   - Read conf/local.conf
   - Set up variable expander with TOPDIR
   - Expand BBLAYERS paths to absolute
   - Verify layers exist

2. **BuildContext Creation**
   - For each layer in BBLAYERS:
     - Parse conf/layer.conf
     - Create LayerConfig
     - Add to BuildContext
   - Set MACHINE and DISTRO from local.conf
   - Verify layer dependencies

3. **Directory Structure**
   - Ensure tmp/ exists
   - Ensure downloads/ exists
   - Ensure sstate-cache/ exists
   - Create work directories as needed

#### Phase 3: Native Build Mode

1. **Build Command Implementation**
   ```rust
   async fn native_build(builddir: PathBuf, target: Option<String>) -> Result<()> {
       // Load build environment
       let build_env = BuildEnvironment::from_build_dir(&builddir)?;

       // Create build context
       let build_context = build_env.create_build_context()?;

       // Discover recipes
       let recipes = build_context.find_recipes();

       // Parse recipes with context
       let parsed_recipes = parse_all_recipes(&build_context, &recipes)?;

       // Build dependency graph
       let graph = RecipeGraph::from_recipes(&parsed_recipes)?;

       // Determine target
       let target = target.or(build_env.local_config.target.clone())
           .ok_or("No target specified")?;

       // Execute build
       execute_build(&graph, &target, &build_env).await?;

       Ok(())
   }
   ```

2. **Task Execution Integration**
   - Set DL_DIR from environment
   - Set TMPDIR for work directories
   - Create work dirs: `${TMPDIR}/work/${ARCH}/${PN}/${PV}/`
   - Use NetworkPolicy::FullNetwork for do_fetch
   - Use NetworkPolicy::Isolated for other tasks

3. **Output Organization**
   - Deploy images to: `${TMPDIR}/deploy/images/${MACHINE}/`
   - Deploy packages to: `${TMPDIR}/deploy/ipk/` (or rpm/deb)
   - Stamps in: `${TMPDIR}/stamps/`
   - Logs in: `${TMPDIR}/log/`

#### Phase 4: Testing

1. **Clone Modern Poky**
   ```bash
   mkdir layers/
   git clone -b scarthgap \
     https://git.openembedded.org/bitbake layers/bitbake
   git clone -b scarthgap \
     https://git.openembedded.org/openembedded-core layers/openembedded-core
   git clone -b scarthgap \
     https://git.yoctoproject.org/meta-yocto layers/meta-yocto
   ```

2. **Initialize Build**
   ```bash
   TEMPLATECONF=$PWD/layers/meta-yocto/meta-poky/conf/templates/default \
     source ./layers/openembedded-core/oe-init-build-env
   ```

3. **Test Bitzel**
   ```bash
   # Use bitzel instead of bitbake
   bitzel build --builddir build core-image-minimal
   ```

4. **Verify Output**
   - Check that recipes are discovered
   - Verify dependency graph is correct
   - Confirm tasks execute in proper order
   - Validate output in tmp/deploy/

## Implementation Files

### New Files to Create

1. **`convenient-bitbake/src/bitbake_config.rs`**
   - BbLayersConfig struct and parser
   - LocalConfig struct and parser
   - VariableExpander implementation

2. **`convenient-bitbake/src/build_env.rs`**
   - BuildEnvironment struct
   - Environment setup from build directory
   - BuildContext creation from environment

3. **`bitzel/src/commands/build.rs`**
   - Native build command implementation
   - Recipe discovery and parsing
   - Build execution logic

4. **`bitzel/src/commands/init.rs`** (optional)
   - Build directory initialization
   - Template conf file copying

5. **`bitzel/src/commands/kas.rs`**
   - Move existing KAS logic here
   - Keep as separate command

### Files to Modify

1. **`convenient-bitbake/src/lib.rs`**
   - Export new modules
   - Add pub use for new types

2. **`bitzel/src/main.rs`**
   - Add clap command-line parsing
   - Route to appropriate command handler
   - Keep existing logic in kas command

3. **`convenient-bitbake/src/layer_context.rs`**
   - Add method to create from BuildEnvironment
   - Enhance variable expansion support

## Variable Expansion Strategy

### Standard Variables

| Variable | Value | When Set |
|----------|-------|----------|
| `${TOPDIR}` | Absolute path to build/ | Always |
| `${OEROOT}` | Path to openembedded-core layer | If detected |
| `${COREBASE}` | Alias for OEROOT | If OEROOT set |
| `${LAYERDIR}` | Current layer directory | Per-layer |
| `${TMPDIR}` | `${TOPDIR}/tmp` or from local.conf | Always |
| `${DL_DIR}` | `${TOPDIR}/downloads` or from local.conf | Always |
| `${SSTATE_DIR}` | `${TOPDIR}/sstate-cache` or from local.conf | Always |
| `${MACHINE}` | From local.conf | If set |
| `${DISTRO}` | From local.conf | If set |

### Expansion Algorithm

```rust
fn expand_variable(input: &str, vars: &HashMap<String, String>) -> String {
    let mut result = input.to_string();
    let mut changed = true;

    // Iterate until no more substitutions (handles nested ${${VAR}})
    while changed {
        changed = false;
        let before = result.clone();

        // Find ${...} patterns
        while let Some(start) = result.find("${") {
            if let Some(end) = result[start..].find('}') {
                let var_name = &result[start+2..start+end];
                if let Some(value) = vars.get(var_name) {
                    result.replace_range(start..start+end+1, value);
                    changed = true;
                } else {
                    // Unknown variable, leave as-is or error
                    break;
                }
            } else {
                break;
            }
        }

        if result == before {
            break;
        }
    }

    result
}
```

## Migration Path

### For KAS Users

No changes required! Existing KAS workflow continues to work:

```bash
bitzel kas --config examples/busybox-qemux86-64.yml core-image-minimal
```

### For Native BitBake Users

New workflow becomes available:

```bash
# Set up Yocto environment (standard way)
TEMPLATECONF=$PWD/layers/meta-yocto/meta-poky/conf/templates/default \
  source ./layers/openembedded-core/oe-init-build-env

# Use bitzel instead of bitbake
bitzel build core-image-minimal
```

## Benefits

1. **No KAS Dependency**: Can use bitzel with standard Yocto setup
2. **Familiar Workflow**: Matches standard BitBake usage
3. **Layer Compatibility**: Works with any BitBake-compatible layer
4. **Configuration Reuse**: Uses existing bblayers.conf and local.conf
5. **Hybrid RustPython**: Real Python execution with fast Rust backend
6. **Bazel-style Caching**: Content-addressable caching for fast rebuilds

## Success Criteria

1. ✅ Can parse bblayers.conf and local.conf
2. ✅ Can discover all layers from BBLAYERS
3. ✅ Can find all recipes across layers
4. ✅ Can build dependency graph
5. ✅ Can execute do_fetch with real downloads
6. ✅ Can execute do_unpack, do_configure, do_compile
7. ✅ Can produce artifacts in proper tmp/deploy/ structure
8. ✅ Build output matches BitBake structure
9. ✅ Can build busybox or core-image-minimal successfully

## Timeline

- **Phase 1** (Config Parsers): 1-2 days
- **Phase 2** (Build Environment): 1 day
- **Phase 3** (Native Build Mode): 2-3 days
- **Phase 4** (Testing & Integration): 2-3 days

**Total**: ~1 week for complete native BitBake support

## Next Steps

1. Implement bblayers.conf parser
2. Implement local.conf parser
3. Implement variable expander
4. Create BuildEnvironment
5. Add build command to bitzel
6. Set up real Poky for testing
7. Test with busybox recipe
8. Document usage

## References

- [Modern BitBake Structure](./MODERN_BITBAKE_STRUCTURE.md)
- [RustPython Fetch Architecture](./RUSTPYTHON_FETCH_ARCHITECTURE.md)
- [Yocto Build Directory Structure](https://docs.yoctoproject.org/ref-manual/structure.html)
- [BitBake User Manual](https://docs.yoctoproject.org/bitbake/)
