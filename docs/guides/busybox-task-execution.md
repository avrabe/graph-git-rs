# Busybox Task Execution Plan

## Overview of First 3 Tasks

From `busybox.inc`, we have these tasks:

1. **do_prepare_config** - Prepares the busybox `.config` file
2. **do_configure** - Runs configuration and saves original config
3. **do_compile** - Compiles busybox binary (possibly split suid/nosuid)

## Task Dependency Chain

```
do_fetch → do_unpack → do_patch → do_prepare_config → do_configure → do_compile
```

**For our 3 tasks:**
```
do_prepare_config
   ↓
do_configure (depends on do_prepare_config)
   ↓
do_compile (depends on do_configure)
```

## Task 1: do_prepare_config

### Dependencies
- **Previous tasks**: `do_patch` (source must be unpacked and patched)
- **Input files**:
  - `${WORKDIR}/defconfig` - Default busybox configuration
  - `${S}/*` - Unpacked busybox source

- **Environment variables**:
  - `WORKDIR` - Work directory
  - `S` - Source directory
  - `configmangle` - Python-generated sed script
  - `DO_IPv4` - Whether IPv4 is enabled
  - `DO_IPv6` - Whether IPv6 is enabled
  - `DEBUG_PREFIX_MAP` - Debug prefix mapping

### What It Does
```bash
# Line 112-134
do_prepare_config () {
    export KCONFIG_NOTIMESTAMP=1

    # Create initial .config from defconfig
    sed -e '/CONFIG_STATIC/d' \
        < ${WORKDIR}/defconfig > ${S}/.config
    echo "# CONFIG_STATIC is not set" >> .config

    # Add markers for configuration
    for i in 'CROSS' 'DISTRO FEATURES'; do echo "### $i"; done >> \
        ${S}/.config

    # Apply configuration mangling (from Python)
    sed -i -e '${configmangle}' ${S}/.config

    # Disable networking applets if no IPv4/IPv6
    if test ${DO_IPv4} -eq 0 && test ${DO_IPv6} -eq 0; then
        mv ${S}/.config ${S}/.config.oe-tmp
        awk 'BEGIN{net=0}
        /^# Networking Utilities/{net=1}
        /^#$/{if(net){net=net+1}}
        {if(net==2&&$0 !~ /^#/&&$1){print("# "$1" is not set")}else{print}}' \
        ${S}/.config.oe-tmp > ${S}/.config
    fi

    # Configure UDHCPC options
    sed -i 's/CONFIG_IFUPDOWN_UDHCPC_CMD_OPTIONS="-R -n"/CONFIG_IFUPDOWN_UDHCPC_CMD_OPTIONS="-R -b"/' ${S}/.config

    # Remove debug prefix mappings
    if [ -n "${DEBUG_PREFIX_MAP}" ]; then
        sed -i 's|${DEBUG_PREFIX_MAP}||g' ${S}/.config
    fi
}
```

### Outputs
- `${S}/.config` - Prepared busybox configuration file

### Sandbox Setup for do_prepare_config

```
sandbox/busybox-do_prepare_config-<uuid>/
  work/
    # Symlinks to previous task outputs
    src/
      busybox-1.35.0/ → hardlink to artifacts/busybox/do_patch-<sig>/src/busybox-1.35.0/
        (contains patched source code)

    # Symlinks to build artifacts
    defconfig → hardlink to artifacts/busybox/do_unpack-<sig>/defconfig

    # Environment (no recipe-sysroot needed - just shell commands)

    # Real directories for outputs
    temp/              # Logs
```

### Execution
```bash
cd /sandbox/.../work/src/busybox-1.35.0
export WORKDIR=/work
export S=/work/src/busybox-1.35.0
export configmangle="..."  # From Python evaluation
export DO_IPv4=1
export DO_IPv6=1
export DEBUG_PREFIX_MAP="..."

# Run task
do_prepare_config

# Collect outputs
# - ${S}/.config → artifacts/busybox/do_prepare_config-<sig>/build/.config
```

## Task 2: do_configure

### Dependencies
- **Previous tasks**: `do_prepare_config`
- **Input files**:
  - `${S}/.config` - From do_prepare_config
  - `${S}/*` - Source code
  - Config fragments from `find_cfgs(d)` Python function

- **Native tools needed**:
  - `merge_config.sh` - Kernel config merge script
  - `cml1_do_configure` - BitBake's kconfig configurator

### What It Does
```bash
# Line 136-145
do_configure () {
    set -x
    do_prepare_config
    merge_config.sh -m .config ${@" ".join(find_cfgs(d))}
    cml1_do_configure

    # Save a copy of .config and autoconf.h.
    cp .config .config.orig
    cp include/autoconf.h include/autoconf.h.orig
}
```

### Outputs
- `${S}/.config` - Final merged configuration
- `${S}/.config.orig` - Backup copy
- `${S}/include/autoconf.h` - Generated C header
- `${S}/include/autoconf.h.orig` - Backup copy

### Sandbox Setup for do_configure

```
sandbox/busybox-do_configure-<uuid>/
  work/
    # Symlinks to previous task outputs
    src/
      busybox-1.35.0/ → hardlink to artifacts/busybox/do_prepare_config-<sig>/build/busybox-1.35.0/
        (includes .config from do_prepare_config)

    # Native tools sysroot
    recipe-sysroot-native/
      usr/
        bin/
          merge_config.sh → hardlink to artifacts/kern-tools-native/do_install-<sig>/sysroot/usr/bin/merge_config.sh
          # Other native tools...

    # Real directories for outputs
    build/             # Build artifacts
    temp/              # Logs
```

### Execution
```bash
cd /work/src/busybox-1.35.0
export PATH=/work/recipe-sysroot-native/usr/bin:$PATH
export S=/work/src/busybox-1.35.0
export B=/work/build

# Run task
do_configure

# Collect outputs
# - ${S}/.config* → artifacts/busybox/do_configure-<sig>/build/
# - ${S}/include/autoconf.h* → artifacts/busybox/do_configure-<sig>/build/include/
```

## Task 3: do_compile

### Dependencies
- **Previous tasks**: `do_configure`
- **Input files**:
  - `${S}/*` - Configured source
  - `${S}/.config.orig` - Original config
  - `${S}/include/autoconf.h.orig` - Original header

- **Dependency sysroots**:
  - `virtual/crypt` (libxcrypt) - Provides crypt functions
  - `kern-tools-native` - Provides build tools

- **Native tools**:
  - Cross-compiler toolchain (gcc, ld, etc.)
  - `oe_runmake` - BitBake's make wrapper

### What It Does
```bash
# Line 147-209
do_compile() {
    unset CFLAGS CPPFLAGS CXXFLAGS LDFLAGS
    export KCONFIG_NOTIMESTAMP=1

    # Restore original config
    cp .config.orig .config
    cp include/autoconf.h.orig include/autoconf.h

    if [ "${BUSYBOX_SPLIT_SUID}" = "1" -a x`grep "CONFIG_FEATURE_INDIVIDUAL=y" .config` = x ]; then
        # Split build: suid and nosuid binaries
        rm -f .config.app.suid .config.app.nosuid .config.disable.apps .config.nonapps

        oe_runmake busybox.cfg.suid
        oe_runmake busybox.cfg.nosuid

        # ... (complex suid/nosuid splitting logic)

        for s in suid nosuid; do
            merge_config.sh -m .config.nonapps .config.app.$s
            oe_runmake busybox_unstripped
            mv busybox_unstripped busybox.$s
            oe_runmake busybox.links
            sort busybox.links > busybox.links.$s
            rm busybox.links
        done

        # Verify sh not in suid binary
        if grep -q -x "/bin/sh" busybox.links.suid; then
            bbfatal "busybox suid binary incorrectly provides /bin/sh"
        fi

        # Cleanup temp files
        rm .config.app.suid .config.app.nosuid .config.disable.apps .config.nonapps
    else
        # Simple build: single binary
        oe_runmake busybox_unstripped
        cp busybox_unstripped busybox
        oe_runmake busybox.links
    fi

    # Restore original config for do_install
    cp .config.orig .config
    cp include/autoconf.h.orig include/autoconf.h
}
```

### Outputs
- `${B}/busybox` or `${B}/busybox.{suid,nosuid}` - Compiled binaries
- `${B}/busybox.links` or `${B}/busybox.links.{suid,nosuid}` - Symlink lists

### Sandbox Setup for do_compile

```
sandbox/busybox-do_compile-<uuid>/
  work/
    # Symlinks to source from do_configure
    src/
      busybox-1.35.0/ → hardlink to artifacts/busybox/do_configure-<sig>/build/
        .config.orig
        include/autoconf.h.orig
        Makefile
        ...

    # Target sysroot with dependencies
    recipe-sysroot/
      usr/
        include/
          crypt.h → hardlink to artifacts/libxcrypt/do_install-<sig>/sysroot/usr/include/crypt.h
        lib/
          libcrypt.so → hardlink to artifacts/libxcrypt/do_install-<sig>/sysroot/usr/lib/libcrypt.so

    # Native toolchain sysroot
    recipe-sysroot-native/
      usr/
        bin/
          x86_64-poky-linux-gcc → hardlink to artifacts/gcc-cross-x86_64/do_install-<sig>/sysroot/usr/bin/...
          x86_64-poky-linux-ld → hardlink to artifacts/binutils-cross-x86_64/do_install-<sig>/sysroot/usr/bin/...
          make → hardlink to artifacts/make-native/do_install-<sig>/sysroot/usr/bin/make

    # Real directories for outputs
    build/             # Compilation outputs
    temp/              # Logs
```

### Execution
```bash
cd /work/src/busybox-1.35.0

# Set up build environment
export S=/work/src/busybox-1.35.0
export B=/work/build
export WORKDIR=/work

# Toolchain environment
export CC="x86_64-poky-linux-gcc --sysroot=/work/recipe-sysroot"
export LD="x86_64-poky-linux-ld --sysroot=/work/recipe-sysroot"
export CFLAGS="-O2 -pipe"
export LDFLAGS="-Wl,-O1 -Wl,--hash-style=gnu"

export PATH=/work/recipe-sysroot-native/usr/bin:$PATH
export STAGING_INCDIR=/work/recipe-sysroot/usr/include
export STAGING_LIBDIR=/work/recipe-sysroot/usr/lib

export EXTRA_OEMAKE="CC='${CC}' LD='${LD}' V=1 ARCH=x86_64 CROSS_COMPILE=x86_64-poky-linux- SKIP_STRIP=y"

export BUSYBOX_SPLIT_SUID=1

# Run task
do_compile

# Collect outputs
# - ${B}/busybox* → artifacts/busybox/do_compile-<sig>/build/
# - ${B}/busybox.links* → artifacts/busybox/do_compile-<sig>/build/
# - logs → artifacts/busybox/do_compile-<sig>/logs/
```

## Critical Observations

### 1. **Task Dependencies are Clear**

Each task depends on outputs from previous task:
- `do_prepare_config` needs patched source
- `do_configure` needs `.config` from do_prepare_config
- `do_compile` needs configured source with autoconf.h

### 2. **Different Sysroot Needs**

- **do_prepare_config**: No sysroot (just shell commands)
- **do_configure**: Native tools only (merge_config.sh)
- **do_compile**: Both native tools (gcc) AND target sysroot (libcrypt)

### 3. **Hardlink Assembly Required**

For do_compile sandbox:
```bash
# Assemble recipe-sysroot from dependencies
copyhardlinktree artifacts/libxcrypt/do_install-<sig>/sysroot/ recipe-sysroot/

# Assemble recipe-sysroot-native from native tools
copyhardlinktree artifacts/gcc-cross-x86_64/do_install-<sig>/sysroot/ recipe-sysroot-native/
copyhardlinktree artifacts/binutils-cross-x86_64/do_install-<sig>/sysroot/ recipe-sysroot-native/
copyhardlinktree artifacts/make-native/do_install-<sig>/sysroot/ recipe-sysroot-native/
```

All files from all dependencies in the same directory tree!

### 4. **Environment Variables are Critical**

Tasks expect specific variables:
- `S`, `B`, `WORKDIR` - Directory structure
- `CC`, `LD`, `CFLAGS`, `LDFLAGS` - Toolchain
- `PATH` - Must find native tools
- `STAGING_INCDIR`, `STAGING_LIBDIR` - Sysroot paths

### 5. **Execution Log Structure**

Each task execution would log:
```
artifacts/busybox/do_compile-<sig>/
  logs/
    log.do_compile          # stdout
    log.do_compile.err      # stderr
    run.do_compile          # Actual script executed
  metadata.json             # Task info, duration, exit code
  build/                    # Task outputs
    busybox.suid
    busybox.nosuid
    busybox.links.suid
    busybox.links.nosuid
```

## Next Steps to Execute These Tasks

1. **Parse Dependencies**: Extract DEPENDS from recipe
   - `virtual/crypt` → resolve to `libxcrypt`
   - `kern-tools-native` → native package

2. **Build Dependency Tasks First**:
   - Execute `libxcrypt:do_install` → store sysroot outputs
   - Execute `gcc-cross:do_install` → store native tools
   - Execute `busybox:do_patch` → store patched source

3. **Create Sandbox with Hardlinks**:
   - Hardlink previous task outputs
   - Hardlink dependency sysroots
   - Set up environment variables

4. **Execute Task**:
   - Run bash script in sandbox
   - Capture stdout/stderr
   - Check exit code

5. **Collect Outputs**:
   - Copy outputs to artifact cache
   - Generate manifest of files
   - Update task signature cache

This is the complete execution model for real BitBake tasks!
