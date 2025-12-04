# Implementation Plan - Q1 2026

**Created**: November 2025
**Status**: Active
**Goal**: Production-ready builds for simple recipes

Based on:
- [Code Quality Audit](../../analysis/code-quality-audit.md)
- [Parser Architecture Assessment](../../analysis/parser-architecture-assessment.md)
- [Capability Matrix](capability-matrix.md)

---

## 📅 **Timeline Overview**

```
Week 1 (Immediate):  Fix failing tests, critical blockers
Week 2-4 (Month 1):  Split god objects, parser gaps
Month 2-3 (Quarter): Production hardening, integration tests
```

---

## 🔴 **WEEK 1: IMMEDIATE (Critical Blockers)**

### **Goal**: Green test suite, basic end-to-end working

#### **Task 1.1: Fix All 17 Failing Tests** 🚨
**Priority**: CRITICAL - Cannot proceed until done
**Estimated**: 2-3 days
**Owner**: TBD

**Failing Tests Identified**:
```
Executor tests (11 failures):
- test_direct_execution_simple
- test_direct_execution_mkdir
- test_auto_detect_execution_mode
- test_direct_rust_execution_no_sandbox
- test_simple_task_execution
- test_task_caching
- test_executor_pool_aggregate_stats
- test_executor_pool_ping_all
- test_sandbox_write_file
- test_simple_execution (native_sandbox)
- test_backend_detection

Sysroot tests (3 failures):
- test_hardlink_tree_builder
- test_sysroot_assembler_conflict_detection
- test_sysroot_assembler_success

Config tests (1 failure):
- test_nested_expansion

Other (2 failures):
- test_metrics (execution_log)
- test_determine_simple_script (script_analyzer)
```

**Action Items**:
- [ ] Run each test individually to understand failure
- [ ] Fix executor path issues (likely temp dir problems)
- [ ] Fix sysroot hardlink issues (likely permissions)
- [ ] Update test expectations if BitBake semantics changed
- [ ] Ensure all tests pass: `cargo test --lib`

**Success Criteria**: `cargo test` shows 0 failures

---

#### **Task 1.2: Reduce Critical unwrap() Calls** 🚨
**Priority**: HIGH - Production crashes waiting to happen
**Estimated**: 3 days
**Owner**: TBD

**Focus Files** (Top 5 hot paths):
1. `convenient-bitbake/src/executor/executor.rs` (~100 unwraps)
2. `convenient-bitbake/src/build_orchestrator.rs` (~50 unwraps)
3. `convenient-bitbake/src/executor/rust_fetcher.rs` (~40 unwraps)
4. `convenient-bitbake/src/pipeline.rs` (~30 unwraps)
5. `hitzeleiter/src/commands/build.rs` (~20 unwraps)

**Action Items**:
- [ ] Audit executor.rs for panicking unwraps
- [ ] Replace with proper Result<> error handling
- [ ] Use `?` operator for error propagation
- [ ] Keep unwraps only in tests and unreachable code paths
- [ ] Add `expect()` with clear messages where unwrap is necessary

**Pattern to Replace**:
```rust
// BAD:
let value = map.get(&key).unwrap();

// GOOD:
let value = map.get(&key)
    .ok_or_else(|| ExecutionError::MissingValue(key.clone()))?;
```

**Success Criteria**: <100 unwraps in production code paths

---

#### **Task 1.3: Layer Dependency Validation** 🔴
**Priority**: CRITICAL - Prevents broken builds
**Estimated**: 1-2 days
**Owner**: TBD

**File**: `convenient-bitbake/src/layer_context.rs`

**Current Gap**:
```rust
// Layer depends field exists but not validated
pub depends: Vec<String>,  // ← No checks!
```

**Implementation**:
```rust
impl BuildContext {
    /// Validate all layer dependencies are satisfied
    pub fn validate_layer_dependencies(&self) -> Result<(), String> {
        let collections: HashSet<_> = self.layers
            .iter()
            .map(|l| l.collection.as_str())
            .collect();

        for layer in &self.layers {
            for dep in &layer.depends {
                if !collections.contains(dep.as_str()) {
                    return Err(format!(
                        "Layer '{}' depends on '{}' which is not present",
                        layer.collection, dep
                    ));
                }
            }
        }

        // Check for circular dependencies
        self.check_circular_deps()?;

        Ok(())
    }

    fn check_circular_deps(&self) -> Result<(), String> {
        // Topological sort algorithm
        // Return error if cycle detected
    }
}
```

**Action Items**:
- [ ] Implement `validate_layer_dependencies()`
- [ ] Implement circular dependency detection
- [ ] Call validation after all layers added
- [ ] Add tests for validation
- [ ] Document error messages

**Success Criteria**:
- Detects missing dependencies
- Detects circular dependencies
- Clear error messages

---

#### **Task 1.4: Read MACHINEOVERRIDES from Config** 🔴
**Priority**: HIGH - Stop guessing architecture
**Estimated**: 1 day
**Owner**: TBD

**File**: `convenient-bitbake/src/override_resolver.rs`

**Current Problem**:
```rust
// Heuristic - WRONG!
if machine.contains("arm") {
    overrides.push("arm".to_string());
}
```

**Fix**:
```rust
pub fn build_overrides_from_machine_conf(
    &mut self,
    machine_conf_vars: &HashMap<String, String>,
    distro_conf_vars: &HashMap<String, String>,
) {
    // Read actual MACHINEOVERRIDES
    if let Some(machine_overrides) = machine_conf_vars.get("MACHINEOVERRIDES") {
        for override_str in machine_overrides.split(':') {
            self.active_overrides.push(override_str.to_string());
        }
    }

    // Read actual DISTROOVERRIDES
    if let Some(distro_overrides) = distro_conf_vars.get("DISTROOVERRIDES") {
        for override_str in distro_overrides.split(':') {
            self.active_overrides.push(override_str.to_string());
        }
    }
}
```

**Action Items**:
- [ ] Parse MACHINEOVERRIDES from machine.conf
- [ ] Parse DISTROOVERRIDES from distro.conf
- [ ] Remove heuristic architecture detection
- [ ] Update tests to use real configs
- [ ] Verify override order matches BitBake

**Success Criteria**: Uses actual OVERRIDES values, no guessing

---

## 🟡 **WEEKS 2-4: THIS MONTH (Architectural Improvements)**

### **Goal**: Clean code structure, core features complete

#### **Task 2.1: Split simple_python_eval.rs** 🔴
**Priority**: CRITICAL - 3,001 line god object
**Estimated**: 5 days
**Owner**: TBD

**Current**:
```
simple_python_eval.rs: 3,001 lines (UNACCEPTABLE)
```

**Target Structure**:
```
convenient-bitbake/src/python_eval/
├── mod.rs              (200 lines) - Main API
├── bb_utils.rs         (500 lines) - bb.utils.* functions
├── oe_utils.rs         (400 lines) - oe.utils.* functions
├── list_ops.rs         (400 lines) - List comprehensions, literals
├── string_ops.rs       (300 lines) - String operations
├── conditional.rs      (300 lines) - if/else, ternary
├── getvar.rs           (200 lines) - d.getVar() handling
└── tests/
    ├── bb_utils_tests.rs
    ├── oe_utils_tests.rs
    └── ...
```

**Action Items**:
- [ ] Create `python_eval/` module directory
- [ ] Extract bb.utils functions to bb_utils.rs
- [ ] Extract oe.utils functions to oe_utils.rs
- [ ] Extract list operations to list_ops.rs
- [ ] Extract string operations to string_ops.rs
- [ ] Extract conditionals to conditional.rs
- [ ] Move tests to dedicated test files
- [ ] Update imports in dependent modules
- [ ] Verify all tests still pass

**Success Criteria**:
- No file >500 lines
- All tests passing
- Same functionality

---

#### **Task 2.2: Split recipe_extractor.rs** 🔴
**Priority**: CRITICAL - 2,447 line god object
**Estimated**: 3 days
**Owner**: TBD

**Current**:
```
recipe_extractor.rs: 2,447 lines (UNACCEPTABLE)
```

**Target Structure**:
```
convenient-bitbake/src/extraction/
├── mod.rs                   (300 lines) - Main RecipeExtractor
├── variable_resolver.rs     (600 lines) - Variable resolution
├── dependency_extractor.rs  (500 lines) - DEPENDS, RDEPENDS
├── task_extractor.rs        (400 lines) - Task extraction
├── inheritance_resolver.rs  (400 lines) - inherit, include
└── provider_resolver.rs     (300 lines) - PROVIDES resolution
```

**Action Items**:
- [ ] Create `extraction/` module directory
- [ ] Extract variable resolution logic
- [ ] Extract dependency extraction logic
- [ ] Extract task extraction (use existing task_extractor)
- [ ] Extract inheritance resolution
- [ ] Extract provider resolution
- [ ] Update imports
- [ ] Verify all tests pass

**Success Criteria**:
- No file >600 lines
- All tests passing
- Same functionality

---

#### **Task 2.3: Implement BBFILES Glob Expansion** 🟡
**Priority**: HIGH - Actually use BBFILES patterns
**Estimated**: 2-3 days
**Owner**: TBD

**File**: `convenient-bitbake/src/layer_context.rs`

**Current Problem**:
```rust
pub fn get_bbfiles(&self) -> Vec<String> {
    self.variables.get("BBFILES")
        .map(|s| vec![s.clone()])  // ← Returns pattern, not files!
        .unwrap_or_default()
}
```

**Fix**:
```rust
use glob::glob;

pub fn get_bbfiles(&self) -> Result<Vec<PathBuf>, String> {
    let pattern = self.variables.get("BBFILES")
        .ok_or("BBFILES not set")?;

    // Expand ${LAYERDIR}
    let expanded = pattern.replace("${LAYERDIR}",
        self.layer_dir.to_str().unwrap());

    // Handle space-separated patterns
    let mut files = Vec::new();
    for pattern in expanded.split_whitespace() {
        for entry in glob(pattern).map_err(|e| e.to_string())? {
            files.push(entry.map_err(|e| e.to_string())?);
        }
    }

    Ok(files)
}
```

**Action Items**:
- [ ] Add `glob` crate dependency
- [ ] Implement variable expansion in patterns
- [ ] Handle multiple patterns (space-separated)
- [ ] Return actual file paths
- [ ] Add tests for glob expansion
- [ ] Update recipe discovery to use expanded patterns

**Success Criteria**: Returns actual .bb files, not patterns

---

#### **Task 2.4: Implement BBCLASSEXTEND** 🟡
**Priority**: MEDIUM - Virtual recipes support
**Estimated**: 3-5 days
**Owner**: TBD

**File**: New `convenient-bitbake/src/class_extend.rs`

**Feature**:
```bitbake
# In busybox_1.36.1.bb:
BBCLASSEXTEND = "native nativesdk"

# Should create virtual recipes:
# - busybox-native
# - busybox-nativesdk
```

**Implementation**:
```rust
pub struct ClassExtender {
    base_recipe: Recipe,
}

impl ClassExtender {
    pub fn extend(&self, classes: &[String]) -> Vec<Recipe> {
        let mut virtual_recipes = Vec::new();

        for class in classes {
            let mut recipe = self.base_recipe.clone();
            recipe.name = format!("{}-{}", recipe.name, class);

            // Apply class-specific overrides
            match class.as_str() {
                "native" => self.apply_native_overrides(&mut recipe),
                "nativesdk" => self.apply_nativesdk_overrides(&mut recipe),
                _ => {}
            }

            virtual_recipes.push(recipe);
        }

        virtual_recipes
    }
}
```

**Action Items**:
- [ ] Create class_extend.rs module
- [ ] Implement ClassExtender
- [ ] Handle -native class overrides
- [ ] Handle -nativesdk class overrides
- [ ] Integrate with RecipeGraph
- [ ] Add tests
- [ ] Document virtual recipe semantics

**Success Criteria**: Creates virtual recipes for BBCLASSEXTEND

---

#### **Task 2.5: Define Stable Public API** 🟡
**Priority**: MEDIUM - API stability
**Estimated**: 2 days
**Owner**: TBD

**Current Problem**: 60+ types exported, no clear API boundary

**Action Items**:
- [ ] Audit all `pub use` in lib.rs
- [ ] Mark internal types as `pub(crate)`
- [ ] Define core API (<20 types):
  - BuildOrchestrator
  - Pipeline
  - TaskExecutor
  - RecipeGraph
  - TaskGraph
  - BuildEnvironment
  - Error types
- [ ] Document stability guarantees
- [ ] Add `#[non_exhaustive]` to extensible types
- [ ] Version public API (0.1.0 → 0.2.0 for breaking changes)

**Success Criteria**: Clear stable API, internal types hidden

---

## 🔵 **MONTHS 2-3: THIS QUARTER (Production Hardening)**

### **Goal**: Production-ready for simple recipes

#### **Task 3.1: Reduce clone() Calls** 🟢
**Priority**: MEDIUM - Performance
**Estimated**: 1 week
**Owner**: TBD

**Focus**:
- Use `&str` instead of `String` where possible
- Use `Cow<'_, str>` for sometimes-owned strings
- Pass `&HashMap` instead of cloning
- Use `Arc<str>` for shared immutable strings

**Files**:
- pipeline.rs
- recipe_extractor.rs
- build_orchestrator.rs

**Success Criteria**: <150 clones (from 401)

---

#### **Task 3.2: Add Custom Error Types** 🟢
**Priority**: MEDIUM - Better error ergonomics
**Estimated**: 1 week
**Owner**: TBD

**Pattern** (from executor/types.rs):
```rust
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid syntax at {line}:{col}: {message}")]
    Syntax { line: usize, col: usize, message: String },

    #[error("Unknown variable: {0}")]
    UnknownVariable(String),
}
```

**Action Items**:
- [ ] Replace `Box<dyn Error>` with specific error types
- [ ] Add error types for: Parser, Resolver, Fetcher, Orchestrator
- [ ] Use thiserror for implementations
- [ ] Improve error messages
- [ ] Add context with error chains

**Success Criteria**: No `Box<dyn Error>` in public API

---

#### **Task 3.3: End-to-End Integration Tests** 🟢
**Priority**: HIGH - Prove it works
**Estimated**: 1 week
**Owner**: TBD

**Test Cases**:
```rust
#[test]
fn test_busybox_build_complete() {
    // Setup KAS environment
    // Parse recipes
    // Build task graph
    // Execute all tasks
    // Verify binary produced
    // Verify binary runs in qemu
}

#[test]
fn test_hello_world_build() {
    // Simple recipe end-to-end
}

#[test]
fn test_multi_layer_build() {
    // Test layer priority and overrides
}
```

**Action Items**:
- [ ] Create integration test suite
- [ ] Setup test fixtures (minimal Poky)
- [ ] Test complete build pipeline
- [ ] Test with real recipes
- [ ] Document test environment setup

**Success Criteria**:
- At least 3 recipes build end-to-end
- Tests run in CI

---

#### **Task 3.4: Performance Benchmarking** 🟢
**Priority**: MEDIUM - Know where you stand
**Estimated**: 3 days
**Owner**: TBD

**Use criterion.rs**:
```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn recipe_parsing_benchmark(c: &mut Criterion) {
    c.bench_function("parse busybox recipe", |b| {
        b.iter(|| {
            BitbakeRecipe::parse_file(black_box("busybox_1.36.1.bb"))
        });
    });
}
```

**Benchmarks**:
- Recipe parsing
- Variable expansion
- Graph building
- Signature computation
- Task execution

**Action Items**:
- [ ] Add criterion dependency
- [ ] Create benchmarks/ directory
- [ ] Benchmark hot paths
- [ ] Compare to BitBake baseline
- [ ] Profile with flamegraph
- [ ] Document results

**Success Criteria**: Know performance characteristics

---

#### **Task 3.5: Dependency Audit & Cleanup** 🟢
**Priority**: LOW - Hygiene
**Estimated**: 1 day
**Owner**: TBD

**Issues**:
- Duplicate dependencies (base64, bitflags)
- 1,229 total dependencies (heavy)

**Action Items**:
- [ ] Run `cargo tree --duplicates`
- [ ] Update dependencies to same versions
- [ ] Consider alternatives to heavy deps
- [ ] Remove unused dependencies
- [ ] Document dependency rationale

**Success Criteria**: No duplicate versions

---

## 📊 **Progress Tracking**

### **Week 1 Checklist**:
- [ ] Task 1.1: Fix 17 failing tests ← BLOCKER
- [ ] Task 1.2: Reduce critical unwraps
- [ ] Task 1.3: Layer dependency validation
- [ ] Task 1.4: Read MACHINEOVERRIDES from config

### **Month 1 Checklist**:
- [ ] Task 2.1: Split simple_python_eval.rs
- [ ] Task 2.2: Split recipe_extractor.rs
- [ ] Task 2.3: BBFILES glob expansion
- [ ] Task 2.4: BBCLASSEXTEND support
- [ ] Task 2.5: Define stable public API

### **Quarter 1 Checklist**:
- [ ] Task 3.1: Reduce clone() calls
- [ ] Task 3.2: Custom error types
- [ ] Task 3.3: End-to-end integration tests
- [ ] Task 3.4: Performance benchmarking
- [ ] Task 3.5: Dependency audit

---

## 🎯 **Success Metrics**

### **Week 1 Success**:
- ✅ All 412 tests passing (0 failures)
- ✅ <100 unwraps in production code
- ✅ Layer dependency validation working
- ✅ OVERRIDES read from configs

### **Month 1 Success**:
- ✅ No file >600 lines
- ✅ BBFILES patterns expanded
- ✅ Virtual recipes supported
- ✅ Stable API defined

### **Quarter 1 Success**:
- ✅ <150 clone() calls (from 401)
- ✅ Custom error types everywhere
- ✅ 3+ recipes build end-to-end
- ✅ Performance benchmarked
- ✅ Grade improves from C+ to B+

---

## 🚀 **Execution Strategy**

### **Parallel Work Streams**:

**Stream A (Critical Path)**:
Week 1 → Month 1 → Quarter 1

**Stream B (Documentation)**:
- Update capability matrix as features complete
- Document each major change
- Keep roadmap current

**Stream C (Testing)**:
- Fix tests as blockers
- Add integration tests ongoing
- Regression testing for each change

### **Review Points**:
- **End of Week 1**: Test suite green, critical blockers fixed
- **End of Month 1**: Architecture cleaned up, features complete
- **End of Quarter 1**: Production-ready assessment

---

## 📋 **Risk Management**

### **High Risk Items**:
1. **Test failures may reveal deeper issues** - Budget extra time
2. **God object refactoring may break things** - Extensive testing needed
3. **End-to-end tests may find new gaps** - Iterative approach

### **Mitigation**:
- Fix tests FIRST before any refactoring
- Refactor incrementally with tests after each step
- Keep capability matrix updated as reality check

---

**Last Updated**: November 2025
**Next Review**: End of Week 1
