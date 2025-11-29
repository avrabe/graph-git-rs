# Hitzeleiter Capability Matrix

**Last Updated**: November 2025
**Status**: Pre-release

This document provides an honest assessment of what Hitzeleiter can actually do versus what is planned or theoretical.

## Legend

- ✅ **PROVEN**: Tested and working in practice
- ⚠️ **IMPLEMENTED**: Code exists but not fully validated end-to-end
- 🚧 **PARTIAL**: Partially implemented, significant gaps remain
- ❌ **MISSING**: Not yet implemented
- 🔬 **THEORETICAL**: Planned but no implementation started

---

## Core Capabilities

### Recipe Parsing

| Feature | Status | Evidence | Notes |
|---------|--------|----------|-------|
| Parse .bb files | ✅ PROVEN | Tests pass, used in kas command | Full BitBake syntax support |
| Parse .bbappend files | ✅ PROVEN | Tests pass | Overlay support working |
| Parse .inc files | ✅ PROVEN | Tests pass | Include resolution works |
| Parse .conf files | ✅ PROVEN | Tests pass | layer.conf, local.conf, etc. |
| Variable expansion | ✅ PROVEN | Tests pass | ${VAR} expansion works |
| Override resolution | ✅ PROVEN | Tests pass | MACHINE/DISTRO overrides work |
| Python expressions ${@...} | ✅ PROVEN | Tests pass, expanded at task spec creation | Simple expressions only |
| Complex Python functions | 🚧 PARTIAL | SimplePythonEvaluator limited | No full Python VM |
| Anonymous Python | ⚠️ IMPLEMENTED | Code exists | Not fully tested |

### Dependency Graph

| Feature | Status | Evidence | Notes |
|---------|--------|----------|-------|
| Recipe dependency graph | ✅ PROVEN | Tests pass, works in build command | DEPENDS, RDEPENDS resolution |
| Task dependency graph | ✅ PROVEN | Tests pass | Intra and inter-recipe deps |
| Provider resolution | ✅ PROVEN | Tests pass | PROVIDES handling works |
| PREFERRED_PROVIDER | ⚠️ IMPLEMENTED | Code exists | Not extensively tested |
| Task filtering (build_for_task) | ✅ PROVEN | Used in build.rs:161 | Prunes to specific target |
| Topological sorting | ✅ PROVEN | execution_order computed | Dependencies-first ordering |
| Cycle detection | ⚠️ IMPLEMENTED | Code exists | Not extensively tested |

### KAS Integration

| Feature | Status | Evidence | Notes |
|---------|--------|----------|-------|
| Parse KAS YAML | ✅ PROVEN | Tests pass, kas command works | Single and multi-file configs |
| Include graph resolution | ✅ PROVEN | KasIncludeGraph working | Handles nested includes |
| Repository fetching | ✅ PROVEN | Pure Rust implementation | Git repos work |
| Config generation | ✅ PROVEN | Generates local.conf, bblayers.conf | Template-based |
| Layer priority handling | ✅ PROVEN | BuildContext respects priorities | Correct override order |
| **Build execution** | ❌ MISSING | Stops after setup | kas command doesn't build |

### Task Execution

| Feature | Status | Evidence | Notes |
|---------|--------|----------|-------|
| do_fetch (HTTP) | ✅ PROVEN | Pure Rust fetcher | Mirror fallback supported |
| do_fetch (Git) | ✅ PROVEN | Pure Rust git2 | Protocol conversion works |
| do_unpack | ✅ PROVEN | Pure Rust implementation | tar.gz, .bz2, .xz supported |
| do_patch | ✅ PROVEN | Patch application working | Git apply and patch -p1 |
| do_configure | ⚠️ IMPLEMENTED | Shell executor exists | Not validated end-to-end |
| do_compile | ⚠️ IMPLEMENTED | Shell executor exists | Not validated end-to-end |
| do_install | ⚠️ IMPLEMENTED | Shell executor exists | Not validated end-to-end |
| do_package | ⚠️ IMPLEMENTED | Package ops exist | Not validated end-to-end |
| do_populate_sysroot | ⚠️ IMPLEMENTED | Sysroot assembler exists | Tests currently failing |
| Custom tasks | ⚠️ IMPLEMENTED | Shell executor can run any task | Not extensively tested |

### Execution Modes

| Mode | Status | Evidence | Notes |
|------|--------|----------|-------|
| DirectRust | ⚠️ IMPLEMENTED | Code exists, tests failing | For simple operations |
| RustShell (brush) | ⚠️ IMPLEMENTED | Integrated, untested | In-process bash |
| Sandboxed (namespaces) | ⚠️ IMPLEMENTED | Code exists, tests failing | Linux namespaces |
| External executor | ⚠️ IMPLEMENTED | Framework exists | Not tested |
| WASM executor | 🚧 PARTIAL | Stub exists | Minimal implementation |

### Caching

| Feature | Status | Evidence | Notes |
|---------|--------|----------|-------|
| Content-addressable store (CAS) | ✅ PROVEN | Used by executor | SHA256-based |
| Action cache | ✅ PROVEN | Used by executor | Task result caching |
| Signature computation | ✅ PROVEN | SignatureCache working | Input-based hashing |
| Incremental builds | ⚠️ IMPLEMENTED | Stats computed | Not validated end-to-end |
| Remote cache | 🔬 THEORETICAL | Planned | REAPI v2 support planned |
| Cache eviction | ❌ MISSING | No LRU/size limits | Cache grows unbounded |

### Sandboxing

| Feature | Status | Evidence | Notes |
|---------|--------|----------|-------|
| Linux namespaces | ⚠️ IMPLEMENTED | Code exists, tests failing | mount, pid, uts, ipc |
| Bind mounts | ⚠️ IMPLEMENTED | Code exists | For sources, sysroot |
| Network policies | ⚠️ IMPLEMENTED | Enum exists | Not enforced yet |
| Resource limits | ⚠️ IMPLEMENTED | Struct exists | Not enforced yet |
| Prelude script | ✅ PROVEN | Generated and mounted | BitBake env setup |

### Query System

| Feature | Status | Evidence | Notes |
|---------|--------|----------|-------|
| deps() function | ✅ PROVEN | Logos lexer, parser working | Forward dependencies |
| rdeps() function | ✅ PROVEN | Parser working | Reverse dependencies |
| somepath() function | ✅ PROVEN | Parser working | Shortest path query |
| allpaths() function | ✅ PROVEN | Parser working | All paths query |
| kind() filter | ✅ PROVEN | Parser working | Filter by node type |
| Set operations | ✅ PROVEN | intersect, union, except | Combinators work |
| Task-specific queries | ⚠️ IMPLEMENTED | Parser support | Not extensively tested |

---

## End-to-End Validation Status

### ❌ **CRITICAL GAP**: No Proven End-to-End Builds

**Status**: Phase 1.5 in roadmap is marked incomplete.

**What this means**:
- The system has **never successfully built a complete binary** from recipe to executable
- Individual components work in isolation
- The full compile → install → package → image pipeline is **UNPROVEN**

**Blocker for production use**: YES

**Test case needed**:
```bash
# This should work but hasn't been validated:
hitzeleiter kas test-fixtures/kas-configs/busybox-qemuarm64.yml
hitzeleiter build -b build busybox
file build/tmp/work/aarch64-poky-linux/busybox/1.36.1/image/bin/busybox
# Expected: ELF 64-bit LSB executable, ARM aarch64
```

### Test Suite Status

- **Total tests**: 412
- **Passing**: 395 (95.9%)
- **Failing**: 17 (4.1%)
- **Ignored**: 3

**Failing test categories**:
- Executor tests (direct execution, sandboxing): 11 failures
- Sysroot tests: 3 failures
- Config tests: 1 failure
- Other: 2 failures

**Impact**: Tests are failing due to assertion errors, suggesting recent code changes broke test assumptions. These need investigation and fixes before claiming production-ready status.

---

## Code Quality Metrics

| Metric | Count | Risk Level | Notes |
|--------|-------|------------|-------|
| unwrap() calls | 462 | 🟡 MEDIUM | Potential panic sites |
| expect() calls | (included in unwrap count) | 🟡 MEDIUM | Better than unwrap but still can panic |
| TODO comments | 19 | 🟢 LOW | Reasonable for pre-release |
| FIXME comments | (included in TODO) | 🟢 LOW | |
| Total dependencies | 1,229 | 🟡 MEDIUM | Heavy dependency tree |
| Rust source files | 99 | 🟢 LOW | Well-organized |

---

## Production Readiness Assessment

### ✅ **Safe for Production**

- Recipe parsing and analysis
- Dependency graph generation
- Query language for exploration
- KAS configuration parsing
- Repository fetching

**Use case**: Static analysis, dependency exploration, build planning

### ⚠️ **Use with Caution**

- Simple fetch/unpack/patch workflows
- Incremental builds with caching
- Dry-run mode for build planning

**Use case**: Development, testing, experimentation

### ❌ **NOT Ready for Production**

- Full BitBake builds (compile → package → image)
- Complex recipes (kernel, toolchain, glibc)
- Multi-machine builds
- Production embedded Linux systems

**Why**: Unproven end-to-end, failing tests, potential panics

---

## Comparison: BitBake vs Hitzeleiter

| Feature | BitBake | Hitzeleiter | Gap |
|---------|---------|-------------|-----|
| Recipe parsing | ✅ Complete | ✅ Complete | None |
| Task execution | ✅ Proven | ⚠️ Unproven | **CRITICAL** |
| Caching | Basic | Advanced (CAS) | Hitzeleiter better |
| Parallel execution | Limited | Good (Rayon) | Hitzeleiter better |
| Query language | Basic | Advanced (Bazel-style) | Hitzeleiter better |
| Production use | ✅ Widespread | ❌ None | **CRITICAL** |
| Python support | ✅ Full | 🚧 Limited | **MAJOR** |
| Maturity | 15+ years | < 1 year | **MAJOR** |

---

## Recommended Use Cases

### ✅ **Recommended Now**

1. **Build analysis**: Understand recipe dependencies
2. **Query exploration**: Use query language to explore dependency graphs
3. **KAS setup**: Use for repository fetching and config generation
4. **Research**: Study Bazel-style build system design
5. **Development**: Contribute to implementation

### ⚠️ **Experimental Use**

1. **Simple recipes**: Try building trivial recipes (hello-world)
2. **Fetch/unpack testing**: Validate source fetching workflows
3. **Cache testing**: Experiment with incremental builds
4. **Dry-run planning**: Analyze what would be built

### ❌ **Not Recommended**

1. **Production builds**: Don't use for embedded Linux products
2. **Complex recipes**: Kernel, toolchain builds likely to fail
3. **CI/CD integration**: Too unstable for automated builds
4. **Multi-machine**: Not validated
5. **Critical infrastructure**: Stick with BitBake for now

---

## Path to Production

To become production-ready, Hitzeleiter needs:

1. **CRITICAL**: Complete Phase 1.5 end-to-end validation
   - Prove busybox builds from scratch to working binary
   - Validate on qemu-aarch64

2. **HIGH**: Fix all 17 failing tests
   - Executor tests
   - Sysroot tests
   - Ensure regression test suite passes

3. **HIGH**: Address unwrap() calls in hot paths
   - Executor: ~100 calls
   - Parser: ~50 calls
   - Convert to proper Result<> error handling

4. **MEDIUM**: Expand test coverage
   - End-to-end integration tests
   - Complex recipe tests (glibc, kernel)
   - Multi-machine validation

5. **MEDIUM**: Performance validation
   - Benchmark against BitBake
   - Optimize hot paths
   - Validate caching effectiveness

6. **LOW**: Documentation
   - User guide
   - Migration guide from BitBake
   - Troubleshooting guide

---

## Honest Assessment

**What Hitzeleiter is today**:
- Impressive technical foundation
- Modern Rust architecture
- Advanced caching and query capabilities
- Well-organized codebase

**What Hitzeleiter is NOT**:
- Production-ready build system
- BitBake replacement (yet)
- Proven technology
- Safe for critical builds

**Timeline to production**:
- **Optimistic**: 3-6 months (if Phase 1.5 completes quickly)
- **Realistic**: 6-12 months (accounting for bugs, edge cases)
- **Conservative**: 12-18 months (for full BitBake parity)

**Biggest risks**:
1. Python support limitations (no full VM)
2. Untested complex recipe builds
3. Unknown edge cases in BitBake semantics
4. Maintenance burden (complex codebase)

---

## Conclusion

Hitzeleiter shows great promise as a modern build system, but it's not ready to replace BitBake in production environments. The core infrastructure is solid, but the critical end-to-end validation is missing.

**Recommendation**: Use for research, development, and experimentation. Continue with Phase 1.5 validation before considering production use.

**Next milestone**: Successfully build and run busybox on qemu-aarch64 from a KAS configuration. Once this works, expand to progressively more complex recipes.
