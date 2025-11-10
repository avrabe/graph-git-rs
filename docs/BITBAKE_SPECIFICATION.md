# BitBake File Format and Structure Specification

## Overview

This document provides a comprehensive specification of the BitBake metadata system based on official Yocto Project and BitBake documentation. This serves as the reference for implementing a full-featured static analyzer for BitBake files.

## File Types

### 1. Recipe Files (.bb)

**Purpose**: Define how to build a specific software package

**Location**: `recipes-*/packagename/packagename_version.bb`

**Contains**:
- Package metadata (PN, PV, PR, LICENSE, DESCRIPTION, AUTHOR, HOMEPAGE)
- Source locations (SRC_URI)
- Source revisions (SRCREV, SRCREV_FORMAT)
- Dependencies (DEPENDS, RDEPENDS, RRECOMMENDS, RPROVIDES, RCONFLICTS, RREPLACES)
- Build configuration
- Task definitions (do_fetch, do_unpack, do_patch, do_configure, do_compile, do_install, do_package)
- Class inheritance (inherit)
- Include directives (include, require)
- Variable assignments and operations

**Example**:
```bitbake
SUMMARY = "My Application"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://LICENSE;md5=..."

SRC_URI = "git://github.com/user/repo.git;protocol=https;branch=master"
SRCREV = "abc123def456"

DEPENDS = "openssl zlib"
RDEPENDS:${PN} = "bash perl"

inherit cmake

do_install() {
    install -d ${D}${bindir}
    install -m 0755 myapp ${D}${bindir}
}
```

### 2. Recipe Append Files (.bbappend)

**Purpose**: Extend or override existing recipes without modifying them

**Location**: Can be in any layer, matches recipe by name

**Naming**: Must match recipe base name with optional `%` wildcard
- `busybox_1.21.1.bbappend` - matches exactly `busybox_1.21.1.bb`
- `busybox_1.21.%.bbappend` - matches `busybox_1.21.x.bb` (any x)
- `busybox_%.bbappend` - matches any version

**Constraints**:
- Must have corresponding .bb file
- Parsed after base recipe
- Can add/modify but not remove from base recipe

**Example**:
```bitbake
# In meta-mylayer/recipes-core/busybox/busybox_%.bbappend
FILESEXTRAPATHS:prepend := "${THISDIR}/${PN}:"
SRC_URI += "file://custom-config.patch"
```

### 3. Class Files (.bbclass)

**Purpose**: Reusable build logic shared across multiple recipes

**Location**: Three types with different scopes:
- `classes-global/` - Must be inherited globally via INHERIT
- `classes-recipe/` - Can only be inherited in recipes
- `classes/` - Can be used in both contexts

**Special Classes**:
- `base.bbclass` - Always included automatically

**Common Classes**:
- `autotools.bbclass` - GNU autotools support
- `cmake.bbclass` - CMake build system
- `cargo.bbclass` - Rust cargo support
- `systemd.bbclass` - systemd service integration
- `kernel.bbclass` - Linux kernel recipes
- `native.bbclass` - Build tools for host

**Usage**:
```bitbake
inherit cmake systemd
```

### 4. Configuration Files (.conf)

**Purpose**: Define build configuration, machines, distributions

**Location**: Various `conf/` directories

**Types**:
- `conf/layer.conf` - Layer configuration
- `conf/machine/*.conf` - Machine definitions
- `conf/distro/*.conf` - Distribution policies
- `conf/local.conf` - Local build settings
- `conf/bitbake.conf` - Core BitBake configuration

**Restrictions**:
- Only variable definitions allowed
- Only include directives allowed
- No functions, no tasks

**Example `conf/layer.conf`**:
```bitbake
# Layer identification
BBPATH .= ":${LAYERDIR}"
BBFILES += "${LAYERDIR}/recipes-*/*/*.bb \
            ${LAYERDIR}/recipes-*/*/*.bbappend"

BBFILE_COLLECTIONS += "mylayer"
BBFILE_PATTERN_mylayer = "^${LAYERDIR}/"
BBFILE_PRIORITY_mylayer = "6"
LAYERSERIES_COMPAT_mylayer = "kirkstone langdale"
```

**Example `conf/machine/mymachine.conf`**:
```bitbake
DEFAULTTUNE = "cortexa9"
include conf/machine/include/arm/armv7a/tune-cortexa9.inc

KERNEL_IMAGETYPE = "zImage"
MACHINE_FEATURES = "usbhost wifi bluetooth"
```

### 5. Include Files (.inc)

**Purpose**: Shared recipe fragments, common variable definitions

**Location**: Usually alongside recipes or in `conf/`

**Usage**: Included via `include` or `require`

**Example `common.inc`**:
```bitbake
HOMEPAGE = "https://example.com"
BUGTRACKER = "https://example.com/bugs"

COMMON_SRC_URI = "file://common.patch"
```

## Layer Structure

Standard Yocto layer directory structure:

```
meta-mylayer/
├── conf/
│   ├── layer.conf              # Layer configuration (required)
│   ├── machine/                # Machine definitions
│   │   └── mymachine.conf
│   └── distro/                 # Distribution configs
│       └── mydistro.conf
├── classes/                    # Shared build logic
│   └── myclass.bbclass
├── classes-global/             # Global-only classes
│   └── myglobal.bbclass
├── classes-recipe/             # Recipe-only classes
│   └── myrecipe.bbclass
├── recipes-bsp/                # Board Support Package recipes
│   ├── bootloader/
│   │   └── u-boot_%.bbappend
│   └── device-tree/
│       └── device-tree.bb
├── recipes-kernel/             # Kernel recipes
│   └── linux/
│       ├── linux-yocto_%.bbappend
│       └── linux-yocto/
│           └── defconfig
├── recipes-core/               # Core system recipes
│   └── busybox/
│       ├── busybox_%.bbappend
│       └── busybox/
│           └── custom.cfg
├── recipes-extended/           # Extended functionality
├── recipes-graphics/           # Graphics stack
├── recipes-connectivity/       # Network/connectivity
├── recipes-support/            # Support libraries
└── recipes-myapp/              # Custom application category
    └── myapp/
        ├── myapp_1.0.bb
        ├── myapp.inc
        └── myapp/
            ├── 0001-fix.patch
            └── myapp.service
```

## Metadata Syntax

### Variable Assignment Operators

| Operator | Name | Behavior | Example |
|----------|------|----------|---------|
| `=` | Assignment | Deferred expansion | `FOO = "bar"` |
| `:=` | Immediate | Immediate expansion | `FOO := "${BAR}"` |
| `?=` | Soft Default | Set if undefined | `FOO ?= "default"` |
| `??=` | Weak Default | Overridable default | `FOO ??= "weak"` |
| `+=` | Append with space | Add to end | `FOO += "more"` |
| `=+` | Prepend with space | Add to start | `FOO =+ "first"` |
| `.=` | Append no space | Concatenate end | `FOO .= "more"` |
| `=.` | Prepend no space | Concatenate start | `FOO =. "first"` |

### Override Syntax

Overrides provide conditional behavior based on context:

**Format**: `VARIABLE:override = "value"`

**Common override types**:
- **Machine**: `:machine-name` (e.g., `:qemuarm`, `:raspberrypi4`)
- **Architecture**: `:arm`, `:x86`, `:mips`
- **Distribution**: `:poky`, `:mydistro`
- **Class**: `:class-target`, `:class-native`, `:class-nativesdk`
- **Recipe-specific**: `:pn-recipe-name`
- **Task-specific**: `:task-configure`, `:task-compile`
- **Custom**: Any value in OVERRIDES variable

**Override operations**:
- `:append` - Append value (applied during expansion)
- `:prepend` - Prepend value (applied during expansion)
- `:remove` - Remove all occurrences (applied last)

**Examples**:
```bitbake
# Machine-specific
KERNEL_IMAGETYPE = "zImage"
KERNEL_IMAGETYPE:qemux86 = "bzImage"

# Multiple overrides (right-most has priority)
SRC_URI:append = " file://common.patch"
SRC_URI:append:qemuarm = " file://qemuarm.patch"

# Task-specific
DEPENDS:task-configure = "cmake-native"

# Remove operation
PACKAGECONFIG:remove = "bluetooth"
```

### Include and Inheritance

#### include
- Non-fatal if file not found
- Syntax: `include filename.inc`
- Variable expansion: `include ${BPN}-crates.inc`

#### require
- Fatal if file not found
- Syntax: `require filename.inc`

#### inherit
- Load bbclass file
- Syntax: `inherit cmake autotools`
- Multiple classes: space-separated

#### inherit_defer
- Defer class inheritance until after recipe parsing
- Syntax: `inherit_defer native`

### Variable Expansion

**Syntax**: `${VARIABLE}`

**Inline Python**: `${@python_expression}`

**Examples**:
```bitbake
S = "${WORKDIR}/${BPN}-${PV}"
DEPENDS = "virtual/${TARGET_PREFIX}gcc"
PV = "${@bb.utils.get_file_mtime('version.txt')}"
```

### SRC_URI Syntax

#### Basic Format
```bitbake
SRC_URI = "scheme://authority/path;parameter1=value1;parameter2=value2"
```

#### Supported Schemes

| Scheme | Purpose | Example |
|--------|---------|---------|
| `file://` | Local files | `file://0001-fix.patch` |
| `http://`, `https://` | HTTP(S) download | `https://example.com/file.tar.gz` |
| `ftp://` | FTP download | `ftp://ftp.example.com/file.tar.gz` |
| `git://` | Git repository | `git://github.com/user/repo.git` |
| `gitsm://` | Git with submodules | `gitsm://github.com/user/repo.git` |
| `svn://` | Subversion | `svn://svn.example.com/repo` |
| `cvs://` | CVS | `cvs://cvs.example.com/module` |
| `p4://` | Perforce | `p4://perforce:1666/depot/path` |
| `repo://` | Google Repo | `repo://manifest-url` |
| `crate://` | Rust crates | `crate://crates.io/name` |
| `npm://` | NPM packages | `npm://registry.npmjs.org;package=name` |
| `az://` | Azure Storage | `az://account/container/blob` |
| `gs://` | Google Cloud Storage | `gs://bucket/object` |

#### Git URL Parameters

**Required unless nobranch=1**:
- `branch` - Branch to checkout

**Optional**:
- `protocol` - Transport protocol: `git`, `http`, `https`, `ssh`, `rsync`, `file`
- `tag` - Git tag to fetch
- `rev` - Specific commit SHA
- `nobranch` - Set to `1` to skip branch validation
- `nocheckout` - Set to `1` to skip working tree checkout
- `subpath` - Limit checkout to specific subdirectory
- `destsuffix` - Destination directory (default: `git/`)
- `bareclone` - Bare clone (no working tree)
- `rebaseable` - Set to `1` if upstream may rebase
- `usehead` - Use HEAD instead of SRCREV

**Git URL Examples**:
```bitbake
SRC_URI = "git://github.com/user/repo.git;protocol=https;branch=master"
SRC_URI = "git://git.kernel.org/linux.git;protocol=https;branch=stable;tag=v5.15"
SRC_URI = "git://internal.com/repo.git;protocol=ssh;branch=develop;destsuffix=src"
```

#### Common URL Parameters (All Schemes)

- `name` - Identifier for checksums: `name=foo` → `SRC_URI[foo.sha256sum]`
- `unpack` - Control extraction: `0`=don't extract, `1`=extract (default varies by type)
- `subdir` - Unpack to subdirectory: `subdir=sources`
- `downloadfilename` - Rename downloaded file
- `striplevel` - Strip path components during extraction (default: 1)

#### Checksums

Three methods to specify checksums:

**Method 1: Variable flags (anonymous URL)**
```bitbake
SRC_URI = "https://example.com/file.tar.gz"
SRC_URI[md5sum] = "..."
SRC_URI[sha256sum] = "..."
```

**Method 2: Named URLs**
```bitbake
SRC_URI = "https://example.com/file.tar.gz;name=source"
SRC_URI[source.md5sum] = "..."
SRC_URI[source.sha256sum] = "..."
```

**Method 3: URL parameters**
```bitbake
SRC_URI = "https://example.com/file.tar.gz;md5sum=...;sha256sum=..."
```

#### Multiple SRC_URI Entries

```bitbake
SRC_URI = "git://github.com/user/repo.git;protocol=https;branch=master \
           file://0001-fix-build.patch \
           file://custom-config.cfg \
          "

# Or with append:
SRC_URI = "git://github.com/user/repo.git;protocol=https;branch=master"
SRC_URI += "file://0001-fix-build.patch"
SRC_URI += "file://0002-add-feature.patch"
```

### SRCREV Syntax

Specifies git commit to use:

```bitbake
# Single repository
SRCREV = "abc123def456..."
SRCREV = "${AUTOREV}"  # Always use latest

# Multiple repositories (named)
SRC_URI = "git://repo1.git;name=repo1;branch=main \
           git://repo2.git;name=repo2;branch=master"
SRCREV_repo1 = "abc123..."
SRCREV_repo2 = "def456..."
SRCREV_FORMAT = "repo1_repo2"
```

### Dependencies

#### Build-time Dependencies
```bitbake
DEPENDS = "openssl zlib cmake-native"
```

#### Runtime Dependencies
```bitbake
RDEPENDS:${PN} = "bash perl python3"
RDEPENDS:${PN}-dev = "${PN}"
```

#### Other Dependency Variables
```bitbake
RRECOMMENDS:${PN} = "optional-package"  # Recommended runtime deps
RSUGGESTS:${PN} = "nice-to-have"        # Suggested runtime deps
RPROVIDES:${PN} = "virtual/editor"      # Virtual packages provided
RCONFLICTS:${PN} = "conflicting-pkg"    # Conflicting packages
RREPLACES:${PN} = "old-package-name"    # Packages replaced
```

### Variable Flags

Access metadata about variables:

```bitbake
FOO[doc] = "Documentation string"
FOO[vardeps] = "OTHER_VAR"
FOO[vardepsexclude] = "DATE TIME"

# Task flags
do_compile[depends] = "other-recipe:do_compile"
do_install[nostamp] = "1"
do_fetch[network] = "1"
```

### Functions

#### Shell Functions
```bitbake
do_compile() {
    oe_runmake
}

my_helper() {
    echo "Helper function"
}
```

#### Python Functions
```bitbake
python do_custom_task() {
    bb.note("Doing custom task")
    d.setVar('FOO', 'bar')
}

def my_python_helper(d):
    return d.getVar('PV')
```

#### Anonymous Python (runs during parsing)
```bitbake
python __anonymous() {
    if d.getVar('SPECIAL_FEATURE') == '1':
        d.appendVar('DEPENDS', ' special-lib')
}
```

### Conditional Syntax

#### Python conditionals in inline expansion
```bitbake
FOO = "${@'yes' if d.getVar('BAR') == '1' else 'no'}"
```

#### bb.utils functions
```bitbake
PACKAGECONFIG = "${@bb.utils.contains('DISTRO_FEATURES', 'bluetooth', 'bluez', '', d)}"
```

## Parsing Order and Resolution

### Layer Priority

1. Layers are processed in BBLAYERS order
2. Within same priority, later layers override earlier
3. BBFILE_PRIORITY determines layer precedence

### Recipe Resolution

1. Base recipe (.bb) parsed first
2. Append files (.bbappend) applied in BBFILES order
3. Classes inherited during parsing
4. Include files processed inline

### Variable Expansion Order

1. Immediate operators (`:=`, `+=`, `=+`, `.=`, `=.`) applied immediately
2. Override-style operations (`:append`, `:prepend`, `:remove`) applied during variable expansion
3. OVERRIDES applied right-to-left (right-most wins)

### Example Resolution

```bitbake
# base.bb
FOO = "base"
FOO:append = " common"

# layer1/recipe.bbappend
FOO += "layer1"
FOO:append:machine = " machine-specific"

# Final value (assuming machine override active):
# "base layer1 common machine-specific"
```

## Best Practices for Static Analysis

### What Can Be Analyzed Statically

1. ✅ File discovery (.bb, .bbappend, .conf, .inc, .bbclass)
2. ✅ Variable assignments (all operators)
3. ✅ SRC_URI extraction and parsing
4. ✅ Dependency extraction (DEPENDS, RDEPENDS, etc.)
5. ✅ Include/require/inherit statements
6. ✅ Override syntax parsing
7. ✅ Basic variable substitution
8. ✅ Function detection (shell and Python)
9. ✅ Checksum extraction

### What Requires BitBake Runtime

1. ❌ Full variable expansion with OVERRIDES
2. ❌ Python function execution
3. ❌ Anonymous Python results
4. ❌ Dynamic variable creation
5. ❌ Class task modifications
6. ❌ Conditional package configurations
7. ❌ Final resolved values with all overrides applied

### Recommended Static Analysis Approach

1. **Parse AST** - Use tree-sitter for syntactic analysis
2. **Extract literals** - Capture all literal values
3. **Track operations** - Record all assignment operators
4. **Follow includes** - Recursively parse included files
5. **Index classes** - Map available bbclass files
6. **Parse URIs** - Extract SRC_URI components
7. **Build dependency graph** - Map DEPENDS/RDEPENDS relationships
8. **Note overrides** - Record but don't resolve override syntax
9. **Flag dynamic content** - Mark variables with Python expansion

## References

- [BitBake User Manual](https://docs.yoctoproject.org/bitbake/)
- [Yocto Project Reference Manual](https://docs.yoctoproject.org/ref-manual/)
- [BitBake GitHub Repository](https://github.com/openembedded/bitbake)
- [Yocto Project Layers](https://layers.openembedded.org/)
