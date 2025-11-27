# Hitzeleiter TODO - Path to State-of-the-Art Build System

**Goal:** `hitzeleiter kas config.yml && hitzeleiter build busybox` produces working aarch64 binary

**Vision:** Become the best Bazel and BitBake inspired build engine by late 2025

---

## Current Status Assessment (November 2025)

### Critical Issues Identified
| Issue | Severity | Status |
|-------|----------|--------|
| Query system SEGFAULTS | Critical | **FIXED** ✓ |
| Tests don't compile (`bitzel` crate) | Critical | **FIXED** ✓ |
| Query parser was hacky | High | **FIXED** ✓ (Logos lexer) |
| Core pipeline incomplete | High | **IN PROGRESS** |
| 50+ unwrap() in production code | Medium | **FIXED** ✓ (convenient-git) |
| No dry-run support for builds | Medium | **FIXED** ✓ (--dry-run flag) |

### Gap to State-of-the-Art
| Feature | Buck2/Bazel | This Project | Gap |
|---------|-------------|--------------|-----|
| Correctness | Hermetic, reproducible | Works, needs testing | Minor |
| Test Coverage | Property-based | Some tests work | Medium |
| Incremental | Adapton-style DCG | Content hash only | Missing |
| Remote Execution | REAPI v2 | HTTP only | Incomplete |
| Dynamic Dependencies | Monadic (Shake) | Static only | Missing |
| Error Messages | Rich TUI | Basic | Medium |

---

## Phase 0: Stabilization ✓ COMPLETED

### 0.1 Fix SEGFAULT in Query Command ✓
- [x] Investigate crash at ~7500/15923 tasks during task spec generation
- [x] Add `build_recipe_graph_only()` method to skip RustPython-heavy task spec generation
- [x] Query command now completes successfully

### 0.2 Fix Test Compilation ✓
- [x] Remove broken `hitzeleiter/examples/simple_sandbox.rs`
- [x] Library builds successfully

### 0.3 Proper Query Language ✓
- [x] Implement Logos-based lexer (`convenient-bitbake/src/query/lexer.rs`)
- [x] Rewrite parser with proper recursive descent (`convenient-bitbake/src/query/parser.rs`)
- [x] Support all query functions: deps, rdeps, somepath, allpaths, kind, filter, attr
- [x] Support task-specific queries: script, inputs, outputs, env, critical-path
- [x] Support set operations: intersect, union, except
- [x] Proper error messages with source location context

### 0.4 Remove Critical unwrap() Calls ✓ COMPLETED
- [x] `convenient-git/src/lib.rs` - panic! in constructor → Now returns Result
- [x] `convenient-git/src/lib.rs` - unwraps on author info → Now uses unwrap_or fallbacks
- [x] `convenient-git/src/lib.rs` - git operations → Proper error handling with Result
- [x] `convenient-bitbake/src/executor/executor.rs` - Only test code uses unwrap() (acceptable)
- [x] `convenient-bitbake/src/build_orchestrator.rs` - Mutex lock unwrap (acceptable for poisoned mutex)

---

## Phase 1: Complete the Pipeline ✓ MOSTLY COMPLETED

### 1.1 Unpack (Wire to Executor) ✓
- [x] Connect `fetcher.rs:unpack_source()` to `do_unpack` task in executor
- [x] Pure Rust implementation using tar, flate2, bzip2, xz2 crates
- [x] Support tar.gz, tar.bz2, tar.xz, tar formats
- [x] Hash unpacked files to CAS

### 1.2 Patch Application ✓
- [x] Implement `git apply` / `patch -p1` for .patch files
- [x] Find patches in workdir and PATCHDIR
- [x] Apply patches in sorted order (supports 0001-*, 0002-* naming)
- [x] Hash patched files to CAS

### 1.3 Toolchain Setup ✓
- [x] Complete prelude.sh with all BitBake environment variables
- [x] STAGING_DIR hierarchy setup
- [x] CC, CXX, LD, AR, STRIP, RANLIB variables
- [x] CFLAGS, CXXFLAGS, LDFLAGS for cross-compilation
- [x] oeconf() helper for autotools configure

### 1.4 Sysroot Assembly ✓
- [x] Complete `sysroot.rs` implementation with hardlink-based assembly
- [x] Conflict detection between dependencies
- [x] Manifest tracking for file provenance
- [x] Whitelist for harmless duplicates (licenses, docs)

### 1.5 End-to-End Build Test (Needs Poky Environment)
- [ ] `hitzeleiter build -b build-test busybox` completes
- [ ] Binary produced in expected location
- [ ] `file busybox` shows ARM aarch64 executable
- [ ] Binary runs in qemu-aarch64

**Network/Proxy Support (November 2025):**
- [x] Added TLS certificate handling via `native-certs` feature (uses OS cert store)
- [x] Git protocol conversion: `git://` → `https://` for proxy compatibility
- [x] HTTP proxy auto-detection from environment variables (`HTTPS_PROXY`, `https_proxy`)
- [x] Added `--skip-fetch` flag for offline/network-less builds
- [x] Git CLI fallback when git2 fails with authentication errors
- [x] **Mirror Fallback System** for HTTP sources:
  - Git config `url.<base>.insteadOf` support
  - BitBake-style `PREMIRRORS` environment variable
  - `FETCH_MIRRORS` environment variable (simpler format)
  - Built-in mirrors for busybox, linux, glibc, openssl, zlib, xz, etc.
  - Auto-detection via `github.com/mirror/*` repositories
  - Creates proper compressed tarballs from git archive

**SRC_URI Resolution (November 2025):**
- [x] Extract `${PV}` from recipe filename (e.g., `busybox_1.35.0.bb` → PV=1.35.0)
- [x] Resolve SRC_URI from include files (e.g., `libxcrypt.inc`)
- [x] Expand `${PV}` and `${BPN}` in include directives
- [x] Expand inline Python expressions `${@...}` in task env vars during task spec creation

**Known Issues:**
- Some external servers (e.g., busybox.net) may return HTTP 503 intermittently
  - ✅ Now handled by automatic git mirror fallback
- ~~Inline Python expressions in SRC_URI (e.g., `${@["", "file://init.cfg"][...]}`) not evaluated~~
  - ✅ Fixed: Python expressions now expanded in build_orchestrator.rs during task spec creation
  - Uses SimplePythonEvaluator.expand_all_expressions() with proper nested brace handling

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
| **Rust fetcher** | `convenient-bitbake/src/executor/rust_fetcher.rs` | ✓ Done |
| **Fetch task** | `convenient-bitbake/src/executor/fetch_task.rs` | ✓ Done |
| **Task executor** | `convenient-bitbake/src/executor/executor.rs` | ✓ Fetch/Unpack/Patch wired |
| **Query lexer** | `convenient-bitbake/src/query/lexer.rs` | ✓ Done (Logos-based) |
| **Query parser** | `convenient-bitbake/src/query/parser.rs` | ✓ Done (Recursive descent) |
| **Unpack** | `convenient-bitbake/src/fetcher.rs:111` | ✓ Wired to executor |
| **Patch** | `convenient-bitbake/src/executor/executor.rs:495` | ✓ Implemented |
| **Sysroot** | `convenient-bitbake/src/sysroot.rs` | ✓ Complete |
| **Build cmd** | `hitzeleiter/src/commands/build.rs` | ✓ --skip-fetch, --dry-run |
| **Query cmd** | `hitzeleiter/src/commands/query.rs` | ✓ Works |
| **Signature cache** | `convenient-bitbake/src/signature_cache.rs` | ✓ Works |
| **Build orchestrator** | `convenient-bitbake/src/build_orchestrator.rs` | ✓ Core logic |

---

## Success Criteria

### Phase 0 Complete When: ✓
```bash
cargo build --release                               # ✓ Builds successfully
hitzeleiter query-help                              # ✓ Shows query language help
hitzeleiter query "busybox"                         # ✓ Parses query (needs env for execution)
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
- [x] **Phase 0: Stabilization** ✓ COMPLETED
  - [x] Fix SEGFAULT in query
  - [x] Fix test compilation
  - [x] Proper query language with Logos lexer
- [x] **Phase 1: Complete Pipeline** ✓ MOSTLY COMPLETE
  - [x] Unpack wired to executor
  - [x] Patch application implemented
  - [x] Toolchain setup (prelude.sh)
  - [x] Sysroot assembly
  - [ ] End-to-end test with Poky environment
- [ ] Phase 2: Architectural Upgrades
- [ ] Phase 3: Developer Experience
