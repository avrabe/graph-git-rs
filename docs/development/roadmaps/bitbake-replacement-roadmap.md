# Hitzeleiter: Path to Complete BitBake Replacement

**Goal:** Build busybox for qemuarm64 using only `hitzeleiter kas config.yml && hitzeleiter build busybox`

**Current State:** Recipe parser + task scheduler (no actual build execution)

**Target State:** Full build system with minimal host dependencies

---

## Milestone 1: First Working Build (Busybox for qemuarm64)

### Phase 1: Source Preparation (Priority: CRITICAL)

#### 1.1 Wire Up Real Fetch to Tasks
- [ ] Connect `fetch_handler.rs` to `do_fetch` task execution
- [ ] Replace ad-hoc SRC_URI parsing in build.rs with proper fetcher
- [ ] Implement SRC_URI variable expansion (handle `${PV}`, `${PN}`, `${BP}`)
- [ ] Add download directory management (`DL_DIR`)
- [ ] Implement checksum verification (md5sum, sha256sum from SRC_URI)
- [ ] Add basic mirror/premirror support
- [ ] Test: Fetch busybox source tarball successfully

**Files to modify:**
- `convenient-bitbake/src/executor/bbhelpers.rs` - Replace stub `base_do_fetch()`
- `hitzeleiter/src/commands/build.rs` - Remove ad-hoc fetch, use fetcher
- `convenient-bitbake/src/fetcher.rs` - Ensure integration

**Effort:** 1 week

#### 1.2 Implement Real Unpack
- [ ] Connect `unpack_source()` from fetcher.rs to `do_unpack` task
- [ ] Handle multiple SRC_URI entries (main source + patches + configs)
- [ ] Set up `${S}` (source directory) correctly
- [ ] Handle `S = "${WORKDIR}/busybox-${PV}"` style overrides
- [ ] Support subdir parameter in SRC_URI
- [ ] Test: Unpack busybox tarball to correct location

**Files to modify:**
- `convenient-bitbake/src/executor/bbhelpers.rs` - Replace stub `base_do_unpack()`
- `convenient-bitbake/src/fetcher.rs` - Add SRC_URI file:// handling

**Effort:** 3-5 days

#### 1.3 Implement Patch Application
- [ ] Parse `file://` entries from SRC_URI that are .patch files
- [ ] Implement `patch -p1` application in correct order
- [ ] Handle PATCHDIR variable
- [ ] Handle striplevel (;striplevel=N in SRC_URI)
- [ ] Support .diff files
- [ ] Implement quilt-style patch management (optional, lower priority)
- [ ] Test: Apply busybox patches successfully

**Files to modify:**
- `convenient-bitbake/src/executor/bbhelpers.rs` - Replace stub `base_do_patch()`
- New file: `convenient-bitbake/src/executor/patch_handler.rs`

**Effort:** 1 week

---

### Phase 2: Build Environment (Priority: CRITICAL)

#### 2.1 Cross-Compilation Toolchain Integration
- [ ] Define MACHINE → toolchain mapping (qemuarm64 → aarch64-linux-gnu)
- [ ] Locate or download cross-compiler
- [ ] Set CC, CXX, LD, AR, AS, STRIP, OBJCOPY, OBJDUMP
- [ ] Set TARGET_ARCH, TARGET_OS, TARGET_SYS, BUILD_SYS
- [ ] Set CFLAGS, LDFLAGS with correct -march, -mtune
- [ ] Handle HOST vs TARGET vs BUILD triplets
- [ ] Test: Cross-compile a simple C file for aarch64

**Files to modify:**
- `convenient-bitbake/src/executor/prelude.sh` - Add toolchain setup
- New file: `convenient-bitbake/src/toolchain.rs`
- `hitzeleiter/src/commands/build.rs` - Pass machine config

**Effort:** 2 weeks

#### 2.2 Sysroot Assembly
- [ ] Connect existing `sysroot.rs` to build pipeline
- [ ] Build dependency tree for recipe
- [ ] Assemble recipe-sysroot from DEPENDS packages
- [ ] Set STAGING_DIR_HOST, STAGING_DIR_NATIVE
- [ ] Handle native vs target dependencies
- [ ] Implement hardlink-based sysroot (code exists, needs integration)
- [ ] Test: Sysroot contains headers from dependency

**Files to modify:**
- `convenient-bitbake/src/sysroot.rs` - Already implemented, needs wiring
- `convenient-bitbake/src/build_orchestrator.rs` - Add sysroot step
- `convenient-bitbake/src/executor/executor.rs` - Mount sysroot in sandbox

**Effort:** 1 week

#### 2.3 Native Tool Building
- [ ] Identify native dependencies (e.g., busybox needs nothing, but others need native tools)
- [ ] Build native tools with host compiler
- [ ] Install to STAGING_BINDIR_NATIVE
- [ ] Add to PATH for cross builds
- [ ] Test: Native tool available during cross build

**Effort:** 1 week

---

### Phase 3: Task Implementation (Priority: HIGH)

#### 3.1 Configure Task
- [ ] Implement `base_do_configure()` properly
- [ ] Handle autotools (autoconf/automake detection)
- [ ] Handle cmake detection
- [ ] Handle meson detection
- [ ] Busybox-specific: Copy defconfig, run `make oldconfig`
- [ ] Pass correct --host, --build flags for cross-compile
- [ ] Set PKG_CONFIG_PATH for cross-compilation
- [ ] Test: Busybox configures without errors

**Files to modify:**
- `convenient-bitbake/src/executor/bbhelpers.rs` - Implement `base_do_configure()`
- Add autotools, cmake, meson helper functions

**Effort:** 1 week

#### 3.2 Compile Task
- [ ] `base_do_compile()` already calls make - verify it works
- [ ] Ensure PARALLEL_MAKE is set correctly
- [ ] Handle EXTRA_OEMAKE
- [ ] Cross-compilation flags passed correctly
- [ ] Test: Busybox compiles to aarch64 binary

**Effort:** 3-5 days

#### 3.3 Install Task
- [ ] Implement `base_do_install()` properly
- [ ] Run `make install DESTDIR=${D}`
- [ ] Handle busybox-specific install
- [ ] Create proper directory structure in ${D}
- [ ] Test: Busybox installed to ${D}/usr/bin/busybox

**Effort:** 3-5 days

---

### Phase 4: Variable System (Priority: HIGH)

#### 4.1 Core Variable Expansion
- [ ] Implement full `${VAR}` expansion (recursive)
- [ ] Handle `${@python_expression}` (re-enable RustPython)
- [ ] Implement variable overrides (`VAR:append`, `VAR:prepend`)
- [ ] Implement conditional overrides (`VAR:qemuarm64`)
- [ ] Handle `??=`, `?=`, `=`, `:=` assignment types
- [ ] Test: Complex variable expressions resolve correctly

**Files to modify:**
- `convenient-bitbake/src/variable_resolver.rs` - Enhance existing
- `convenient-bitbake/src/python_executor.rs` - Re-enable

**Effort:** 1-2 weeks

#### 4.2 Standard Variables
- [ ] Ensure all standard variables are set:
  - PN, PV, PR, PF, P, BP, BPN
  - WORKDIR, S, B, D, T
  - STAGING_* variables
  - TARGET_*, BUILD_*, HOST_*
- [ ] Load from conf/bitbake.conf or equivalent
- [ ] Test: `printenv` in task shows correct values

**Effort:** 1 week

---

### Phase 5: Integration & Testing (Priority: HIGH)

#### 5.1 End-to-End Pipeline
- [ ] Wire all phases together in build_orchestrator.rs
- [ ] Proper task ordering with real execution
- [ ] Error handling and recovery
- [ ] Progress reporting
- [ ] Test: Full busybox build from kas file

**Effort:** 1 week

#### 5.2 Verification
- [ ] Verify busybox binary is aarch64
- [ ] Verify busybox runs in qemu-aarch64
- [ ] Verify all busybox applets work
- [ ] Compare output with real BitBake build
- [ ] Test: `file busybox` shows "ELF 64-bit LSB executable, ARM aarch64"

**Effort:** 1 week

---

## Milestone 2: Minimal Host Dependencies

### Phase 6: Bootstrapping

#### 6.1 Toolchain Bootstrap
- [ ] Download pre-built cross-toolchain (or build with crosstool-ng)
- [ ] Package toolchain as fetchable artifact
- [ ] Auto-download on first build if not present
- [ ] Cache toolchain in DL_DIR
- [ ] Test: Build works on fresh system with only Rust installed

**Effort:** 2 weeks

#### 6.2 Host Tool Minimization
- [ ] Identify minimum host requirements
- [ ] Bundle or build: tar, gzip, bzip2, xz, patch, make
- [ ] Use Rust implementations where possible (e.g., flate2 for gzip)
- [ ] Document remaining host requirements
- [ ] Test: Build works with minimal host tools

**Effort:** 2 weeks

---

## Milestone 3: Feature Parity

### Phase 7: Package Output

- [ ] Implement do_package - split into packages
- [ ] Implement do_package_write_ipk (or rpm/deb)
- [ ] Generate package manifests
- [ ] Handle PACKAGES variable
- [ ] Handle FILES:${PN} patterns

**Effort:** 2-3 weeks

### Phase 8: Image Generation

- [ ] Implement image recipes
- [ ] Root filesystem assembly
- [ ] Image types (ext4, wic, etc.)
- [ ] Boot configuration

**Effort:** 3-4 weeks

### Phase 9: Advanced Features

- [ ] Shared state (sstate) for distributed builds
- [ ] PR server for version tracking
- [ ] Multiconfig builds
- [ ] SDK generation

**Effort:** Ongoing

---

## Progress Tracking

### Current Status

| Phase | Status | Progress |
|-------|--------|----------|
| 1.1 Fetch | Not Started | 0% |
| 1.2 Unpack | Not Started | 0% |
| 1.3 Patch | Not Started | 0% |
| 2.1 Toolchain | Not Started | 0% |
| 2.2 Sysroot | Code exists, not wired | 20% |
| 2.3 Native tools | Not Started | 0% |
| 3.1 Configure | Stub | 5% |
| 3.2 Compile | Partial | 30% |
| 3.3 Install | Stub | 5% |
| 4.1 Variables | Partial | 40% |
| 4.2 Std vars | Partial | 30% |
| 5.1 Integration | Not Started | 0% |

### Estimated Timeline

**Milestone 1 (First Working Build):** 8-10 weeks
- Phase 1: 2.5 weeks
- Phase 2: 4 weeks
- Phase 3: 2 weeks
- Phase 4: 2 weeks
- Phase 5: 2 weeks

**Milestone 2 (Minimal Host Deps):** +4 weeks

**Milestone 3 (Feature Parity):** +8-12 weeks

---

## Next Actions (Immediate)

1. **START HERE:** Phase 1.1 - Wire up fetch_handler.rs to actual task execution
2. Get a single `do_fetch` task to actually download busybox tarball
3. Verify tarball lands in DL_DIR with correct checksum

This is the critical path - everything else depends on having real source code to work with.
