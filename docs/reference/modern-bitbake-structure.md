# Modern BitBake/Poky/OpenEmbedded Structure (2024+)

## Overview

As of recent Yocto versions, **Poky is no longer a monolithic repository**. It has been split into separate components that are cloned individually and composed together.

This document describes the modern structure and how bitzel needs to work with it.

## Repository Structure

### 1. Separate Repositories

Modern Poky consists of three independent repositories:

```bash
mkdir layers/
git clone -b yocto-5.2 https://git.openembedded.org/bitbake ./layers/bitbake
git clone -b yocto-5.2 https://git.openembedded.org/openembedded-core ./layers/openembedded-core
git clone -b yocto-5.2 https://git.yoctoproject.org/meta-yocto ./layers/meta-yocto
```

**Components:**
- **bitbake** - The build engine itself
- **openembedded-core** - Core recipes and metadata (contains meta/ layer)
- **meta-yocto** - Poky reference distribution (contains meta-poky layer)

### 2. Directory Layout After Clone

```
project/
├── layers/
│   ├── bitbake/                    # BitBake build engine
│   │   └── bin/bitbake             # The bitbake executable
│   ├── openembedded-core/          # OE-Core layer
│   │   ├── meta/                   # Main metadata layer
│   │   │   ├── conf/
│   │   │   │   ├── bitbake.conf   # Core BitBake config
│   │   │   │   ├── layer.conf      # Layer metadata
│   │   │   │   └── machine/        # Machine configs
│   │   │   ├── recipes-core/       # Core recipes
│   │   │   ├── recipes-devtools/   # Development tools
│   │   │   ├── classes/            # BitBake classes (.bbclass)
│   │   │   └── lib/                # Python libraries
│   │   ├── oe-init-build-env       # Build environment setup script
│   │   └── scripts/                # Utility scripts
│   └── meta-yocto/                 # Poky reference distro
│       ├── meta-poky/              # Poky-specific layer
│       │   ├── conf/
│       │   │   ├── layer.conf
│       │   │   ├── distro/         # Distro configs (poky.conf)
│       │   │   └── templates/      # Build directory templates
│       │   │       └── default/
│       │   │           ├── bblayers.conf.sample
│       │   │           └── local.conf.sample
│       │   └── recipes-*/
│       └── meta-yocto-bsp/         # BSP layer for reference boards
└── build/                          # Created by oe-init-build-env
    └── conf/
        ├── bblayers.conf           # Layer configuration
        └── local.conf              # Local build settings
```

## Build Environment Initialization

### 1. Setup Command

```bash
TEMPLATECONF=$PWD/layers/meta-yocto/meta-poky/conf/templates/default \
  source ./layers/openembedded-core/oe-init-build-env
```

**What this does:**
1. Sources `oe-init-build-env` from openembedded-core
2. Creates `build/` directory (or custom directory if specified)
3. Generates `build/conf/` with configuration files
4. Uses templates from `TEMPLATECONF` to create `bblayers.conf` and `local.conf`
5. Sets environment variables (`BBPATH`, `PATH`, etc.)
6. Changes current directory to `build/`

### 2. Build Directory Structure

After initialization, the build directory contains:

```
build/
├── conf/
│   ├── local.conf              # User configuration
│   ├── bblayers.conf           # Layer list
│   ├── bblock.conf            # Build locking (optional)
│   └── auto.conf              # Auto-generated (optional)
├── downloads/                  # Downloaded source tarballs (DL_DIR)
├── sstate-cache/              # Shared state cache (SSTATE_DIR)
├── cache/                     # BitBake metadata cache
└── tmp/                       # Build output (TMPDIR)
    ├── buildstats/            # Build statistics
    ├── cache/                 # Per-machine cache
    ├── deploy/                # Final artifacts
    │   ├── images/           # Bootable images, kernels
    │   ├── ipk/              # IPK packages
    │   ├── rpm/              # RPM packages
    │   ├── deb/              # DEB packages
    │   ├── licenses/         # License manifests
    │   └── sdk/              # SDK installers
    ├── stamps/               # Task completion stamps
    ├── log/                  # Build logs
    └── work/                 # Per-recipe work directories
        └── ARCH/RECIPE/VERSION/
            ├── temp/         # Task logs and scripts
            ├── image/        # do_install output
            ├── package/      # Packaging work
            ├── deploy-*/     # Deployment staging
            ├── recipe-sysroot/         # Target dependencies
            ├── recipe-sysroot-native/  # Native build tools
            └── [source]      # Unpacked sources
```

## Configuration Files

### 1. bblayers.conf

**Purpose:** Lists all layers BitBake will use

**Example:**
```python
# POKY_BBLAYERS_CONF_VERSION is increased each time build/conf/bblayers.conf
# changes incompatibly
POKY_BBLAYERS_CONF_VERSION = "2"

BBPATH = "${TOPDIR}"
BBFILES ?= ""

BBLAYERS ?= " \
  /home/user/project/layers/openembedded-core/meta \
  /home/user/project/layers/meta-yocto/meta-poky \
  /home/user/project/layers/meta-yocto/meta-yocto-bsp \
  "
```

**Key Variables:**
- `BBLAYERS` - Space-separated list of absolute paths to layers
- `BBPATH` - Search path for configuration files (usually `${TOPDIR}`)
- `BBFILES` - Recipe file patterns (usually empty here, set per-layer)

**Template Processing:**
- Template: `${TEMPLATECONF}/bblayers.conf.sample`
- `##OEROOT##` placeholders are replaced with `${OEROOT}` during generation
- Paths are made absolute based on layer locations

### 2. local.conf

**Purpose:** User-specific build configuration

**Key Variables:**
```python
# Machine (target hardware)
MACHINE ??= "qemux86-64"

# Package format
PACKAGE_CLASSES ?= "package_rpm"

# Download directory (can be shared)
DL_DIR ?= "${TOPDIR}/downloads"

# Shared state cache (can be shared)
SSTATE_DIR ?= "${TOPDIR}/sstate-cache"

# Build output directory
TMPDIR = "${TOPDIR}/tmp"

# Parallelism
BB_NUMBER_THREADS ?= "8"
PARALLEL_MAKE ?= "-j 8"

# Disk space monitoring
BB_DISKMON_DIRS ??= "\
    STOPTASKS,${TMPDIR},1G,100K \
    STOPTASKS,${DL_DIR},1G,100K \
    STOPTASKS,${SSTATE_DIR},1G,100K \
    STOPTASKS,/tmp,100M,100K \
    HALT,${TMPDIR},100M,1K \
    HALT,${DL_DIR},100M,1K \
    HALT,${SSTATE_DIR},100M,1K \
    HALT,/tmp,10M,1K"

# Package configuration
CONF_VERSION = "2"
```

**Template Processing:**
- Template: `${TEMPLATECONF}/local.conf.sample`
- Contains sensible defaults for common targets
- Users typically modify `MACHINE`, `DL_DIR`, `SSTATE_DIR`

## Layer Structure

### 1. Layer Metadata (layer.conf)

Each layer must have `conf/layer.conf`:

```python
# We have a conf and classes directory, add to BBPATH
BBPATH .= ":${LAYERDIR}"

# We have recipes-* directories, add to BBFILES
BBFILES += "${LAYERDIR}/recipes-*/*/*.bb \
            ${LAYERDIR}/recipes-*/*/*.bbappend"

BBFILE_COLLECTIONS += "core"
BBFILE_PATTERN_core = "^${LAYERDIR}/"
BBFILE_PRIORITY_core = "5"

LAYERVERSION_core = "1"
LAYERSERIES_COMPAT_core = "scarthgap styhead"
```

**Key Variables:**
- `BBPATH` - Adds layer to BitBake search path
- `BBFILES` - Glob patterns for finding recipes
- `BBFILE_COLLECTIONS` - Unique layer identifier
- `BBFILE_PATTERN_<layer>` - Pattern to identify layer files
- `BBFILE_PRIORITY_<layer>` - Layer priority (higher wins)
- `LAYERVERSION_<layer>` - Layer version
- `LAYERSERIES_COMPAT_<layer>` - Compatible Yocto releases
- `LAYERDEPENDS_<layer>` - Required dependent layers

### 2. Recipe Organization

Recipes are organized by category:

```
meta-layer/
├── recipes-core/          # Core system packages
│   ├── base-files/
│   │   └── base-files_3.0.bb
│   ├── busybox/
│   │   ├── busybox_1.36.1.bb
│   │   └── files/
│   │       └── defconfig
│   └── init-scripts/
├── recipes-kernel/        # Kernel recipes
│   └── linux/
│       ├── linux-yocto_6.1.bb
│       └── linux-yocto/
│           └── defconfig
├── recipes-devtools/      # Development tools
│   ├── gcc/
│   ├── binutils/
│   └── cmake/
└── recipes-extended/      # Extended packages
```

**Naming Convention:**
- Recipe files: `<name>_<version>.bb`
- Append files: `<name>_<version>.bbappend` or `<name>_%.bbappend`
- Recipe-specific files go in: `<recipe-name>/<recipe-name>/files/`

### 3. Classes

Reusable build logic in `.bbclass` files:

```
meta-layer/
└── classes/
    ├── autotools.bbclass      # Autotools build
    ├── cmake.bbclass          # CMake build
    ├── kernel.bbclass         # Kernel build
    ├── package.bbclass        # Packaging
    └── image.bbclass          # Image creation
```

Used via `inherit autotools` in recipes.

## BitBake Variables Reference

### Environment Variables

| Variable | Purpose | Example |
|----------|---------|---------|
| `OEROOT` | Root of OE-Core | `/path/to/layers/openembedded-core` |
| `TOPDIR` | Build directory | `/path/to/build` |
| `TMPDIR` | Build output | `${TOPDIR}/tmp` |
| `DL_DIR` | Downloads | `${TOPDIR}/downloads` |
| `SSTATE_DIR` | Shared state | `${TOPDIR}/sstate-cache` |
| `BBPATH` | Config search path | `${TOPDIR}:layers...` |
| `LAYERDIR` | Current layer dir | `/path/to/meta-layer` |

### Layer Discovery

| Variable | Purpose |
|----------|---------|
| `BBLAYERS` | List of all layers (absolute paths) |
| `BBFILES` | Recipe file patterns (globs) |
| `BBFILE_COLLECTIONS` | Layer identifiers |
| `BBFILE_PATTERN_<layer>` | Pattern matching layer files |
| `BBFILE_PRIORITY_<layer>` | Layer precedence |
| `LAYERDEPENDS_<layer>` | Required dependencies |

### Recipe Variables

| Variable | Purpose | Example |
|----------|---------|---------|
| `PN` | Package name | `busybox` |
| `PV` | Package version | `1.36.1` |
| `PR` | Package revision | `r0` |
| `SRC_URI` | Source URLs | `https://...tar.gz` |
| `DEPENDS` | Build dependencies | `zlib openssl` |
| `RDEPENDS` | Runtime dependencies | `libc` |

## How BitBake Discovers Recipes

1. **Read bblayers.conf** → Get `BBLAYERS` list
2. **For each layer in BBLAYERS**:
   - Read `conf/layer.conf`
   - Append to `BBPATH`
   - Collect `BBFILES` patterns
   - Register `BBFILE_COLLECTIONS`
3. **Expand BBFILES patterns** → Get list of all `.bb` and `.bbappend` files
4. **Parse recipes** according to `BBFILE_PRIORITY`
5. **Build dependency graph** from task dependencies

## Implications for Bitzel

### Current Assumptions (to verify)

Bitzel likely assumes:
- Single layer or hardcoded layer paths
- Recipes in fixed locations
- No layer priority handling
- No template-based configuration

### Required Changes

1. **Configuration Parser**
   - Parse `bblayers.conf` to get `BBLAYERS`
   - Parse each `layer.conf` to get `BBFILES`, priorities
   - Support variable expansion (`${LAYERDIR}`, `${TOPDIR}`)
   - Handle glob patterns for recipe discovery

2. **Layer Discovery**
   - Find all layers from `BBLAYERS`
   - Respect `LAYERDEPENDS` for ordering
   - Apply `BBFILE_PRIORITY` for recipe conflicts
   - Support `.bbappend` files

3. **Environment Setup**
   - Support `TEMPLATECONF` for config generation
   - Parse `local.conf` for `DL_DIR`, `TMPDIR`, `MACHINE`
   - Set up proper directory structure
   - Handle `##OEROOT##` substitutions

4. **Recipe Discovery**
   - Walk `recipes-*/*/*.bb` patterns in each layer
   - Merge recipes across layers by priority
   - Apply `.bbappend` files in priority order
   - Support wildcard versions (`%.bbappend`)

5. **Build Output**
   - Organize per modern `tmp/` structure
   - Create work directories: `tmp/work/ARCH/RECIPE/VERSION/`
   - Place artifacts in `tmp/deploy/images/MACHINE/`
   - Generate stamps in `tmp/stamps/`

## Testing Plan

1. **Clone Modern Poky**
   ```bash
   mkdir layers/
   git clone -b scarthgap https://git.openembedded.org/bitbake layers/bitbake
   git clone -b scarthgap https://git.openembedded.org/openembedded-core layers/openembedded-core
   git clone -b scarthgap https://git.yoctoproject.org/meta-yocto layers/meta-yocto
   ```

2. **Initialize Build Environment**
   ```bash
   TEMPLATECONF=$PWD/layers/meta-yocto/meta-poky/conf/templates/default \
     source ./layers/openembedded-core/oe-init-build-env
   ```

3. **Test Bitzel**
   - Parse `build/conf/bblayers.conf`
   - Discover all layers
   - Find all recipes
   - Build simple recipe (busybox)
   - Verify output matches expected structure

## References

- [Yocto Manual Setup](https://docs.yoctoproject.org/dev/dev-manual/poky-manual-setup.html)
- [BitBake Environment Setup](https://docs.yoctoproject.org/bitbake/dev/bitbake-user-manual/bitbake-user-manual-environment-setup.html)
- [Structure Reference](https://docs.yoctoproject.org/ref-manual/structure.html)
- [Layers Documentation](https://docs.yoctoproject.org/dev-manual/layers.html)
- [Variable Reference](https://docs.yoctoproject.org/ref-manual/variables.html)
