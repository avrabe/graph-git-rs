# KAS YAML Specification Reference

This document provides the complete specification for kas YAML configuration files based on the official kas documentation (v5.0).

## Overview

Kas is a setup tool for bitbake-based projects that uses YAML configuration files to describe:
- Which repositories and layers to use
- Build configuration (machine, distro, targets)
- Environment settings
- Patches to apply

Configuration files merge depth-first and top-to-bottom, allowing layered configuration with includes.

## Core Structure

### Minimal Configuration

```yaml
header:
  version: 14  # Latest format version

machine: qemux86-64
distro: poky
```

### Complete Configuration Example

```yaml
header:
  version: 14
  includes:
    - base.yml
    - debug-config.yml

machine: qemux86-64
distro: poky
target:
  - core-image-minimal
  - core-image-sato
task: build

build_system: openembedded

env:
  BB_ENV_PASSTHROUGH_ADDITIONS: SSTATE_DIR DL_DIR

repos:
  poky:
    url: https://git.yoctoproject.org/poky
    branch: kirkstone
    layers:
      meta:
      meta-poky:
      meta-yocto-bsp:

  meta-openembedded:
    url: https://git.openembedded.org/meta-openembedded
    branch: kirkstone
    layers:
      meta-oe:
      meta-python:
      meta-networking:

local_conf_header:
  standard: |
    PACKAGE_CLASSES = "package_ipk"
  debug: |
    EXTRA_IMAGE_FEATURES += "debug-tweaks"

bblayers_conf_header:
  meta-custom: |
    # Custom layer configuration
    BBMASK = "meta-*/recipes-test/*"
```

## Field Reference

### Header Section

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `version` | integer | Yes | Format version (current: 14) |
| `includes` | list[string\|dict] | No | Files to include (relative paths or cross-repo) |

**Include formats:**
```yaml
# Simple in-tree include
includes:
  - common.yml
  - machines/qemu.yml

# Cross-repo include
includes:
  - repo: meta-custom
    file: kas-config.yml
```

### Machine & Distro

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `machine` | string | Yes | Target machine (sets MACHINE in local.conf) |
| `distro` | string | No | Distribution (sets DISTRO, default: "poky") |
| `build_system` | string | No | "openembedded" or "isar" |

### Build Targets

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `target` | string\|list[string] | No | Recipes to build (default: "core-image-minimal") |
| `task` | string | No | BitBake task (default: "build") |

### Repository Configuration (`repos`)

Each repository is a dictionary key with these properties:

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `url` | string | No* | Git repository URL (*required if not local) |
| `path` | string | No | Local path (alternative to url) |
| `type` | string | No | Repository type: "git" (default) or "hg" |
| `refspec` | string | No | Git refspec to fetch |
| `commit` | string | No | Full commit SHA to checkout |
| `branch` | string | No | Branch to checkout |
| `tag` | string | No | Tag to checkout |
| `layers` | dict | No | Layers within repository |
| `patches` | dict | No | Patches to apply |
| `signed` | boolean | No | Require GPG/SSH signatures |

**Priority:** If multiple refs specified: `commit` > `tag` > `branch` > `refspec`

**Layers format:**
```yaml
repos:
  poky:
    url: https://git.yoctoproject.org/poky
    branch: kirkstone
    layers:
      meta:                    # Layer at repo root/meta
      meta-poky:               # Layer at repo root/meta-poky
      meta-yocto-bsp:
        path: path/to/layer    # Layer at custom path
```

**Patches format:**
```yaml
repos:
  meta-custom:
    url: https://example.com/meta-custom
    patches:
      patch-set-1:
        - 0001-fix-something.patch
        - 0002-add-feature.patch
```

### Configuration Headers

| Field | Type | Description |
|-------|------|-------------|
| `local_conf_header` | dict[str, str] | Sections for local.conf |
| `bblayers_conf_header` | dict[str, str] | Sections for bblayers.conf |

Entries are applied alphabetically by section ID.

```yaml
local_conf_header:
  00-base: |
    PACKAGE_CLASSES = "package_ipk"
    BB_NUMBER_THREADS = "8"

  10-features: |
    EXTRA_IMAGE_FEATURES += "debug-tweaks"
    EXTRA_IMAGE_FEATURES += "package-management"
```

### Environment Variables

| Field | Type | Description |
|-------|------|-------------|
| `env` | dict[str, str] | Environment variables for BitBake |

```yaml
env:
  SSTATE_DIR: /shared/sstate-cache
  DL_DIR: /shared/downloads
  BB_ENV_PASSTHROUGH_ADDITIONS: SSTATE_DIR DL_DIR
```

### Advanced Fields

| Field | Type | Description |
|-------|------|-------------|
| `defaults` | dict | Default values for repos (branch, patches, etc.) |
| `artifacts` | dict | Expected build outputs with glob patterns |
| `buildtools` | dict | Yocto buildtools configuration |
| `signers` | dict | GPG/SSH key specifications |
| `overrides` | dict | Auto-generated commit overrides (lockfiles) |
| `menu_configuration` | dict | Kconfig variables (menu plugin) |

## Include Mechanism

Files merge depth-first and top-to-bottom:

1. **Load root file**
2. **Process includes recursively** (depth-first)
3. **Merge configurations** (later overrides earlier)

**Merge behavior:**
- Simple values: later overwrites earlier
- Lists: concatenated (earlier + later)
- Dictionaries: merged recursively (keys combined, conflicts resolved)

**Include types:**

```yaml
# 1. In-tree relative path
header:
  includes:
    - common/base.yml
    - machines/qemux86-64.yml

# 2. Cross-repository include
header:
  includes:
    - repo: meta-custom
      file: kas/config.yml

# 3. Lockfile (automatic)
# File: project.yml.lock.yml
# Automatically included and overrides commit refs
```

## Lockfiles

Lockfiles freeze repository commits for reproducible builds:

**Naming:** `<original>.lock.<ext>`
- `project.yml` → `project.yml.lock.yml`

**Content:**
```yaml
# project.yml.lock.yml
overrides:
  repos:
    poky:
      commit: abc123def456...
    meta-openembedded:
      commit: 789ghi012jkl...
```

Lockfiles are automatically loaded and override repository refs without modifying source files.

## Version History

| Version | Changes |
|---------|---------|
| 1 | Initial format |
| 2 | Added `task` field |
| 3 | Added repository `signed` verification |
| 4 | Added `defaults` section |
| 5 | Added cross-repo includes |
| 6 | Added `buildtools` section |
| 7 | Added `menu_configuration` |
| 8 | Added `artifacts` section |
| 9 | Repository URL now optional (for local repos) |
| 10 | Added `overrides` for lockfiles |
| 11 | Added `env` section |
| 12 | Added include from command line |
| 13 | Added `build_system` field |
| 14 | Current version |

## Validation Rules

1. **Header required:** Must have `header.version`
2. **Machine required:** Must specify `machine`
3. **Repository refs:** Use one of: commit, tag, branch, or refspec
4. **Layer paths:** Relative to repository root
5. **Circular includes:** Not allowed
6. **Lockfile priority:** Overrides always win

## Common Patterns

### Multi-Machine Configuration

```yaml
# base.yml
header:
  version: 14
distro: poky
repos:
  poky:
    url: https://git.yoctoproject.org/poky
    branch: kirkstone
    layers:
      meta:
      meta-poky:

# qemux86-64.yml
header:
  version: 14
  includes:
    - base.yml
machine: qemux86-64

# raspberrypi4.yml
header:
  version: 14
  includes:
    - base.yml
machine: raspberrypi4-64
repos:
  meta-raspberrypi:
    url: https://git.yoctoproject.org/meta-raspberrypi
    branch: kirkstone
```

### Debug vs Release

```yaml
# common.yml
header:
  version: 14
machine: qemux86-64
distro: poky

# debug.yml
header:
  version: 14
  includes:
    - common.yml
local_conf_header:
  debug: |
    EXTRA_IMAGE_FEATURES += "debug-tweaks"
    IMAGE_INSTALL:append = " gdbserver strace"

# release.yml
header:
  version: 14
  includes:
    - common.yml
local_conf_header:
  release: |
    EXTRA_IMAGE_FEATURES:remove = "debug-tweaks"
```

### Layer Ordering

```yaml
repos:
  # Layers are added to bblayers.conf in this order
  poky:
    layers:
      meta:              # First
      meta-poky:         # Second

  meta-openembedded:
    layers:
      meta-oe:           # Third
      meta-python:       # Fourth
```

## References

- **Official Documentation:** https://kas.readthedocs.io/
- **GitHub Repository:** https://github.com/siemens/kas
- **Yocto Project:** https://www.yoctoproject.org/
- **BitBake Manual:** https://docs.yoctoproject.org/bitbake/

## Implementation Notes for Bitzel

When implementing kas support in Bitzel:

1. **Version Support:** Start with version 14, consider backwards compatibility
2. **Include Resolution:** Implement depth-first traversal with cycle detection
3. **Merge Logic:** Deep merge for dicts, concatenate for lists, overwrite for scalars
4. **Checksum Tracking:** Hash all included files for cache invalidation
5. **Lockfile Support:** Auto-load `.lock.` files and apply overrides
6. **Validation:** Check required fields and validate repository refs
7. **Error Messages:** Provide clear errors for parse failures and missing includes
8. **Testing:** Test with real Yocto kas configurations from poky, meta-openembedded

## Test Coverage Requirements

For 100% kas reading & action coverage:

1. **Basic parsing:** All field types (string, int, list, dict)
2. **Includes:** In-tree, cross-repo, circular detection
3. **Merging:** Override behavior, list concatenation, dict merging
4. **Repositories:** All ref types (commit, tag, branch, refspec)
5. **Layers:** Root layers, custom paths, empty layers
6. **Patches:** Single patches, patch sets
7. **Headers:** local.conf and bblayers.conf sections
8. **Environment:** Variable propagation
9. **Lockfiles:** Override application
10. **Error cases:** Invalid YAML, missing fields, circular includes, bad refs
