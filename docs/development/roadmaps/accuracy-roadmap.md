# BitBake Dependency Extraction: Phased Accuracy Roadmap
## From 82-84% → 85% → 95%

**Current Status:** 78.3% baseline (36/46 recipes), projected 82-84% with Phase 7d-e
**Date:** 2025-11-12
**Analysis Basis:** 46 Yocto Scarthgap recipes, 118 passing unit tests

---

## PART 1: IMMEDIATE WORK - REACH 85% TARGET

### Overview
**Goal:** Close 1-3% gap from projected 82-84% to 85%
**Timeline:** 12-18 hours
**Risk:** Low (building on proven patterns)
**Approach:** Target remaining low-hanging fruit in .bbclass parsing and Python evaluation

---

### Phase 7f: Enhanced .bbclass Python Pattern Handling
**Priority:** HIGH | **ROI:** HIGH | **Complexity:** MEDIUM

**Objective:** Parse and evaluate more Python patterns in dynamically loaded .bbclass files

**Current Gap:**
- .bbclass files are parsed for static DEPENDS (line 185-210, class_dependencies.rs)
- Python expressions like `${@...}` are skipped for safety (line 251)
- Missing ~2-3% of dependencies from conditional class logic

**Improvements:**

1. **Parse .bbclass Python Expressions** (4-6 hours)
   - Extend parse_class_file() to handle ${@bb.utils.contains(...)} in DEPENDS
   - Use SimplePythonEvaluator for safe evaluation with class context
   - Fallback to skip if expression too complex
   
   ```rust
   // In class_dependencies.rs::extract_depends_from_line()
   fn extract_depends_from_line(line: &str, var_name: &str, evaluator: Option<&SimplePythonEvaluator>) -> Option<Vec<String>> {
       // Current code handles static values...
       
       // NEW: Handle Python expressions if evaluator provided
       if value.contains("${@") && evaluator.is_some() {
           let expanded = evaluator.unwrap().evaluate(&value)?;
           return Some(expanded.split_whitespace().map(String::from).collect());
       }
       
       // Existing static parsing...
   }
   ```

2. **Handle Conditional Class Dependencies** (2-3 hours)
   - Some classes add dependencies only if certain conditions met
   - Example: systemd.bbclass checks DISTRO_FEATURES
   - Parse simple if/elif/else blocks in .bbclass files
   
   ```python
   # Example pattern in .bbclass:
   if bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d):
       DEPENDS += "systemd"
   ```

3. **Add Class Variable Context** (2-3 hours)
   - Pass recipe variables to class parser for better evaluation
   - Enables evaluating ${PN}, ${BPN} references in .bbclass files
   - Some classes use DEPENDS += "${BPN}-native"

**Files to Modify:**
- `class_dependencies.rs`:
  - parse_class_file() - add evaluator parameter (+30 lines)
  - extract_depends_from_line() - evaluate Python expressions (+40 lines)
- `recipe_extractor.rs`:
  - extract_class_dependencies() - pass evaluator to parser (+10 lines)

**Expected Impact:** +1.5-2.0% accuracy
**Accuracy Target:** 83.5-86%
**Test Strategy:**
- Add tests for Python expressions in mock .bbclass files
- Test with real autotools.bbclass, systemd.bbclass
- Verify no regression on existing 118 tests

---

### Phase 7g: PACKAGECONFIG Edge Cases & Enhancements
**Priority:** MEDIUM | **ROI:** MEDIUM | **Complexity:** LOW-MEDIUM

**Objective:** Handle remaining PACKAGECONFIG patterns and edge cases

**Current Gap:**
- PACKAGECONFIG basic parsing works (line 554-624, recipe_extractor.rs)
- Missing: nested expressions, default value resolution, conditional enables

**Improvements:**

1. **PACKAGECONFIG Default Resolution** (2-3 hours)
   - Handle ??= (default assignment) properly
   - PACKAGECONFIG ??= "systemd ${@bb.utils.filter(...)}"
   - Currently may not resolve complex defaults correctly
   
   ```rust
   // In recipe_extractor.rs::parse_variables()
   // Enhanced operator handling for PACKAGECONFIG specifically
   fn resolve_packageconfig_defaults(value: &str, evaluator: &SimplePythonEvaluator) -> String {
       // Expand all ${@...} expressions in default value
       // Split result by whitespace for active options
   }
   ```

2. **PACKAGECONFIG Variable References** (2-3 hours)
   - Handle ${PACKAGECONFIG_X11}, ${PACKAGECONFIG_GL}
   - Recipes sometimes reference sub-configs
   - Need variable expansion before parsing
   
3. **Conditional PACKAGECONFIG Lines** (1-2 hours)
   - PACKAGECONFIG:append:class-target = "extra"
   - Apply override resolution to PACKAGECONFIG itself

**Files to Modify:**
- `recipe_extractor.rs`:
  - parse_packageconfig() - handle ??= and nested expressions (+25 lines)
  - extract_packageconfig_deps() - resolve variable refs (+15 lines)
  - Add resolve_packageconfig_defaults() helper (+30 lines)

**Expected Impact:** +0.5-1.0% accuracy
**Accuracy Target:** 84-87%
**Test Strategy:**
- Add 8-10 new PACKAGECONFIG test cases
- Test with real recipes: mesa, pulseaudio, dbus

---

### Phase 7h: Variable Expansion Completeness
**Priority:** LOW-MEDIUM | **ROI:** MEDIUM | **Complexity:** LOW

**Objective:** Handle remaining variable expansion patterns

**Current Gap:**
- Basic variables handled: ${PN}, ${PV}, ${BPN}, ${P}, 9 arch/build variables
- Missing: ${WORKDIR}, ${S}, ${D}, ${TMPDIR}, nested expansions

**Improvements:**

1. **Additional Standard Variables** (2-3 hours)
   - ${WORKDIR} → typically "${TMPDIR}/work/${MACHINE}/${PN}/${PV}"
   - ${S} → typically "${WORKDIR}/${BPN}-${PV}"
   - ${B} → build directory
   - ${D} → destination directory
   - Add sensible defaults for dependency extraction context
   
2. **Nested Variable Expansion** (2-3 hours)
   - ${@d.getVar('${VAR}')} - variable inside getVar
   - Currently handled linearly, need recursive expansion
   - Limit depth to 3 to prevent infinite loops

**Files to Modify:**
- `recipe_extractor.rs`:
  - expand_simple_variables() - add new variables (+40 lines)
  - Add recursive expansion support (+35 lines)

**Expected Impact:** +0.3-0.5% accuracy
**Accuracy Target:** 84.3-87.5%
**Test Strategy:**
- Add 5-6 new variable expansion tests
- Test nested expansion patterns

---

## PART 1 SUMMARY: Path to 85%

### Phased Execution Plan

**Week 1 (8-10 hours):**
- Day 1-2: Phase 7f (6-9 hours) - .bbclass Python patterns
- Day 3: Phase 7g (4-6 hours) - PACKAGECONFIG enhancements

**Week 2 (4-8 hours):**
- Day 1: Phase 7h (4-6 hours) - Variable expansion
- Day 2: Integration testing, validation, documentation (2-4 hours)

### Expected Results

| Phase | Accuracy Gain | Cumulative | Effort | Complexity |
|-------|--------------|------------|--------|-----------|
| 7d-e (completed) | +3.7-5.7% | 82-84% | 8-10h | Medium |
| 7f | +1.5-2.0% | 83.5-86% | 8-12h | Medium |
| 7g | +0.5-1.0% | 84-87% | 5-8h | Low-Med |
| 7h | +0.3-0.5% | 84.3-87.5% | 4-6h | Low |
| **TOTAL** | **+5.5-9.2%** | **84.3-87.5%** | **17-26h** | **Medium** |

**Conservative Target:** 85% (39/46 recipes)
**Optimistic Target:** 87% (40/46 recipes)
**Most Likely:** 85-86% (39-40/46 recipes)

### Files Summary - Part 1

**Modified Files:**
1. `class_dependencies.rs` - +70 lines (Python expression handling)
2. `recipe_extractor.rs` - +110 lines (PACKAGECONFIG + variable expansion)

**New Test Files:**
1. `test_class_python.rs` - Test .bbclass Python evaluation
2. `test_packageconfig_advanced.rs` - Advanced PACKAGECONFIG patterns

**Updated Documentation:**
1. Update `phase-7-completion-summary.md` with 7f-7h results
2. Create `85-percent-achievement.md` - Final summary

---

## PART 2: STRATEGIC ROADMAP - 85% → 95%

### Overview
**Goal:** Plan smart path from 85% to 95% accuracy
**Timeline:** 60-80 hours over 2-3 months
**Risk:** Medium-High (increasing complexity)
**Approach:** Target high-value features with acceptable complexity

---

### Phase 8: Advanced Python Expression Handling
**Priority:** HIGH | **ROI:** HIGH | **Effort:** 16-24 hours

**Objective:** Handle more complex Python patterns without full Python execution

**Components:**

#### Phase 8a: String Operations (6-8 hours)
**Patterns to support:**
- `"${VAR}".split(':')[0]` - string indexing
- `"${VAR}".replace('old', 'new')` - replacements  
- `"${VAR}".strip()/.lstrip()/.rstrip()` - trimming
- `len("${VAR}")` - length checks

**Implementation:**
```rust
// Extend SimplePythonEvaluator with string operation support
fn eval_string_operation(&self, expr: &str) -> Option<String> {
    // Parse: value.split(sep)[index]
    // Parse: value.replace(old, new)
    // Parse: value.strip()
}
```

**Impact:** +1.5-2.5% (handles ~15-20 recipes with string ops)
**Files:** `simple_python_eval.rs` (+120 lines)

#### Phase 8b: List Operations (6-8 hours)
**Patterns to support:**
- `['a', 'b', 'c']` - list literals
- `'${VAR}'.split()` - already partially handled
- `' '.join([...])` - list joining
- List comprehensions (simple): `[x for x in list if condition]`

**Implementation:**
```rust
fn eval_list_operation(&self, expr: &str) -> Option<Vec<String>> {
    // Parse list literals
    // Parse simple list comprehensions
    // Handle join operations
}
```

**Impact:** +1.0-1.5% (handles ~10-15 recipes)
**Files:** `simple_python_eval.rs` (+100 lines)

#### Phase 8c: Logical Operators (4-6 hours)
**Patterns to support:**
- `condition1 and condition2` - logical AND
- `condition1 or condition2` - logical OR
- `not condition` - logical NOT
- `value in ['a', 'b', 'c']` - membership in literal lists

**Implementation:**
```rust
fn eval_logical_operation(&self, expr: &str) -> Option<bool> {
    // Parse and/or/not operators
    // Short-circuit evaluation
    // Combine with existing condition evaluation
}
```

**Impact:** +0.5-1.0% (handles ~8-12 recipes)
**Files:** `simple_python_eval.rs` (+80 lines)

**Phase 8 Total:**
- **Effort:** 16-22 hours
- **Impact:** +3.0-5.0%
- **Target Accuracy:** 88-92%
- **Files:** `simple_python_eval.rs` (+300 lines, 25 new tests)

---

### Phase 9: Context-Aware Override Resolution
**Priority:** MEDIUM | **ROI:** MEDIUM | **Effort:** 14-20 hours

**Objective:** Properly resolve overrides based on build context

**Background:**
- Current implementation has BuildContext struct (line 15-37, recipe_extractor.rs)
- Infrastructure exists but not fully utilized
- Inclusive approach chosen for dependency extraction (want ALL possible deps)

**Components:**

#### Phase 9a: Override Precedence Resolution (6-8 hours)
**Implementation:**
- Fully implement override precedence (rightmost wins)
- DEPENDS = "a", DEPENDS:append = " b", DEPENDS:class-native = "c"
- For native build: resolve to "c" (override wins over append)
- For dependency extraction: keep inclusive (want both "a b" and "c")

```rust
// In recipe_extractor.rs
fn resolve_overrides_with_context(&self, variables: &HashMap<String, Vec<(String, String, Vec<String>)>>) 
    -> HashMap<String, String> {
    // Sort by override specificity
    // Apply precedence rules
    // Return resolved values for specific context OR merged for complete view
}
```

**Files:** `override_resolver.rs` (already exists at 441 lines, enhance it)
**Impact:** +1.0-1.5% (more precise for recipes with heavy override use)

#### Phase 9b: Multi-Context Extraction (8-12 hours)
**Implementation:**
- Extract dependencies for multiple contexts: native, target, nativesdk
- Return context-specific dependency sets
- Useful for understanding different build scenarios

```rust
pub struct MultiContextExtraction {
    native_deps: Vec<String>,
    target_deps: Vec<String>,
    nativesdk_deps: Vec<String>,
    common_deps: Vec<String>,  // deps across all contexts
}
```

**Files:** New `multi_context.rs` (200 lines), update `recipe_extractor.rs` (+50 lines)
**Impact:** +0.5-1.0% (better accuracy for cross-compile scenarios)

**Phase 9 Total:**
- **Effort:** 14-20 hours
- **Impact:** +1.5-2.5%
- **Target Accuracy:** 89.5-94.5%
- **Files:** `override_resolver.rs` enhanced, new `multi_context.rs`

---

### Phase 10: Task Dependency Extraction
**Priority:** MEDIUM-LOW | **ROI:** LOW-MEDIUM | **Effort:** 12-16 hours

**Objective:** Extract task-level dependencies (do_build[depends], do_fetch[depends])

**Current State:**
- Task parsing exists (task_parser.rs, 391 lines)
- Extracts task structure but not task[depends] flags
- Task dependencies are ~5-10% of total dependency info

**Components:**

#### Phase 10a: Parse task[depends] Flags (6-8 hours)
**Implementation:**
```rust
// Extend task_parser.rs
pub struct TaskDependencies {
    task_name: String,
    depends: Vec<String>,      // Other tasks in same recipe
    rdepends: Vec<String>,     // Runtime task deps
    recipe_depends: Vec<String>, // Cross-recipe: "recipe:do_task"
}

fn parse_task_depends_flag(line: &str) -> Option<TaskDependencies> {
    // Parse: do_compile[depends] += "virtual/kernel:do_shared_workdir"
    // Extract recipe:task pairs
}
```

**Files:** `task_parser.rs` (+120 lines)
**Impact:** +0.5-1.0%

#### Phase 10b: Task Dependency Graph (6-8 hours)
**Implementation:**
- Build complete task dependency graph
- Include both internal task ordering and cross-recipe task deps
- Useful for parallel build optimization

**Files:** `task_graph.rs` (new, 250 lines), update `recipe_graph.rs` (+80 lines)
**Impact:** +0.3-0.5% (mostly value for build orchestration, less for dep accuracy)

**Phase 10 Total:**
- **Effort:** 12-16 hours
- **Impact:** +0.8-1.5%
- **Target Accuracy:** 90.3-96%
- **Files:** `task_parser.rs` enhanced, new `task_graph.rs`

---

### Phase 11: Advanced BBCLASS Coverage
**Priority:** LOW-MEDIUM | **ROI:** MEDIUM | **Effort:** 10-14 hours

**Objective:** Parse and evaluate more complex .bbclass patterns

**Components:**

#### Phase 11a: Python Function Definitions in .bbclass (6-8 hours)
**Current Gap:**
- Many .bbclass files define Python functions: `python do_configure() { ... }`
- These functions may set variables that affect dependencies
- Currently skipped entirely

**Implementation:**
- Parse Python function blocks in .bbclass
- Extract simple variable assignments: `d.setVar('DEPENDS', '...')`
- Skip complex logic (would require Python execution)

```rust
fn parse_python_function(content: &str) -> HashMap<String, String> {
    // Extract: python function_name() { ... }
    // Parse simple d.setVar() calls
    // Return extracted variables
}
```

**Files:** `class_dependencies.rs` (+100 lines)
**Impact:** +0.5-1.0%

#### Phase 11b: Anonymous Python Functions (4-6 hours)
**Pattern:**
```python
python __anonymous() {
    # Runs at parse time
    if d.getVar('CONDITION'):
        d.appendVar('DEPENDS', ' extra-dep')
}
```

**Implementation:**
- Parse __anonymous functions in .bbclass and .bb files
- Evaluate simple conditional patterns
- Track variable modifications

**Files:** `class_dependencies.rs` (+80 lines), `recipe_extractor.rs` (+40 lines)
**Impact:** +0.3-0.8%

**Phase 11 Total:**
- **Effort:** 10-14 hours
- **Impact:** +0.8-1.8%
- **Target Accuracy:** 91.1-97.8%
- **Files:** `class_dependencies.rs` enhanced

---

### Phase 12: Edge Cases & Polish
**Priority:** LOW | **ROI:** LOW | **Effort:** 8-12 hours

**Objective:** Handle remaining edge cases and corner scenarios

**Components:**

1. **Dynamic Variable Names** (3-4 hours)
   - DEPENDS_${PN} = "..."
   - Need to expand ${PN} in variable names before lookup
   
2. **Nested Python Expressions** (3-4 hours)
   - ${@bb.utils.contains('X', 'y', d.getVar('A'), d.getVar('B'), d)}
   - Nested function calls and variable references
   
3. **Multiline Strings** (2-3 hours)
   - DEPENDS = "dep1 \
              dep2 \
              dep3"
   - Line continuation in variable values
   - Likely already handled but verify

4. **Variable Flags** (2-3 hours)
   - DEPENDS[flag] = "value"
   - DEPENDS:append[flag] = "value"
   - Not commonly used for dependencies but exists

**Files:** Various (`recipe_extractor.rs`, `simple_python_eval.rs`)
**Impact:** +0.2-0.5%

**Phase 12 Total:**
- **Effort:** 10-14 hours
- **Impact:** +0.2-0.5%
- **Target Accuracy:** 91.3-98.3%

---

## PART 2 SUMMARY: Path to 95%

### Phased Execution Timeline

**Month 1 (20-30 hours):**
- Week 1-2: Phase 8 (Advanced Python) - 16-22 hours
- Week 3: Phase 9a (Override Resolution) - 6-8 hours

**Month 2 (20-28 hours):**
- Week 1-2: Phase 9b (Multi-Context) - 8-12 hours
- Week 3: Phase 10 (Task Dependencies) - 12-16 hours

**Month 3 (18-26 hours):**
- Week 1-2: Phase 11 (Advanced .bbclass) - 10-14 hours
- Week 3: Phase 12 (Edge Cases) - 8-12 hours

### Cumulative Progress

| Phase | Accuracy Gain | Cumulative | Effort | ROI | Dependencies |
|-------|--------------|------------|--------|-----|-------------|
| **COMPLETED** | | **82-84%** | **~30h** | | |
| 7f-7h (Part 1) | +2.3-3.5% | 84.3-87.5% | 17-26h | High | None |
| **8: Python** | **+3.0-5.0%** | **87.3-92.5%** | **16-22h** | **High** | None |
| **9: Overrides** | **+1.5-2.5%** | **88.8-95%** | **14-20h** | **Medium** | None |
| 10: Tasks | +0.8-1.5% | 89.6-96.5% | 12-16h | Low-Med | None |
| 11: .bbclass | +0.8-1.8% | 90.4-98.3% | 10-14h | Medium | Phase 8 |
| 12: Polish | +0.2-0.5% | 90.6-98.8% | 8-12h | Low | All |
| **TOTAL** | **+8.6-14.8%** | **90.6-98.8%** | **77-110h** | | |

**Realistic 95% Path:** Complete Phases 7f-7h, 8, 9, 10
- **Total Effort:** 59-84 hours
- **Timeline:** 7-11 weeks at 8-10 hours/week
- **Expected Accuracy:** 92-96%

---

## EFFORT VS. ACCURACY ROI ANALYSIS

### High ROI Phases (Recommended)

1. **Phase 7f - .bbclass Python** → 1.5-2.0% for 8-12h ⭐⭐⭐⭐⭐
   - Best immediate return
   - Low risk, proven patterns
   
2. **Phase 8 - Advanced Python** → 3.0-5.0% for 16-22h ⭐⭐⭐⭐⭐
   - Highest single-phase impact
   - Enables many other improvements
   
3. **Phase 7g - PACKAGECONFIG** → 0.5-1.0% for 5-8h ⭐⭐⭐⭐
   - Quick wins
   - Common pattern

### Medium ROI Phases (Consider)

4. **Phase 9 - Overrides** → 1.5-2.5% for 14-20h ⭐⭐⭐
   - Important for correctness
   - Moderate complexity
   
5. **Phase 11 - .bbclass Advanced** → 0.8-1.8% for 10-14h ⭐⭐⭐
   - Good return
   - Requires Phase 8

### Lower ROI Phases (Optional)

6. **Phase 7h - Variables** → 0.3-0.5% for 4-6h ⭐⭐
   - Easy but limited impact
   
7. **Phase 10 - Tasks** → 0.8-1.5% for 12-16h ⭐⭐
   - More useful for build orchestration than accuracy
   
8. **Phase 12 - Polish** → 0.2-0.5% for 8-12h ⭐
   - Diminishing returns
   - Only for completeness

---

## RECOMMENDED EXECUTION STRATEGY

### Strategy A: Fast 85% (Minimum Viable)
**Goal:** Reach 85% quickly with proven approaches
**Timeline:** 2-3 weeks (17-26 hours)
**Phases:** 7f, 7g, 7h
**Result:** 84.3-87.5% accuracy
**Risk:** Low
**Recommendation:** ✅ **HIGHLY RECOMMENDED** for immediate production use

### Strategy B: Solid 90% (Balanced)
**Goal:** Strong accuracy with acceptable effort
**Timeline:** 6-8 weeks (47-68 hours)
**Phases:** 7f, 7g, 7h, 8, 9a
**Result:** 89-93% accuracy
**Risk:** Low-Medium
**Recommendation:** ✅ **RECOMMENDED** for serious production deployment

### Strategy C: Ambitious 95% (Maximum)
**Goal:** Near-complete coverage short of full Python execution
**Timeline:** 10-14 weeks (77-110 hours)
**Phases:** 7f-7h, 8, 9, 10, 11
**Result:** 92-96% accuracy
**Risk:** Medium
**Recommendation:** ⚠️ **CONSIDER** only if 90% insufficient for use case

### Strategy D: True 99% (Research)
**Goal:** Full BitBake parity
**Timeline:** 6+ months (150-250 hours)
**Phases:** All phases + Python execution (pyo3 integration)
**Result:** 98-99.5% accuracy
**Risk:** High
**Recommendation:** ❌ **NOT RECOMMENDED** - use BitBake directly at this point

---

## RISK ASSESSMENT

### Technical Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| Phase complexity underestimated | Medium | Medium | Add 20% buffer to estimates |
| Test coverage insufficient | Low | High | Require 95%+ test coverage per phase |
| Performance regression | Low | Medium | Benchmark after each phase |
| Breaking changes to API | Low | High | Semantic versioning, deprecation warnings |
| Python evaluation edge cases | Medium | Low | Comprehensive fallback strategy |

### Maintainability Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| Code complexity growth | High | High | Regular refactoring, code review |
| Test maintenance burden | Medium | Medium | Focus on integration tests |
| Documentation drift | Medium | Medium | Update docs per phase |
| Technical debt accumulation | Medium | High | Allocate 15% time to refactoring |

---

## SUCCESS METRICS

### Accuracy Metrics
- **Primary:** DEPENDS extraction accuracy (recipes with correct DEPENDS)
- **Secondary:** RDEPENDS extraction accuracy
- **Tertiary:** PROVIDES extraction accuracy

### Quality Metrics
- Test coverage: >95% for new code
- All existing tests pass (118+ tests)
- No performance regression >10%
- Documentation completeness: 100%

### Performance Metrics (maintain or improve)
- Per-recipe parsing: <250ms (current: 217ms)
- 920 recipes: <3 minutes (current: ~2 min estimated)
- Memory usage: <500MB for full parse

---

## CONCLUSION

### Summary

The codebase is in an excellent state at 82-84% projected accuracy after Phase 7d-e. The path to 85% is clear with low-risk, high-ROI improvements in Phases 7f-7h.

**Recommended Next Steps:**

1. **Immediate (Weeks 1-3):** Execute **Strategy A** → 85%
   - Phase 7f: .bbclass Python patterns
   - Phase 7g: PACKAGECONFIG enhancements  
   - Phase 7h: Variable expansion
   - **Result:** Production-ready 85%+ accuracy

2. **Near-term (Months 2-3):** Execute **Strategy B** → 90%
   - Phase 8: Advanced Python expression handling
   - Phase 9a: Override precedence resolution
   - **Result:** Robust 90%+ accuracy for serious deployment

3. **Long-term (Months 4-6):** Evaluate **Strategy C** → 95%
   - Only if 90% proves insufficient
   - Phases 10-11 for final accuracy gains
   - **Result:** Near-complete 95%+ accuracy

4. **Not Recommended:** Strategy D (99%+)
   - Would require full Python execution (pyo3)
   - Estimated 150-250 additional hours
   - Use BitBake directly if true 99% needed

### Key Insights

1. **Phase 8 (Advanced Python) is the highest value strategic phase** - unlocks 3-5% gain
2. **Phases 7f-7h are the best immediate ROI** - proven patterns, low risk
3. **95% is achievable without Python execution** - but requires 77-110 hours
4. **Beyond 95% requires fundamental architecture change** - embed Python interpreter

### Final Recommendation

**Execute Strategy A immediately** (85% target) for quick production value, then **evaluate Strategy B** (90% target) based on real-world usage patterns and accuracy requirements.

The current implementation is already production-ready. These phases provide incremental value for demanding use cases.

---

**Document Version:** 1.0
**Last Updated:** 2025-11-12
**Total Estimated Effort:** 77-110 hours for 95% accuracy
**Current Status:** 82-84% (projected), 118 tests passing
