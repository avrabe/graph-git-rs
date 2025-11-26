# CLAUDE.md - Project Guidelines for AI Assistants

This file contains guidelines and conventions for AI assistants working on this project.

## Documentation Guidelines

### Directory Structure

All documentation MUST be placed in the `docs/` directory following this structure:

```
docs/
├── README.md              # Documentation index (always update when adding docs)
├── architecture/          # Design & architecture decisions
│   └── *.md              # System design, architectural decisions, technical designs
├── reference/             # Specifications & technical reference
│   └── *.md              # File format specs, API specs, protocol specs
├── development/           # Development history & roadmaps
│   ├── roadmaps/         # Implementation plans and feature roadmaps
│   ├── phases/           # Phase completion summaries
│   └── status/           # Current implementation status updates
├── guides/               # How-to guides and tutorials
│   └── *.md              # Step-by-step guides, setup instructions
├── analysis/             # Analysis documents & reports
│   └── *.md              # Code analysis, performance analysis, evaluations
└── reports/              # Validation & test reports
    └── *.md              # Test results, validation reports, benchmarks
```

### File Naming Conventions

- Use **lowercase-kebab-case** for all documentation files: `my-document-name.md`
- Do NOT use SCREAMING_CASE or PascalCase
- Be descriptive but concise: `execution-and-sandboxing.md` not `EXECUTION_SANDBOXING_GUIDE.md`

### When Creating New Documentation

1. **Choose the correct category** based on document purpose:
   - Architecture decision? → `docs/architecture/`
   - Technical specification? → `docs/reference/`
   - Implementation roadmap? → `docs/development/roadmaps/`
   - Phase summary? → `docs/development/phases/`
   - How-to guide? → `docs/guides/`
   - Analysis or evaluation? → `docs/analysis/`
   - Test or validation results? → `docs/reports/`

2. **Update the index**: After creating a new document, add it to `docs/README.md`

3. **Keep root directory clean**: Only `Readme.md` (and this file) should be in root
   - Crate-specific READMEs go in their crate directories

### Documentation Content Guidelines

- Start with a clear `# Title` heading
- Include a brief overview/purpose section
- Use proper markdown formatting
- Link to related documents when relevant
- Date significant documents (status updates, reports)

## Code Organization

### Crate Structure

This is a Cargo workspace with these main crates:

- `hitzeleiter/` - Main CLI application
- `convenient-bitbake/` - Core BitBake implementation
- `convenient-cache/` - Caching subsystem
- `convenient-git/` - Git integration
- `convenient-graph/` - Dependency graph
- `convenient-kas/` - KAS configuration
- `convenient-repo/` - Repo management
- `graph-git/` - Git graph analysis
- `graph-git-cli/` - CLI interface

### Workspace-Relative Paths

The build system uses workspace-relative paths for all generated artifacts:

- Cache directory: `build/.hitzeleiter-cache/`
- Sandbox directory: `build/.hitzeleiter-cache/sandboxes/`
- Prelude script: Written to sandbox directory and bind-mounted

**Important**: Never write to system directories like `/hitzeleiter/` directly.
The prelude.sh should be:
1. Written to the workspace cache directory
2. Bind-mounted into sandboxes as `/hitzeleiter/prelude.sh`

This ensures:
- Multiple workspaces can have different prelude versions
- No root permissions required
- Proper caching and versioning

## Testing

- Run tests with: `cargo test`
- Run with release optimizations: `cargo test --release`
- Individual crate tests: `cargo test -p crate-name`

## Build Commands

- Debug build: `cargo build`
- Release build: `cargo build --release`
- Run hitzeleiter: `cargo run -p hitzeleiter -- [args]`
