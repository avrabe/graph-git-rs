# Hitzeleiter TODO - Path to Working Build System

**Goal:** `hitzeleiter kas config.yml && hitzeleiter build busybox` produces working aarch64 binary

**Full roadmap:** [docs/development/roadmaps/bitbake-replacement-roadmap.md](docs/development/roadmaps/bitbake-replacement-roadmap.md)

---

## Immediate Priority: Source Preparation

### Fetch (Phase 1.1) - START HERE
- [ ] Wire `fetch_handler.rs` to `do_fetch` task execution
- [ ] Replace stub in `bbhelpers.rs:206` with real implementation
- [ ] SRC_URI variable expansion (`${PV}`, `${PN}`)
- [ ] Checksum verification
- [ ] **Test:** Download busybox tarball to DL_DIR

### Unpack (Phase 1.2)
- [ ] Connect `fetcher.rs:unpack_source()` to `do_unpack` task
- [ ] Handle `S = "${WORKDIR}/busybox-${PV}"` path
- [ ] **Test:** Tarball extracted to correct ${S}

### Patch (Phase 1.3)
- [ ] Implement `patch -p1` for .patch files in SRC_URI
- [ ] Apply in order specified
- [ ] **Test:** Busybox patches applied

---

## Next: Build Environment

### Toolchain (Phase 2.1)
- [ ] MACHINE → toolchain mapping (qemuarm64 → aarch64-linux-gnu)
- [ ] Set CC, CXX, LD, AR, CFLAGS, LDFLAGS
- [ ] **Test:** Cross-compile hello.c for aarch64

### Sysroot (Phase 2.2)
- [ ] Wire existing `sysroot.rs` to build pipeline
- [ ] Assemble recipe-sysroot from DEPENDS
- [ ] **Test:** Headers available from dependencies

---

## Then: Task Execution

### Configure (Phase 3.1)
- [ ] Busybox: copy defconfig, run `make oldconfig`
- [ ] Autotools: `--host=aarch64-linux-gnu`
- [ ] **Test:** Busybox configures

### Compile (Phase 3.2)
- [ ] Verify `oe_runmake` works with cross-compiler
- [ ] **Test:** `file busybox` → ARM aarch64

### Install (Phase 3.3)
- [ ] `make install DESTDIR=${D}`
- [ ] **Test:** Binary in ${D}/usr/bin/

---

## Current Blockers

1. **Fetch is a stub** - No source code = no build
2. **Patch is empty** - Busybox needs patches
3. **No toolchain setup** - Can't cross-compile
4. **Sysroot not wired** - Dependencies not available

---

## Quick Reference: Key Files

| Component | File | Status |
|-----------|------|--------|
| Fetch stub | `convenient-bitbake/src/executor/bbhelpers.rs:206` | STUB |
| Real fetcher | `convenient-bitbake/src/executor/fetch_handler.rs` | Exists, not wired |
| Unpack | `convenient-bitbake/src/fetcher.rs:111` | Works, not wired |
| Patch | None | NOT IMPLEMENTED |
| Sysroot | `convenient-bitbake/src/sysroot.rs` | Exists, not wired |
| Build cmd | `hitzeleiter/src/commands/build.rs` | Ad-hoc, needs rewrite |
| Task exec | `convenient-bitbake/src/executor/executor.rs` | Works |
| Prelude | `convenient-bitbake/src/executor/prelude.sh` | Needs toolchain vars |

---

## Progress

- [x] Recipe parsing
- [x] Task graph building
- [x] KAS integration (setup only)
- [x] Caching infrastructure
- [x] Sandbox infrastructure
- [ ] **Fetch** ← START HERE
- [ ] Unpack
- [ ] Patch
- [ ] Toolchain
- [ ] Sysroot
- [ ] Configure
- [ ] Compile
- [ ] Install
- [ ] End-to-end test
