# Critical Gaps Implementation Plan
**Date:** 2025-12-02
**Goal:** Close all 7 critical gaps identified in honest assessment
**Target:** Working end-to-end build of busybox for qemuarm64

---

## Overview

This document provides a detailed implementation plan to address all critical gaps blocking real-world usage of hitzeleiter.

**Current State:** 60-70% complete (Assessment score: 80/100 B-)
**Target State:** 90-95% complete - viable BitBake replacement

---

## Phase 1: Wire Up Sysroot Assembly (1 week)
**Priority: CRITICAL** | **Status: Code exists, needs integration**

### What Exists

✅ **Fully implemented** (`convenient-bitbake/src/sysroot.rs` - 571 lines):
- `SysrootAssembler::assemble_sysroot()` - hardlink-based assembly
- `HardlinkTreeBuilder::copyhardlinktree()` - cp -afl implementation
- `SysrootManifest` - tracking which files come from which dependency
- Conflict detection - prevents duplicate files from different recipes
- 5 comprehensive tests - all passing

### What Needs Integration

**Integration Points:**

1. **BuildOrchestrator** (`convenient-bitbake/src/build_orchestrator.rs`)
   - Add sysroot assembly step before task execution
   - Location: After recipe graph built, before task execution
   - Function: `fn assemble_recipe_sysroot(recipe_id, dependencies) -> Result<PathBuf>`

2. **TaskExecutor** (`convenient-bitbake/src/executor/executor.rs`)
   - Pass sysroot path to task execution
   - Mount sysroot in sandbox as STAGING_DIR_HOST
   - Environment variables: `STAGING_DIR_HOST`, `STAGING_DIR_NATIVE`

3. **TaskSpec** (`convenient-bitbake/src/executor/types.rs`)
   - Add `sysroot_path: Option<PathBuf>` field
   - This allows each task to have its own recipe-specific sysroot

### Implementation Steps

```rust
// Step 1: Add to TaskSpec (types.rs)
pub struct TaskSpec {
    // ... existing fields ...
    pub sysroot_path: Option<PathBuf>,  // NEW
}

// Step 2: Add to BuildOrchestrator (build_orchestrator.rs)
use crate::sysroot::{SysrootAssembler, TaskDependency};

impl BuildOrchestrator {
    fn assemble_recipe_sysroot(
        &self,
        recipe_id: RecipeId,
        dependencies: &[RecipeId],
        cache_dir: &Path,
    ) -> Result<PathBuf, BuildError> {
        let assembler = SysrootAssembler::new();

        // Convert recipe dependencies to task dependencies
        let task_deps: Vec<TaskDependency> = dependencies
            .iter()
            .map(|dep_id| {
                let dep_recipe = self.recipe_graph.get_recipe(*dep_id)?;
                TaskDependency {
                    recipe: dep_recipe.name.clone(),
                    task: "do_populate_sysroot".to_string(),
                    signature: self.compute_signature(*dep_id)?,
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Create sysroot directory
        let recipe = self.recipe_graph.get_recipe(recipe_id)?;
        let sysroot_path = cache_dir.join("sysroots")
            .join(format!("{}-{}", recipe.name, recipe.version));

        // Assemble sysroot from dependencies
        assembler.assemble_sysroot(&task_deps, cache_dir, &sysroot_path)?;

        Ok(sysroot_path)
    }
}

// Step 3: Add to executor (executor.rs)
impl TaskExecutor {
    fn execute_task_with_sysroot(
        &self,
        spec: &TaskSpec,
    ) -> ExecutionResult<TaskOutput> {
        // Set sysroot environment variables
        let mut env = spec.environment.clone();

        if let Some(sysroot_path) = &spec.sysroot_path {
            env.insert("STAGING_DIR_HOST".to_string(),
                      sysroot_path.to_string_lossy().to_string());
            env.insert("STAGING_DIR_NATIVE".to_string(),
                      sysroot_path.join("native").to_string_lossy().to_string());
        }

        // Execute with updated environment
        // ... existing execution logic ...
    }
}
```

### Testing

**Test Plan:**
1. Create minimal recipe with DEPENDS
2. Build dependency first (creates sysroot content)
3. Build main recipe - verify sysroot assembled correctly
4. Check that files from dependency are in recipe sysroot
5. Verify conflict detection works for duplicate files

**Acceptance Criteria:**
- Recipe-specific sysroots created in cache/sysroots/
- STAGING_DIR_HOST points to correct sysroot
- Headers from dependencies accessible during build
- Conflict detection prevents duplicate file issues

---

## Phase 2: Cross-Compilation Toolchain (2 weeks)
**Priority: CRITICAL** | **Status: Not implemented**

### What Needs Implementation

**New Module:** `convenient-bitbake/src/toolchain.rs`

### Machine → Toolchain Mapping

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::env;

#[derive(Debug, Clone)]
pub struct ToolchainConfig {
    /// Target architecture (e.g., "aarch64", "armv7", "x86_64")
    pub target_arch: String,

    /// Target vendor (e.g., "unknown", "pc")
    pub target_vendor: String,

    /// Target OS (e.g., "linux", "linux-gnu", "linux-musl")
    pub target_os: String,

    /// Build system triplet (host that's doing the build)
    pub build_sys: String,

    /// Host system triplet (where binary will run)
    pub host_sys: String,

    /// Target system triplet (for cross-compilers)
    pub target_sys: String,

    /// Toolchain prefix (e.g., "aarch64-linux-gnu-")
    pub toolchain_prefix: String,

    /// Toolchain paths
    pub toolchain_dir: Option<PathBuf>,

    /// Compiler flags
    pub cflags: Vec<String>,
    pub cxxflags: Vec<String>,
    pub ldflags: Vec<String>,
}

pub struct ToolchainManager {
    machine_configs: HashMap<String, ToolchainConfig>,
}

impl ToolchainManager {
    pub fn new() -> Self {
        let mut machine_configs = HashMap::new();

        // qemuarm64 (AArch64)
        machine_configs.insert("qemuarm64".to_string(), ToolchainConfig {
            target_arch: "aarch64".to_string(),
            target_vendor: "unknown".to_string(),
            target_os: "linux-gnu".to_string(),
            build_sys: Self::detect_build_system(),
            host_sys: "aarch64-unknown-linux-gnu".to_string(),
            target_sys: "aarch64-unknown-linux-gnu".to_string(),
            toolchain_prefix: "aarch64-linux-gnu-".to_string(),
            toolchain_dir: Self::find_toolchain("aarch64-linux-gnu"),
            cflags: vec!["-march=armv8-a".to_string()],
            cxxflags: vec!["-march=armv8-a".to_string()],
            ldflags: vec![],
        });

        // qemuarm (32-bit ARM)
        machine_configs.insert("qemuarm".to_string(), ToolchainConfig {
            target_arch: "arm".to_string(),
            target_vendor: "unknown".to_string(),
            target_os: "linux-gnueabi".to_string(),
            build_sys: Self::detect_build_system(),
            host_sys: "arm-unknown-linux-gnueabi".to_string(),
            target_sys: "arm-unknown-linux-gnueabi".to_string(),
            toolchain_prefix: "arm-linux-gnueabi-".to_string(),
            toolchain_dir: Self::find_toolchain("arm-linux-gnueabi"),
            cflags: vec!["-march=armv7-a".to_string(), "-mfpu=neon".to_string()],
            cxxflags: vec!["-march=armv7-a".to_string(), "-mfpu=neon".to_string()],
            ldflags: vec![],
        });

        // qemux86-64 (x86_64 - native)
        machine_configs.insert("qemux86-64".to_string(), ToolchainConfig {
            target_arch: "x86_64".to_string(),
            target_vendor: "pc".to_string(),
            target_os: "linux-gnu".to_string(),
            build_sys: "x86_64-pc-linux-gnu".to_string(),
            host_sys: "x86_64-pc-linux-gnu".to_string(),
            target_sys: "x86_64-pc-linux-gnu".to_string(),
            toolchain_prefix: "".to_string(), // Native build
            toolchain_dir: None,
            cflags: vec![],
            cxxflags: vec![],
            ldflags: vec![],
        });

        Self { machine_configs }
    }

    /// Detect build system triplet
    fn detect_build_system() -> String {
        // Get from rustc
        let output = std::process::Command::new("rustc")
            .args(&["-vV"])
            .output()
            .ok();

        if let Some(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.starts_with("host: ") {
                    return line.strip_prefix("host: ").unwrap().to_string();
                }
            }
        }

        // Fallback
        "x86_64-pc-linux-gnu".to_string()
    }

    /// Find toolchain in standard locations
    fn find_toolchain(prefix: &str) -> Option<PathBuf> {
        let search_paths = vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from(format!("/usr/{}/bin", prefix)),
        ];

        for path in search_paths {
            let gcc = path.join(format!("{}-gcc", prefix));
            if gcc.exists() {
                return Some(path);
            }
        }

        None
    }

    /// Get toolchain config for a machine
    pub fn get_config(&self, machine: &str) -> Option<&ToolchainConfig> {
        self.machine_configs.get(machine)
    }

    /// Generate environment variables for a toolchain
    pub fn generate_env_vars(&self, machine: &str) -> Result<HashMap<String, String>, String> {
        let config = self.get_config(machine)
            .ok_or_else(|| format!("Unknown machine: {}", machine))?;

        let mut env = HashMap::new();

        // System triplets
        env.insert("BUILD_SYS".to_string(), config.build_sys.clone());
        env.insert("HOST_SYS".to_string(), config.host_sys.clone());
        env.insert("TARGET_SYS".to_string(), config.target_sys.clone());

        // Architecture
        env.insert("TARGET_ARCH".to_string(), config.target_arch.clone());
        env.insert("TARGET_VENDOR".to_string(), config.target_vendor.clone());
        env.insert("TARGET_OS".to_string(), config.target_os.clone());

        // Toolchain binaries
        if !config.toolchain_prefix.is_empty() {
            let prefix = &config.toolchain_prefix;
            env.insert("CC".to_string(), format!("{}gcc", prefix));
            env.insert("CXX".to_string(), format!("{}g++", prefix));
            env.insert("LD".to_string(), format!("{}ld", prefix));
            env.insert("AR".to_string(), format!("{}ar", prefix));
            env.insert("AS".to_string(), format!("{}as", prefix));
            env.insert("RANLIB".to_string(), format!("{}ranlib", prefix));
            env.insert("STRIP".to_string(), format!("{}strip", prefix));
            env.insert("OBJCOPY".to_string(), format!("{}objcopy", prefix));
            env.insert("OBJDUMP".to_string(), format!("{}objdump", prefix));

            // Add toolchain to PATH
            if let Some(toolchain_dir) = &config.toolchain_dir {
                let current_path = env::var("PATH").unwrap_or_default();
                env.insert("PATH".to_string(), format!("{}:{}",
                    toolchain_dir.display(), current_path));
            }
        } else {
            // Native build
            env.insert("CC".to_string(), "gcc".to_string());
            env.insert("CXX".to_string(), "g++".to_string());
        }

        // Compiler flags
        env.insert("CFLAGS".to_string(), config.cflags.join(" "));
        env.insert("CXXFLAGS".to_string(), config.cxxflags.join(" "));
        env.insert("LDFLAGS".to_string(), config.ldflags.join(" "));

        Ok(env)
    }
}
```

### Integration with BuildEnvironment

```rust
// In build_environment.rs
use crate::toolchain::ToolchainManager;

impl BuildEnvironment {
    pub fn with_toolchain(mut self, machine: &str) -> Result<Self, BuildError> {
        let toolchain_mgr = ToolchainManager::new();
        let toolchain_env = toolchain_mgr.generate_env_vars(machine)?;

        // Merge toolchain environment into build environment
        for (key, value) in toolchain_env {
            self.variables.insert(key, value);
        }

        Ok(self)
    }
}
```

### Testing

**Test Plan:**
1. Detect build system correctly
2. Find installed cross-compilers
3. Generate correct environment variables
4. Verify CC points to cross-compiler
5. Compile simple C file with cross-compiler

---

## Phase 3: Complete Task Implementations (4-6 weeks)
**Priority: CRITICAL** | **Status: Stubs exist, need real implementations**

### Current Status (bbhelpers.rs)

**Implemented:**
- ✅ oe_runmake() - make with parallel jobs
- ✅ bb_note/warn/fatal() - logging
- ✅ oe_soinstall() - library installation with symlinks
- ✅ oe_libinstall() - library file installation
- ✅ autotools_do_configure/compile/install() - basic autotools
- ✅ oe_runconf() - configure with standard args

**Stubs (lines 206-223):**
- ❌ base_do_fetch() - "Stub: would fetch from SRC_URI"
- ❌ base_do_unpack() - "Stub: would extract sources"
- ❌ base_do_patch() - "Stub: would apply patches"

### 3.1: Real do_fetch Implementation

**Approach:** Connect existing fetcher code

**Current:** `convenient-bitbake/src/fetcher.rs` has HTTP/Git fetcher
**Issue:** Not wired up to task execution

```rust
// In executor.rs
fn execute_do_fetch(&self, spec: &TaskSpec) -> ExecutionResult<TaskOutput> {
    use crate::fetcher::{Fetcher, FetchContext};

    // Parse SRC_URI from spec environment
    let src_uri = spec.environment.get("SRC_URI")
        .ok_or_else(|| ExecutionError::MissingVariable("SRC_URI".to_string()))?;

    // Create fetch context
    let dl_dir = spec.environment.get("DL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("downloads"));

    let ctx = FetchContext {
        dl_dir: dl_dir.clone(),
        mirrors: Vec::new(), // TODO: Parse MIRRORS variable
        proxies: HashMap::new(),
    };

    let fetcher = Fetcher::new(ctx);

    // Parse and fetch each URI
    let uris = parse_src_uri(src_uri)?;
    let mut fetched_files = Vec::new();

    for uri in uris {
        let file_path = fetcher.fetch(&uri)?;
        fetched_files.push(file_path);
    }

    // Return output with fetched files
    Ok(TaskOutput {
        exit_code: 0,
        output_files: fetched_files.into_iter()
            .map(|p| (p.clone(), ContentHash::from_file(&p)))
            .collect(),
        stdout: format!("Fetched {} files", fetched_files.len()),
        stderr: String::new(),
    })
}
```

### 3.2: Real do_unpack Implementation

**Approach:** Use existing archive extraction

```rust
fn execute_do_unpack(&self, spec: &TaskSpec) -> ExecutionResult<TaskOutput> {
    let workdir = spec.workdir.clone();
    let s_dir = spec.environment.get("S")
        .map(PathBuf::from)
        .unwrap_or_else(|| workdir.join("source"));

    fs::create_dir_all(&s_dir)?;

    // Find fetched files in DL_DIR
    let dl_dir = spec.environment.get("DL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("downloads"));

    // Extract archives
    for entry in fs::read_dir(&dl_dir)? {
        let entry = entry?;
        let path = entry.path();

        if is_archive(&path) {
            extract_archive(&path, &s_dir)?;
        } else if is_patch(&path) {
            // Copy patches to WORKDIR for later application
            fs::copy(&path, &workdir.join(entry.file_name()))?;
        }
    }

    Ok(TaskOutput::success())
}

fn is_archive(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| matches!(ext, "tar.gz" | "tar.bz2" | "tar.xz" | "tgz" | "zip"))
        .unwrap_or(false)
}

fn extract_archive(archive: &Path, dest: &Path) -> ExecutionResult<()> {
    // Use existing archive extraction code
    // ... implementation ...
}
```

### 3.3: Real do_patch Implementation

**Approach:** Use git apply + patch command

```rust
fn execute_do_patch(&self, spec: &TaskSpec) -> ExecutionResult<TaskOutput> {
    let s_dir = spec.environment.get("S")
        .map(PathBuf::from)
        .ok_or_else(|| ExecutionError::MissingVariable("S".to_string()))?;

    let workdir = &spec.workdir;

    // Find patch files
    let mut patches: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(workdir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("patch") {
            patches.push(path);
        }
    }

    // Sort patches (0001-xxx.patch comes before 0002-xxx.patch)
    patches.sort();

    // Apply patches
    for patch in patches {
        self.apply_patch(&patch, &s_dir, 1)?; // Default -p1
    }

    Ok(TaskOutput::success())
}

fn apply_patch(&self, patch: &Path, target_dir: &Path, strip: usize) -> ExecutionResult<()> {
    // Try git apply first (better error messages)
    let git_result = Command::new("git")
        .args(&["apply", &format!("-p{}", strip), patch.to_str().unwrap()])
        .current_dir(target_dir)
        .output()?;

    if git_result.status.success() {
        return Ok(());
    }

    // Fall back to patch command
    let patch_result = Command::new("patch")
        .args(&[&format!("-p{}", strip), "-i", patch.to_str().unwrap()])
        .current_dir(target_dir)
        .output()?;

    if !patch_result.status.success() {
        return Err(ExecutionError::PatchFailed(
            String::from_utf8_lossy(&patch_result.stderr).to_string()
        ));
    }

    Ok(())
}
```

### 3.4: Real do_configure Implementation

**Enhance autotools_do_configure in prelude.sh:**

Already implemented! Lines 159-174 in bbhelpers.rs have working autotools_do_configure.

**Add detection in executor:**

```rust
fn execute_do_configure(&self, spec: &TaskSpec) -> ExecutionResult<TaskOutput> {
    let s_dir = spec.environment.get("S")
        .map(PathBuf::from)
        .ok_or_else(|| ExecutionError::MissingVariable("S".to_string()))?;

    // Detect build system
    if s_dir.join("CMakeLists.txt").exists() {
        self.execute_cmake_configure(spec)?;
    } else if s_dir.join("meson.build").exists() {
        self.execute_meson_configure(spec)?;
    } else if s_dir.join("configure").exists() || s_dir.join("configure.ac").exists() {
        self.execute_autotools_configure(spec)?;
    } else {
        // No-op configure (base_do_configure)
        eprintln!("No configure step needed");
    }

    Ok(TaskOutput::success())
}
```

---

## Phase 4: .bbclass Parsing (2 weeks)
**Priority: HIGH** | **Status: Not implemented - hardcoded only**

### Current Limitation

`convenient-bitbake/src/class_dependencies.rs` has hardcoded mappings:

```rust
// Lines 10-120: Hardcoded for ~30 classes
"autotools" => vec!["autoconf-native", "automake-native", "libtool-native"],
"cmake" => vec!["cmake-native"],
```

**Problem:** Won't work with custom classes - breaks Yocto extensibility

### Implementation Approach

**Parse actual .bbclass files** using existing parser:

```rust
// New module: convenient-bitbake/src/class_parser.rs

use crate::parser::BitbakeParser;
use std::path::{Path, PathBuf};
use std::collections::HashMap;

pub struct ClassRegistry {
    /// Loaded classes: class_name -> ClassDefinition
    classes: HashMap<String, ClassDefinition>,

    /// Search paths for .bbclass files
    search_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ClassDefinition {
    /// Class name (e.g., "autotools")
    pub name: String,

    /// Full path to .bbclass file
    pub file_path: PathBuf,

    /// Dependencies declared in class (DEPENDS, RDEPENDS)
    pub build_depends: Vec<String>,
    pub runtime_depends: Vec<String>,

    /// Other classes this class inherits
    pub inherits: Vec<String>,

    /// Variables set by this class
    pub variables: HashMap<String, String>,

    /// Task definitions in this class
    pub tasks: Vec<String>,
}

impl ClassRegistry {
    pub fn new(search_paths: Vec<PathBuf>) -> Self {
        Self {
            classes: HashMap::new(),
            search_paths,
        }
    }

    /// Find .bbclass file in search paths
    fn find_class_file(&self, class_name: &str) -> Option<PathBuf> {
        for search_path in &self.search_paths {
            let class_file = search_path.join("classes").join(format!("{}.bbclass", class_name));
            if class_file.exists() {
                return Some(class_file);
            }
        }
        None
    }

    /// Load and parse a .bbclass file
    pub fn load_class(&mut self, class_name: &str) -> Result<(), ClassError> {
        // Check cache first
        if self.classes.contains_key(class_name) {
            return Ok(());
        }

        // Find class file
        let class_file = self.find_class_file(class_name)
            .ok_or_else(|| ClassError::ClassNotFound(class_name.to_string()))?;

        // Parse class file using existing parser
        let parser = BitbakeParser::new();
        let content = std::fs::read_to_string(&class_file)?;
        let parse_result = parser.parse(&content)?;

        // Extract class definition
        let class_def = ClassDefinition {
            name: class_name.to_string(),
            file_path: class_file,
            build_depends: parse_result.depends.clone(),
            runtime_depends: parse_result.rdepends.clone(),
            inherits: parse_result.inherits.clone(),
            variables: parse_result.variables.clone(),
            tasks: parse_result.tasks.keys().cloned().collect(),
        };

        // Cache class definition
        self.classes.insert(class_name.to_string(), class_def);

        // Recursively load inherited classes
        for inherited in &parse_result.inherits {
            self.load_class(inherited)?;
        }

        Ok(())
    }

    /// Get all dependencies for a class (including inherited)
    pub fn get_class_dependencies(&self, class_name: &str) -> Vec<String> {
        let mut deps = Vec::new();
        let mut visited = std::collections::HashSet::new();

        self.collect_dependencies_recursive(class_name, &mut deps, &mut visited);

        deps
    }

    fn collect_dependencies_recursive(
        &self,
        class_name: &str,
        deps: &mut Vec<String>,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if visited.contains(class_name) {
            return;
        }
        visited.insert(class_name.to_string());

        if let Some(class_def) = self.classes.get(class_name) {
            // Add this class's dependencies
            deps.extend(class_def.build_depends.clone());

            // Recursively add inherited class dependencies
            for inherited in &class_def.inherits {
                self.collect_dependencies_recursive(inherited, deps, visited);
            }
        }
    }
}
```

### Integration

Replace hardcoded `class_dependencies.rs` with dynamic `ClassRegistry`:

```rust
// In recipe_extractor.rs or similar
impl RecipeExtractor {
    pub fn extract_with_classes(&mut self, class_registry: &mut ClassRegistry) -> Result<Recipe> {
        let recipe = self.extract_basic()?;

        // Load all inherited classes
        for class_name in &recipe.inherits {
            class_registry.load_class(class_name)?;

            // Add class dependencies to recipe dependencies
            let class_deps = class_registry.get_class_dependencies(class_name);
            recipe.depends.extend(class_deps);
        }

        Ok(recipe)
    }
}
```

---

## Phase 5: Connect Remote Cache (1 week)
**Priority: MEDIUM** | **Status: gRPC client exists, needs wiring**

### Current Status

`convenient-bitbake/src/executor/remote_cache.rs` has TODOs:
- Line 78: `// TODO: Implement gRPC call to remote cache`
- Line 96: `// TODO: Implement gRPC call to remote cache`
- Line 111: `// TODO: Implement gRPC call to remote cache`
- Line 124: `// TODO: Implement gRPC call to remote cache`

### Implementation

**Use existing convenient-cache gRPC client:**

```rust
// In remote_cache.rs
use convenient_cache::grpc_client::RemoteExecutionClient;

impl RemoteCacheClient {
    pub fn get_action_result(
        &self,
        action_digest: &ContentHash,
    ) -> ExecutionResult<Option<ActionResult>> {
        // Try local cache first
        if let Some(result) = self.local_cache.get_action_result(action_digest)? {
            return Ok(Some(result));
        }

        // Try remote cache if configured
        if let Some(url) = &self.config.url {
            // NEW: Actually call gRPC
            let runtime = tokio::runtime::Runtime::new()?;
            let client = runtime.block_on(async {
                RemoteExecutionClient::connect(url.clone()).await
            })?;

            let digest_proto = action_digest.to_proto();

            let response = runtime.block_on(async {
                client.get_action_result(digest_proto).await
            });

            if let Ok(Some(remote_result)) = response {
                // Convert proto result to ActionResult
                let action_result = ActionResult::from_proto(remote_result)?;

                // Cache locally for future use
                self.local_cache.put_action_result(action_digest, &action_result)?;

                return Ok(Some(action_result));
            }
        }

        Ok(None)
    }

    pub fn put_action_result(
        &self,
        action_digest: &ContentHash,
        result: &ActionResult,
    ) -> ExecutionResult<()> {
        // Store in local cache
        self.local_cache.put_action_result(action_digest, result)?;

        // Upload to remote cache if configured
        if let Some(url) = &self.config.url {
            // NEW: Actually upload via gRPC
            let runtime = tokio::runtime::Runtime::new()?;
            let client = runtime.block_on(async {
                RemoteExecutionClient::connect(url.clone()).await
            })?;

            let digest_proto = action_digest.to_proto();
            let result_proto = result.to_proto();

            runtime.block_on(async {
                client.update_action_result(digest_proto, result_proto).await
            })?;
        }

        Ok(())
    }
}
```

### Configuration

Add remote cache URL to build config:

```bash
# In local.conf or kas.yml
REMOTE_CACHE_URL = "grpc://cache.example.com:8980"
REMOTE_CACHE_INSTANCE = "hitzeleiter-builds"
```

---

## Phase 6: Enforce Hermetic Builds (2-4 weeks)
**Priority: MEDIUM** | **Status: Architecture designed, not enforced**

### Current Issue

From `docs/architecture/sandbox-design.md` (lines 156-171):
> "Current implementation is 'Wrong' because it doesn't properly map dependency outputs or create hermetic builds."

### What Hermetic Builds Require

1. **Explicit input declaration** - TaskSpec must declare all inputs
2. **Input verification** - Sandbox only allows access to declared inputs
3. **Output tracking** - All outputs must be in declared output directories
4. **No system access** - Can't read /usr/lib or other system paths
5. **Symlink farm** - Bazel-style input staging

### Implementation

**Step 1: Add input/output declaration to TaskSpec**

```rust
// In types.rs
pub struct TaskSpec {
    // ... existing fields ...

    /// Declared inputs (files task is allowed to read)
    pub inputs: Vec<InputFile>,

    /// Declared output directories
    pub output_dirs: Vec<PathBuf>,

    /// Whether to enforce hermeticity (fail if undeclared access)
    pub hermetic: bool,
}

#[derive(Debug, Clone)]
pub struct InputFile {
    /// Logical path in sandbox (e.g., "sysroot/usr/lib/libc.so")
    pub sandbox_path: PathBuf,

    /// Actual path in cache/artifact store
    pub cache_path: PathBuf,

    /// Content hash for verification
    pub digest: ContentHash,
}
```

**Step 2: Create input symlink farm**

```rust
// In executor.rs
fn create_input_symlinks(
    &self,
    inputs: &[InputFile],
    sandbox_root: &Path,
) -> ExecutionResult<()> {
    for input in inputs {
        let link_path = sandbox_root.join(&input.sandbox_path);

        // Create parent directories
        if let Some(parent) = link_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Create symlink to actual file
        #[cfg(unix)]
        std::os::unix::fs::symlink(&input.cache_path, &link_path)?;

        #[cfg(not(unix))]
        fs::copy(&input.cache_path, &link_path)?;
    }

    Ok(())
}
```

**Step 3: Re-enable mount namespaces**

Currently disabled in `native_sandbox.rs` (line 661-670).

Need to:
1. Debug why they were disabled
2. Re-enable CLONE_NEWNS
3. Bind-mount only declared inputs
4. Make / read-only except for output dirs

---

## Phase 7: Remove All unwrap() Calls (Ongoing)
**Priority: MEDIUM** | **Status: 305 instances found**

### Strategy

1. **Never use unwrap() in library code** - always return Result<>
2. **Tests can use unwrap()** - acceptable in test code
3. **Replace with proper error handling:**

**Before:**
```rust
let value = map.get("key").unwrap();
```

**After:**
```rust
let value = map.get("key")
    .ok_or_else(|| ExecutionError::MissingKey("key".to_string()))?;
```

### Systematic Replacement

```bash
# Find all unwrap() calls (excluding tests)
rg '\.unwrap\(\)' convenient-bitbake/src --type rust

# Replace patterns:
.unwrap() → .context("error message")?  # Using anyhow
.unwrap() → .map_err(|e| CustomError::from(e))?
.unwrap() → .ok_or_else(|| Error::Missing)?
```

---

## Testing Strategy

### End-to-End Test: Busybox for qemuarm64

**Goal:** Single command builds busybox

```bash
hitzeleiter kas config.yml && hitzeleiter build busybox
```

**Test Steps:**
1. Set up minimal KAS configuration
2. Configure for qemuarm64 machine
3. Run hitzeleiter build busybox
4. Verify:
   - All tasks execute successfully
   - Sysroot assembled from dependencies
   - Cross-compilation uses aarch64 toolchain
   - Output binary is AArch64 ELF
   - Cache reuse works on second build

### Unit Tests

Each phase must have:
- ✅ Unit tests for new functions
- ✅ Integration tests for new workflows
- ✅ All existing tests still pass

### Acceptance Criteria

**Phase 1 (Sysroot):** Recipe-specific sysroots created successfully
**Phase 2 (Toolchain):** Cross-compiler detected and used
**Phase 3 (Tasks):** fetch, unpack, patch, configure, compile, install all work
**Phase 4 (Classes):** Custom .bbclass files parsed correctly
**Phase 5 (Cache):** Remote cache uploads and downloads work
**Phase 6 (Hermetic):** Tasks fail if accessing undeclared inputs
**Phase 7 (unwrap):** No unwrap() in src/ (tests OK)

---

## Timeline

| Phase | Duration | Dependencies | Risk |
|-------|----------|--------------|------|
| 1. Sysroot | 1 week | None | Low (code exists) |
| 2. Toolchain | 2 weeks | None | Medium (toolchain detection) |
| 3. Tasks | 4-6 weeks | 1, 2 | High (many edge cases) |
| 4. .bbclass | 2 weeks | None | Low (parser exists) |
| 5. Remote cache | 1 week | None | Low (gRPC client exists) |
| 6. Hermetic | 2-4 weeks | 1, 3 | High (complex) |
| 7. unwrap() | Ongoing | None | Low (refactoring) |

**Total Sequential:** 12-17 weeks (3-4 months)
**With Parallelization:** 8-12 weeks (2-3 months)

### Parallel Tracks

- **Track A:** Phases 1 → 3 (execution path)
- **Track B:** Phases 2, 4 (infrastructure)
- **Track C:** Phases 5, 6 (optional enhancements)
- **Track D:** Phase 7 (continuous cleanup)

---

## Success Metrics

**Milestone 1 (MVP):** Busybox builds successfully
- Phases 1, 2, 3 complete
- ~8 weeks

**Milestone 2 (Production):** Can replace BitBake for simple recipes
- All phases complete
- ~12 weeks

**Milestone 3 (Excellence):** Better than BitBake
- Performance optimization
- Full Bazel-like features
- ~16 weeks

---

## Next Steps

1. **Start with Phase 1 (Sysroot)** - Lowest risk, highest impact
2. **Then Phase 2 (Toolchain)** - Required for cross-compilation
3. **Then Phase 3.1 (Fetch)** - Get sources downloading
4. **Build incrementally** - Test after each phase

**Ready to begin implementation?**
