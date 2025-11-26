# Hitzeleiter TODO - Path to Working Build System

**Goal:** `hitzeleiter kas config.yml && hitzeleiter build busybox` produces working aarch64 binary

**Full roadmap:** [docs/development/roadmaps/bitbake-replacement-roadmap.md](docs/development/roadmaps/bitbake-replacement-roadmap.md)

---

## Immediate Priority: Source Preparation

### Fetch (Phase 1.1) - IMPLEMENTED & WIRED
- [x] Pure Rust HTTP fetcher (ureq + rustls) - no wget/curl
- [x] Pure Rust Git fetcher (git2/libgit2) - no git CLI
- [x] Proxy support (HTTP_PROXY, HTTPS_PROXY, NO_PROXY, ALL_PROXY)
- [x] SOCKS proxy support (socks5://, socks5h://)
- [x] Proxy authentication support
- [x] SRC_URI variable expansion (`${PV}`, `${PN}`)
- [x] Checksum verification (SHA256, MD5)
- [x] SRC_URI parameter parsing (;branch=, ;protocol=, etc.)
- [x] Wire to build orchestrator (executor.rs calls fetch_task for do_fetch)
- [x] Wire to KAS command (kas.rs uses rust_fetcher for repo cloning)
- [x] Enhanced SSH key support (ssh-agent, explicit key paths, default keys)
- [x] GitHub token authentication (x-access-token)
- [ ] **Test:** Download busybox tarball to DL_DIR

**Known Limitation:** git2 library has issues with complex proxy authentication
(JWT tokens in proxy URL). HTTP downloads work correctly through such proxies.
Workaround: Pre-clone repos or use NO_PROXY for git hosts when using JWT proxies.

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

1. ~~**Fetch is a stub**~~ - **RESOLVED**: Pure Rust fetcher now wired to executor
2. **Patch is empty** - Busybox needs patches
3. **No toolchain setup** - Can't cross-compile
4. **Sysroot not wired** - Dependencies not available

---

## Quick Reference: Key Files

| Component | File | Status |
|-----------|------|--------|
| **Rust fetcher** | `convenient-bitbake/src/executor/rust_fetcher.rs` | **DONE - Pure Rust** |
| **Fetch task** | `convenient-bitbake/src/executor/fetch_task.rs` | **DONE - SRC_URI parsing** |
| **Task executor** | `convenient-bitbake/src/executor/executor.rs` | **WIRED to fetch_task** |
| Fetch stub | `convenient-bitbake/src/executor/bbhelpers.rs:206` | STUB (bypassed for fetch) |
| Unpack | `convenient-bitbake/src/fetcher.rs:111` | Works, not wired |
| Patch | None | NOT IMPLEMENTED |
| Sysroot | `convenient-bitbake/src/sysroot.rs` | Exists, not wired |
| Build cmd | `hitzeleiter/src/commands/build.rs` | Ad-hoc, needs rewrite |
| Prelude | `convenient-bitbake/src/executor/prelude.sh` | Needs toolchain vars |

---

## Progress

- [x] Recipe parsing
- [x] Task graph building
- [x] KAS integration (setup only)
- [x] Caching infrastructure
- [x] Sandbox infrastructure
- [x] **Fetch** - Pure Rust implementation (ureq, git2)
- [x] **Fetch wiring** - Connected to executor.rs for do_fetch tasks
- [x] **Proxy support** - HTTP, HTTPS, SOCKS5, with authentication
- [x] **SSH support** - ssh-agent, explicit keys, default keys
- [ ] **Unpack** ← NEXT: Wire unpack_source to do_unpack
- [ ] Patch
- [ ] Toolchain
- [ ] Sysroot
- [ ] Configure
- [ ] Compile
- [ ] Install
- [ ] End-to-end test
