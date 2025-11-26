# Documentation

This directory contains all project documentation organized by category.

## Directory Structure

```
docs/
├── architecture/      # Design & architecture decisions
├── reference/         # Specifications & technical reference
├── development/       # Roadmaps, phases, and status
│   ├── roadmaps/     # Implementation and feature roadmaps
│   ├── phases/       # Phase completion summaries
│   └── status/       # Current implementation status
├── guides/           # How-to guides and tutorials
├── analysis/         # Analysis documents & reports
└── reports/          # Validation & test reports
```

## Architecture

Core system design and architectural decisions.

- [Overview](architecture/overview.md) - Main Hitzeleiter architecture
- [Execution & Sandboxing](architecture/execution-and-sandboxing.md) - Task execution modes
- [Caching](architecture/caching.md) - Content-addressable caching strategy
- [Parser](architecture/parser.md) - Rowan-based BitBake parser
- [Query System](architecture/query-system.md) - Bazel-inspired query interface
- [Sandbox Design](architecture/sandbox-design.md) - Linux namespace sandboxing
- [Sysroot Design](architecture/sysroot-design.md) - Hardlink-based sysroot assembly
- [Bootstrap](architecture/bootstrap.md) - Bootstrap architecture
- [Parallelism](architecture/parallelism.md) - Async runtime strategy
- [External Executor](architecture/external-executor.md) - External execution framework

## Reference

Technical specifications and reference material.

- [BitBake Specification](reference/bitbake-specification.md) - BitBake file format spec
- [BitBake Variable Resolution](reference/bitbake-variable-resolution.md) - Variable resolution strategy
- [KAS Specification](reference/kas-specification.md) - KAS YAML configuration
- [Modern BitBake Structure](reference/modern-bitbake-structure.md) - Poky/OE structure

## Development

Implementation progress, roadmaps, and phase summaries.

### Roadmaps
- [BitBake Replacement Roadmap](development/roadmaps/bitbake-replacement-roadmap.md) - **Path to working build system**
- [Executive Summary](development/roadmaps/executive-summary.md) - High-level roadmap overview
- [Accuracy Roadmap](development/roadmaps/accuracy-roadmap.md) - Path to 95% accuracy
- [BitBake Implementation](development/roadmaps/bitbake-implementation.md) - Parser implementation plan

### Phase Summaries
- [Phases 1-6](development/phases/phases-1-6.md) - Foundation phases
- [Phase 7](development/phases/phase-7.md) - Dependency extraction improvements
- [Phase 8](development/phases/phase-8.md) - 90% accuracy milestone
- [Phase 9](development/phases/phase-9.md) - 92-93% accuracy
- [Phase 10](development/phases/phase-10.md) - Python IR architecture
- [Phase 11](development/phases/phase-11.md) - RustPython integration

### Status
- [Implementation Status](development/status/implementation-status.md) - Current status
- [RustPython Integration](development/status/rustpython-integration.md) - Python VM status

## Guides

Practical how-to guides.

- [Bazel Cache Setup](guides/bazel-cache-setup.md) - Remote cache configuration
- [Task Execution](guides/busybox-task-execution.md) - Busybox task example
- [Ferrari Build](guides/ferrari-build.md) - Full-featured build infrastructure
- [Native BitBake Support](guides/native-bitbake-support.md) - Working with Yocto builds

## Analysis

Technical analysis and evaluations.

- [Code Review](analysis/code-review.md) - Comprehensive code review
- [Performance](analysis/performance.md) - Performance analysis
- [Python Challenges](analysis/python-challenges.md) - Python execution challenges
- [RustPython Analysis](analysis/rustpython-analysis.md) - RustPython evaluation
- [License Analysis](analysis/license-analysis.md) - Dependency license review

## Reports

Validation and test results.

- [Validation Report](reports/validation-report.md) - BitBake comparison validation
- [Test Report](reports/test-report.md) - Comprehensive test results
