# Hitzeleiter

[![Rust](https://github.com/avrabe/graph-git-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/avrabe/graph-git-rs/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/avrabe/graph-git-rs/graph/badge.svg?token=9rYlCv0G2W)](https://codecov.io/gh/avrabe/graph-git-rs)

**Hitzeleiter** (German: "hot conductor") is a modern, Bazel-inspired build orchestration system for BitBake and Yocto projects. Written in Rust, it provides content-addressable caching, hermetic sandboxing, and a powerful query language for exploring build dependencies.

## Features

- 🚀 **Content-Addressable Caching**: Fast incremental builds with CAS and action cache
- 🔒 **Hermetic Execution**: Linux namespace sandboxing for reproducible builds
- 🦀 **Pure Rust Implementation**: No host tool contamination for fetch/unpack/patch tasks
- 📊 **Query Language**: Bazel-style dependency exploration (deps, rdeps, allpaths)
- 🎯 **KAS Integration**: Native support for KAS configuration files
- ⚡ **Parallel Execution**: Multi-threaded recipe parsing and task execution
- 🔍 **Advanced Variable Resolution**: Full BitBake override syntax support

## Quick Start

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install protobuf compiler (required for build)
# Debian/Ubuntu:
sudo apt-get install protobuf-compiler

# macOS:
brew install protobuf
```

### Build from Source

```bash
git clone https://github.com/avrabe/graph-git-rs.git
cd graph-git-rs
cargo build --release
```

### Setup Build Environment with KAS

```bash
# Initialize from KAS configuration
./target/release/hitzeleiter kas test-fixtures/kas-configs/busybox-qemuarm64.yml

# Build a target
./target/release/hitzeleiter build -b build busybox
```

### Query Dependencies

```bash
# Find all dependencies of busybox
hitzeleiter query "deps(busybox)"

# Find reverse dependencies
hitzeleiter query "rdeps(glibc)"

# Find dependency paths
hitzeleiter query "allpaths(busybox, glibc)"
```

## Architecture

Hitzeleiter consists of several Rust crates organized as a workspace:

- **hitzeleiter**: Main CLI application and command orchestration
- **convenient-bitbake**: BitBake recipe parsing, task graph building, and execution
- **convenient-cache**: Content-addressable storage and action caching
- **convenient-kas**: KAS configuration file parsing and repository management
- **convenient-git**: Pure Rust Git operations for repository fetching
- **convenient-graph**: Generic graph data structures and algorithms
- **convenient-repo**: Repository manifest handling

### Execution Modes

Tasks can execute in three modes:

1. **DirectRust**: Pure Rust execution (fetch, unpack, patch) - no sandbox overhead
2. **RustShell**: In-process bash interpreter with variable tracking
3. **Sandboxed**: Full Linux namespace isolation for complex shell/Python tasks

## Current Status

⚠️ **Pre-release**: Core infrastructure is complete, but end-to-end builds are not yet fully validated.

**What works:**
- ✅ BitBake recipe parsing with full override resolution
- ✅ Recipe and task dependency graph building
- ✅ Task graph filtering to specific build targets
- ✅ KAS configuration parsing and repository fetching
- ✅ Pure Rust fetch/unpack/patch task execution
- ✅ Content-addressable caching infrastructure
- ✅ Query language for dependency exploration

**In progress:**
- ⚠️ End-to-end validation of complete compile→install→package pipeline
- ⚠️ Complex recipe builds (kernel, toolchain, glibc)
- ⚠️ Remote execution API (REAPI v2) support

See [Roadmap](docs/development/status/roadmap.md) for the complete roadmap and [docs/](docs/) for detailed documentation.

## Documentation

Comprehensive documentation is available in the `docs/` directory:

- [Architecture](docs/architecture/) - System design and technical decisions
- [Reference](docs/reference/) - BitBake and KAS specifications
- [Guides](docs/guides/) - How-to guides and tutorials
- [Development](docs/development/) - Roadmaps and implementation status

## Project Goals

Hitzeleiter aims to become a state-of-the-art build system by combining:

- **BitBake compatibility**: Parse and execute existing Yocto recipes
- **Bazel performance**: Content-addressable caching and remote execution
- **Modern tooling**: Rust safety, parallel execution, incremental computation

Long-term vision includes:
- Adapton-style demand-driven incremental computation
- Shake-like monadic dependencies for dynamic dependency discovery
- Full REAPI v2 support for distributed builds
- Build Systems à la Carte scheduler architecture

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run tests for specific crate
cargo test -p convenient-bitbake

# Run with release optimizations
cargo test --release
```

### Code Structure

Follow the guidelines in [CLAUDE.md](CLAUDE.md) for:
- Documentation organization
- File naming conventions
- Build prerequisites
- Workspace structure

## Contributing

This is an active research and development project. The codebase is evolving rapidly as we work toward production-ready builds.

## License

MIT OR Apache-2.0

## Acknowledgments

Inspired by:
- [Buck2](https://buck2.build/) and [Bazel](https://bazel.build/) - Modern build systems
- [BitBake](https://docs.yoctoproject.org/bitbake/) - Embedded Linux build tool
- [KAS](https://kas.readthedocs.io/) - Setup tool for BitBake projects
- Research: "Build Systems à la Carte", "Adapton", "Shake Before Building"
