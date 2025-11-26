# Hitzeleiter TODO - Path to State-of-the-Art Build System

**Goal:** `hitzeleiter kas config.yml && hitzeleiter build busybox` produces working aarch64 binary

**Vision:** Become the best Bazel and BitBake inspired build engine by late 2025

---

## Current Status Assessment (November 2025)

### Critical Issues Identified
| Issue | Severity | Status |
|-------|----------|--------|
| Query system SEGFAULTS | Critical | **TODO** |
| Tests don't compile (`bitzel` crate) | Critical | **TODO** |
| 50+ unwrap() in production code | High | **TODO** |
| No dry-run support for builds | High | **TODO** |
| Core pipeline incomplete | High | **TODO** |

### Gap to State-of-the-Art
| Feature | Buck2/Bazel | This Project | Gap |
|---------|-------------|--------------|-----|
| Correctness | Hermetic, reproducible | SEGFAULTS | Critical |
| Test Coverage | Property-based | Tests don't compile | Critical |
| Incremental | Adapton-style DCG | Content hash only | Missing |
| Remote Execution | REAPI v2 | HTTP only | Incomplete |
| Dynamic Dependencies | Monadic (Shake) | Static only | Missing |
| Error Messages | Rich TUI | Crashes | Critical |

---

## Phase 0: Stabilization (IMMEDIATE)

### 0.1 Fix SEGFAULT in Query Command
- [ ] Investigate crash at ~7500/15923 tasks during task spec generation
- [ ] Add bounds checking and error handling
- [ ] Test with: `hitzeleiter query -b build-test "deps(busybox, 2)"`

### 0.2 Fix Test Compilation
- [ ] Remove/fix `bitzel` references in `hitzeleiter/examples/simple_sandbox.rs`
- [ ] Ensure `cargo test` compiles successfully
- [ ] All tests should pass

### 0.3 Remove Critical unwrap() Calls
- [ ] `convenient-git/src/lib.rs:173` - panic! in constructor
- [ ] `convenient-git/src/lib.rs:48-50` - unwraps on author info
- [ ] `convenient-git/src/lib.rs:260,287,305-306,346,356` - git operations
- [ ] `convenient-bitbake/src/executor/executor.rs:433` - chained unwraps
- [ ] `convenient-bitbake/src/build_orchestrator.rs:392` - thread pool

### 0.4 Query Command Works End-to-End
- [ ] `hitzeleiter query -b build-test "deps(busybox, 2)"` returns results
- [ ] `hitzeleiter query -b build-test "rdeps(glibc, 1)"` returns results
- [ ] No crashes, proper error messages

---

## Phase 1: Complete the Pipeline

### 1.1 Unpack (Wire to Executor)
- [ ] Connect `fetcher.rs:unpack_source()` to `do_unpack` task in executor
- [ ] Handle `S = "${WORKDIR}/busybox-${PV}"` path resolution
- [ ] Support tar.gz, tar.bz2, tar.xz, zip formats
- [ ] Test: Tarball extracted to correct ${S}

### 1.2 Patch Application
- [ ] Implement `patch -p1` for .patch files in SRC_URI
- [ ] Parse SRC_URI for file:// patches
- [ ] Apply patches in order specified
- [ ] Test: Busybox patches applied correctly

### 1.3 Toolchain Setup
- [ ] MACHINE → toolchain mapping (qemuarm64 → aarch64-linux-gnu)
- [ ] Set CC, CXX, LD, AR, STRIP, OBJCOPY
- [ ] Set CFLAGS, CXXFLAGS, LDFLAGS for cross-compilation
- [ ] Detect host toolchain location
- [ ] Test: Cross-compile hello.c for aarch64

### 1.4 Sysroot Assembly
- [ ] Wire existing `sysroot.rs` to build pipeline
- [ ] Assemble recipe-sysroot from DEPENDS
- [ ] Hardlink outputs from dependencies
- [ ] Test: Headers and libraries available from dependencies

### 1.5 End-to-End Build Test
- [ ] `hitzeleiter build -b build-test busybox` completes
- [ ] Binary produced in expected location
- [ ] `file busybox` shows ARM aarch64 executable
- [ ] Binary runs in qemu-aarch64

---

## Phase 2: Architectural Upgrades (Future)

### 2.1 Suspending Scheduler
- [ ] Implement monadic/dynamic dependencies (à la Shake)
- [ ] Support dependencies discovered at runtime
- [ ] Reference: "Build Systems à la Carte" paper

### 2.2 Adapton-Style Incremental Computation
- [ ] Implement Demanded Computation Graph (DCG)
- [ ] First-class names for cached computations
- [ ] Demand-driven change propagation
- [ ] Only re-execute affected computations

### 2.3 Full REAPI v2 Support
- [ ] Complete gRPC implementation (tonic)
- [ ] Action Cache integration
- [ ] Content Addressable Store integration
- [ ] Remote execution worker support

### 2.4 File System Watching
- [ ] Integrate `notify` crate for file watching
- [ ] Incremental re-parsing on file changes
- [ ] Daemon mode for persistent analysis

---

## Phase 3: Developer Experience (Future)

### 3.1 Rich TUI
- [ ] Progress visualization (ratatui)
- [ ] Task execution status
- [ ] Resource utilization display
- [ ] Error highlighting

### 3.2 Actionable Error Messages
- [ ] Context-rich error reporting
- [ ] Suggestions for common issues
- [ ] Links to documentation

### 3.3 Query Language (BXL-style)
- [ ] Starlark-based query language
- [ ] Code generation from build graph
- [ ] Custom analysis scripts

---

## Research Papers & References

### Foundational Theory

#### "Build Systems à la Carte" (Mokhov, Mitchell, Peyton Jones)
- **Publication**: ICFP 2018, JFP 2020
- **URL**: https://dl.acm.org/doi/10.1145/3236774
- **Key Insight**: Build systems decompose into scheduler × rebuilder
- **Relevance**: Our scheduler is topological (Make-like), should be suspending (Shake-like)
- **Action**: Implement monadic dependencies for dynamic dep support

#### "Adapton: Composable, Demand-Driven Incremental Computation"
- **Publication**: PLDI 2014
- **Authors**: Hammer, Phang, Hicks, Foster
- **URL**: https://dl.acm.org/doi/abs/10.1145/2666356.2594324
- **Key Insight**: Demanded Computation Graph with first-class names
- **Relevance**: Our signature computation recomputes everything
- **Action**: Implement DCG for fine-grained invalidation

#### "Shake Before Building: Replacing Make with Haskell"
- **Publication**: ICFP 2012
- **Author**: Neil Mitchell
- **Key Insight**: Monadic dependencies as first-class feature
- **Relevance**: Static dependency graphs limit expressiveness
- **Action**: Study Shake's need/want primitives

### Applied Research

#### "Escaping Dependency Hell" (ISSTA 2020)
- **Authors**: Gang Fan et al.
- **URL**: https://dl.acm.org/doi/10.1145/3395363.3397388
- **Key Insight**: Unified dependency graph detects missing/wrong dependencies
- **Relevance**: Our dependency resolution may miss implicit deps
- **Action**: Implement dependency verification

#### "Enabling Fine-Grained Incremental Builds" (CGO 2024)
- **Key Insight**: Stateful compilation for within-file incrementality
- **Relevance**: Recipe-level granularity may be too coarse
- **Action**: Consider finer-grained tracking

#### "BuildSheriff: Change-Aware Test Failure Triage" (ICSE 2022)
- **Key Insight**: Cluster test failures by root cause
- **Relevance**: Better failure diagnosis
- **Action**: Implement failure clustering

### Industry References

#### Buck2 Architecture (Meta)
- **URL**: https://buck2.build/docs/
- **Key Features**: DICE computation model, Starlark rules, RE API
- **Relevance**: Modern Rust-based build system
- **Action**: Study DICE for incremental computation

#### Bazel Remote Execution API
- **URL**: https://github.com/bazelbuild/remote-apis
- **Key Features**: gRPC protocol, Action Cache, CAS
- **Relevance**: Industry standard for remote execution
- **Action**: Complete REAPI v2 implementation

#### Pants Build System
- **URL**: https://www.pantsbuild.org/
- **Key Features**: Automatic dependency inference, fine-grained invalidation
- **Relevance**: Python-friendly, good DX
- **Action**: Study dependency inference approach

### Reproducible Builds

#### SOURCE_DATE_EPOCH Specification
- **URL**: https://reproducible-builds.org/specs/source-date-epoch/
- **Key Insight**: Standardized timestamp for reproducibility
- **Action**: Implement SOURCE_DATE_EPOCH support

#### Nix/NixOS Papers
- **URL**: https://nixos.org/guides/nix-pills/
- **Key Insight**: Purely functional package management
- **Relevance**: Content-addressed, reproducible builds
- **Action**: Study derivation model

---

## Key Files Reference

| Component | File | Status |
|-----------|------|--------|
| **Rust fetcher** | `convenient-bitbake/src/executor/rust_fetcher.rs` | Done |
| **Fetch task** | `convenient-bitbake/src/executor/fetch_task.rs` | Done |
| **Task executor** | `convenient-bitbake/src/executor/executor.rs` | Needs fixes |
| **Unpack** | `convenient-bitbake/src/fetcher.rs:111` | Not wired |
| **Patch** | None | Not implemented |
| **Sysroot** | `convenient-bitbake/src/sysroot.rs` | Not wired |
| **Build cmd** | `hitzeleiter/src/commands/build.rs` | Needs rewrite |
| **Query cmd** | `hitzeleiter/src/commands/query.rs` | SEGFAULTS |
| **Signature cache** | `convenient-bitbake/src/signature_cache.rs` | Works |
| **Build orchestrator** | `convenient-bitbake/src/build_orchestrator.rs` | Core logic |

---

## Success Criteria

### Phase 0 Complete When:
```bash
cargo test                                           # All tests pass
cargo clippy --all-targets                          # No errors
hitzeleiter query -b build-test "deps(busybox, 2)"  # Returns results, no crash
```

### Phase 1 Complete When:
```bash
hitzeleiter build -b build-test busybox             # Completes successfully
file build-test/tmp/work/.../busybox               # ELF 64-bit LSB executable, ARM aarch64
qemu-aarch64 ./busybox --help                       # Shows busybox help
```

---

## Progress Tracking

- [x] Recipe parsing
- [x] Task graph building
- [x] KAS integration (setup only)
- [x] Caching infrastructure
- [x] Sandbox infrastructure
- [x] Fetch - Pure Rust implementation
- [x] Fetch wiring - Connected to executor
- [ ] **Phase 0: Stabilization** ← CURRENT
- [ ] Phase 1: Complete Pipeline
- [ ] Phase 2: Architectural Upgrades
- [ ] Phase 3: Developer Experience
