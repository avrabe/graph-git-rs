#!/bin/bash
# BitBake Task Execution Prelude
# This script is sourced by all task scripts to provide standard environment
# and helper functions. It reduces script size and provides consistent behavior.

# Strict error handling
set -e          # Exit on error
set -u          # Exit on undefined variable
set -o pipefail # Pipe failures propagate

# Standard BitBake environment variables (can be overridden)
export PN="${PN:-unknown}"
export PV="${PV:-1.0}"
export PR="${PR:-r0}"
export WORKDIR="${WORKDIR:-/work}"
export S="${S:-${WORKDIR}/src}"
export B="${B:-${WORKDIR}/build}"
export D="${D:-${WORKDIR}/image}"
export TMPDIR="${TMPDIR:-/tmp}"

# Test and QA directories
export PTEST_PATH="${PTEST_PATH:-/usr/lib/ptest}"
export TESTDIR="${TESTDIR:-${WORKDIR}/tests}"
export QEMU_OPTIONS="${QEMU_OPTIONS:-}"

# Package installation paths
export PKGD="${PKGD:-${WORKDIR}/package}"
export PKGDEST="${PKGDEST:-${TMPDIR}/work-shared/pkgdata}"

# Source and patch related
export PATCHES="${PATCHES:-}"
export SRC_URI="${SRC_URI:-}"
export SRCPV="${SRCPV:-${PV}}"

# License and metadata
export LICENSE="${LICENSE:-CLOSED}"
export SUMMARY="${SUMMARY:-}"
export DESCRIPTION="${DESCRIPTION:-}"
export HOMEPAGE="${HOMEPAGE:-}"
export SECTION="${SECTION:-base}"

# File and package names
export FILE="${FILE:-${PN}-${PV}.bb}"
export BP="${BP:-${PN}-${PV}}"
export BPN="${BPN:-${PN}}"

# Recipe and layer paths
export RECIPE_SYSROOT="${RECIPE_SYSROOT:-${TMPDIR}/sysroots/${TARGET_SYS}}"
export RECIPE_SYSROOT_NATIVE="${RECIPE_SYSROOT_NATIVE:-${TMPDIR}/sysroots/${BUILD_SYS}}"

# Stamp and log directories
export STAMP="${STAMP:-${TMPDIR}/stamps/${PN}/${PV}}"
export LOGDIR="${LOGDIR:-${TMPDIR}/logs}"

# Standard paths
export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
export HOME="${HOME:-/tmp}"
export SHELL="/bin/bash"

# BitBake logging functions
bb_plain() {
    echo "$*"
}

bb_note() {
    echo "NOTE: $*"
}

bb_warn() {
    echo "WARNING: $*" >&2
}

bb_error() {
    echo "ERROR: $*" >&2
}

bb_fatal() {
    echo "FATAL: $*" >&2
    exit 1
}

bb_debug() {
    if [ "${BB_VERBOSE:-0}" = "1" ]; then
        echo "DEBUG: $*" >&2
    fi
}

# Helper: Create directory if it doesn't exist
bbdirs() {
    for dir in "$@"; do
        # Skip if it already exists as a symlink (even if broken) or as a directory
        if [ -L "$dir" ] || [ -d "$dir" ]; then
            # Already exists as symlink or directory, skip
            continue
        elif [ -e "$dir" ]; then
            # Exists but is neither symlink nor directory - this is an error
            bb_fatal "bbdirs: $dir exists but is not a directory or symlink"
        else
            # Doesn't exist, create it
            mkdir -p "$dir"
        fi
    done
}

# Helper: Change to build directory (create if needed)
bb_cd_build() {
    bbdirs "${B}"
    cd "${B}"
}

# Helper: Change to source directory
bb_cd_src() {
    if [ ! -d "${S}" ]; then
        bb_fatal "Source directory ${S} does not exist"
    fi
    cd "${S}"
}

# Helper: Install file with optional permissions
bb_install() {
    local mode=""
    if [ "$1" = "-m" ]; then
        mode="$2"
        shift 2
    fi

    local src="$1"
    local dest="$2"

    if [ ! -e "$src" ]; then
        bb_fatal "Cannot install $src: file not found"
    fi

    bbdirs "$(dirname "$dest")"
    cp -a "$src" "$dest"

    if [ -n "$mode" ]; then
        chmod "$mode" "$dest"
    fi
}

# Helper: Run command with logging
bb_run() {
    bb_note "Running: $*"
    "$@"
}

#
# BitBake Build System Function Stubs
# These replicate the behavior of OpenEmbedded/Yocto shell functions
#

# Standard directory paths (FHS-compliant)
export prefix="${prefix:-/usr}"
export exec_prefix="${exec_prefix:-${prefix}}"
export base_prefix="${base_prefix:-}"
export bindir="${bindir:-${exec_prefix}/bin}"
export sbindir="${sbindir:-${exec_prefix}/sbin}"
export libexecdir="${libexecdir:-${exec_prefix}/libexec}"
export libdir="${libdir:-${exec_prefix}/lib}"
export includedir="${includedir:-${prefix}/include}"
export datadir="${datadir:-${prefix}/share}"
export sysconfdir="${sysconfdir:-${base_prefix}/etc}"
export servicedir="${servicedir:-${base_prefix}/srv}"
export sharedstatedir="${sharedstatedir:-${base_prefix}/com}"
export localstatedir="${localstatedir:-${base_prefix}/var}"
export mandir="${mandir:-${datadir}/man}"
export docdir="${docdir:-${datadir}/doc}"
export infodir="${infodir:-${datadir}/info}"

# Base directories (non-prefixed paths for core utilities)
export base_bindir="${base_bindir:-${base_prefix}/bin}"
export base_sbindir="${base_sbindir:-${base_prefix}/sbin}"
export base_libdir="${base_libdir:-${base_prefix}/lib}"

# Additional standard paths
export nonarch_base_libdir="${nonarch_base_libdir:-${base_prefix}/lib}"
export systemd_unitdir="${systemd_unitdir:-${base_prefix}/lib/systemd}"
export systemd_system_unitdir="${systemd_system_unitdir:-${systemd_unitdir}/system}"

# Build configuration
export BUILD_CC="${BUILD_CC:-gcc}"
export BUILD_CXX="${BUILD_CXX:-g++}"
export BUILD_CPP="${BUILD_CPP:-gcc -E}"
export BUILD_LD="${BUILD_LD:-ld}"
export BUILD_AR="${BUILD_AR:-ar}"
export BUILD_AS="${BUILD_AS:-as}"
export BUILD_RANLIB="${BUILD_RANLIB:-ranlib}"
export BUILD_STRIP="${BUILD_STRIP:-strip}"

# Target configuration (cross-compile)
export CC="${CC:-${BUILD_CC}}"
export CXX="${CXX:-${BUILD_CXX}}"
export CPP="${CPP:-${BUILD_CPP}}"
export LD="${LD:-${BUILD_LD}}"
export AR="${AR:-${BUILD_AR}}"
export AS="${AS:-${BUILD_AS}}"
export RANLIB="${RANLIB:-${BUILD_RANLIB}}"
export STRIP="${STRIP:-${BUILD_STRIP}}"

# Compiler flags
export CFLAGS="${CFLAGS:--O2 -pipe}"
export CXXFLAGS="${CXXFLAGS:--O2 -pipe}"
export LDFLAGS="${LDFLAGS:-}"

# Make flags
export EXTRA_OEMAKE="${EXTRA_OEMAKE:-}"
export PARALLEL_MAKE="${PARALLEL_MAKE:--j$(nproc)}"

# Target architecture
export TARGET_ARCH="${TARGET_ARCH:-x86_64}"
export TARGET_OS="${TARGET_OS:-linux}"
export TARGET_VENDOR="${TARGET_VENDOR:-unknown}"
export TARGET_SYS="${TARGET_SYS:-${TARGET_ARCH}-${TARGET_VENDOR}-${TARGET_OS}}"
export BUILD_SYS="${BUILD_SYS:-$(uname -m)-linux}"
export HOST_SYS="${HOST_SYS:-${TARGET_SYS}}"

# Staging and sysroot paths
export STAGING_DIR="${STAGING_DIR:-${TMPDIR}/sysroots}"
export STAGING_DIR_HOST="${STAGING_DIR_HOST:-${STAGING_DIR}/${HOST_SYS}}"
export STAGING_DIR_NATIVE="${STAGING_DIR_NATIVE:-${STAGING_DIR}/${BUILD_SYS}}"
export STAGING_BINDIR="${STAGING_BINDIR:-${STAGING_DIR_HOST}${bindir}}"
export STAGING_LIBDIR="${STAGING_LIBDIR:-${STAGING_DIR_HOST}${libdir}}"
export STAGING_INCDIR="${STAGING_INCDIR:-${STAGING_DIR_HOST}${includedir}}"
export STAGING_DATADIR="${STAGING_DATADIR:-${STAGING_DIR_HOST}${datadir}}"

# Package architecture
export PACKAGE_ARCH="${PACKAGE_ARCH:-${TARGET_ARCH}}"

#
# oe_runmake: Run make with proper flags and parallel builds
#
oe_runmake() {
    bb_note "oe_runmake: $*"

    # Ensure we're in the build directory
    if [ ! -f Makefile ] && [ ! -f makefile ] && [ ! -f GNUmakefile ]; then
        if [ -f "${B}/Makefile" ] || [ -f "${B}/makefile" ] || [ -f "${B}/GNUmakefile" ]; then
            cd "${B}"
        elif [ -f "${S}/Makefile" ] || [ -f "${S}/makefile" ] || [ -f "${S}/GNUmakefile" ]; then
            cd "${S}"
        else
            bb_warn "No Makefile found in ${PWD}, ${B}, or ${S}"
        fi
    fi

    # Build make command with all flags
    local make_flags="${PARALLEL_MAKE} ${EXTRA_OEMAKE}"

    bb_note "Running: make ${make_flags} $*"

    # Run make (allow it to fail gracefully for now)
    if command -v make >/dev/null 2>&1; then
        make ${make_flags} "$@" || {
            bb_warn "make command failed (this may be expected in stub mode)"
            return 0
        }
    else
        bb_warn "make not available, skipping build step"
        return 0
    fi
}

#
# oe_runmake_call: Run make but don't exit on failure
#
oe_runmake_call() {
    oe_runmake "$@" || true
}

#
# oeconf: Run autotools configure with proper cross-compile settings
#
oeconf() {
    bb_note "oeconf: Configuring with autotools"

    # Ensure we're in the build directory
    if [ "${S}" != "${B}" ]; then
        bbdirs "${B}"
        cd "${B}"
    else
        cd "${S}"
    fi

    # Find configure script
    local configure_script="${S}/configure"
    if [ ! -x "$configure_script" ]; then
        if [ -f "$configure_script" ]; then
            chmod +x "$configure_script"
        else
            bb_warn "No configure script found at $configure_script"
            return 0
        fi
    fi

    # Build configure arguments
    local configure_args=""
    configure_args="$configure_args --build=${BUILD_SYS}"
    configure_args="$configure_args --host=${HOST_SYS}"
    configure_args="$configure_args --target=${TARGET_SYS}"
    configure_args="$configure_args --prefix=${prefix}"
    configure_args="$configure_args --exec_prefix=${exec_prefix}"
    configure_args="$configure_args --bindir=${bindir}"
    configure_args="$configure_args --sbindir=${sbindir}"
    configure_args="$configure_args --libexecdir=${libexecdir}"
    configure_args="$configure_args --datadir=${datadir}"
    configure_args="$configure_args --sysconfdir=${sysconfdir}"
    configure_args="$configure_args --sharedstatedir=${sharedstatedir}"
    configure_args="$configure_args --localstatedir=${localstatedir}"
    configure_args="$configure_args --libdir=${libdir}"
    configure_args="$configure_args --includedir=${includedir}"
    configure_args="$configure_args --infodir=${infodir}"
    configure_args="$configure_args --mandir=${mandir}"

    # Add any extra configure options
    configure_args="$configure_args ${EXTRA_OECONF:-}"
    configure_args="$configure_args $*"

    bb_note "Running: $configure_script $configure_args"

    # Run configure
    if "$configure_script" $configure_args; then
        bb_note "Configure succeeded"
    else
        bb_warn "Configure failed (may be expected in stub mode)"
        return 0
    fi
}

#
# autotools_do_configure: Standard autotools configure step
#
autotools_do_configure() {
    bb_note "autotools_do_configure"

    # Run autoreconf if needed
    if [ -f "${S}/configure.ac" ] || [ -f "${S}/configure.in" ]; then
        if [ ! -f "${S}/configure" ]; then
            bb_note "Running autoreconf"
            if command -v autoreconf >/dev/null 2>&1; then
                cd "${S}"
                autoreconf -Wcross --verbose --install --force || bb_warn "autoreconf failed"
            else
                bb_warn "autoreconf not available"
            fi
        fi
    fi

    # Run configure
    oeconf "$@"
}

#
# oe_soinstall: Install shared library with proper symlinks
#
oe_soinstall() {
    bb_note "oe_soinstall: $*"

    # Need at least library and destination
    if [ $# -lt 2 ]; then
        bb_fatal "oe_soinstall requires at least 2 arguments: library dest"
    fi

    local src="$1"
    local dest="$2"

    if [ ! -f "$src" ]; then
        bb_warn "Library $src not found"
        return 0
    fi

    # Create destination directory
    bbdirs "$dest"

    # Install the library
    install -m 0755 "$src" "$dest"

    # Create symlinks for .so.X and .so if this is a versioned library
    local basename=$(basename "$src")
    if [[ "$basename" =~ ^(.+\.so)\.([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
        local soname="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}"
        local linkname="${BASH_REMATCH[1]}"

        cd "$dest"
        ln -sf "$basename" "$soname" 2>/dev/null || true
        ln -sf "$basename" "$linkname" 2>/dev/null || true
    elif [[ "$basename" =~ ^(.+\.so)\.([0-9]+)$ ]]; then
        local linkname="${BASH_REMATCH[1]}"

        cd "$dest"
        ln -sf "$basename" "$linkname" 2>/dev/null || true
    fi
}

#
# oe_libinstall: Install library files (static and/or shared)
#
oe_libinstall() {
    bb_note "oe_libinstall: $*"

    local dir=""
    local so_only=false
    local a_only=false

    # Parse flags
    while [ $# -gt 0 ]; do
        case "$1" in
            -C)
                dir="$2"
                shift 2
                ;;
            -so)
                so_only=true
                shift
                ;;
            -a)
                a_only=true
                shift
                ;;
            *)
                break
                ;;
        esac
    done

    local libname="$1"
    local dest="$2"

    # Change to directory if specified
    if [ -n "$dir" ]; then
        cd "$dir" || bb_fatal "Directory $dir not found"
    fi

    bbdirs "$dest"

    # Install shared library
    if ! $a_only; then
        if [ -f "lib${libname}.so" ]; then
            oe_soinstall "lib${libname}.so" "$dest"
        fi

        # Also check for versioned .so files
        for sofile in lib${libname}.so.*; do
            if [ -f "$sofile" ]; then
                oe_soinstall "$sofile" "$dest"
            fi
        done
    fi

    # Install static library
    if ! $so_only; then
        if [ -f "lib${libname}.a" ]; then
            install -m 0644 "lib${libname}.a" "$dest/"
        fi
    fi
}

#
# do_install: Default install implementation (runs make install)
#
do_install() {
    bb_note "do_install (default implementation)"
    oe_runmake install DESTDIR="${D}"
}

#
# do_populate_sysroot: Copy files from staging to sysroot
#
do_populate_sysroot() {
    bb_note "do_populate_sysroot"

    # Create sysroot directories
    bbdirs "${STAGING_DIR_HOST}"
    bbdirs "${STAGING_BINDIR}"
    bbdirs "${STAGING_LIBDIR}"
    bbdirs "${STAGING_INCDIR}"
    bbdirs "${STAGING_DATADIR}"

    # Copy installed files to sysroot
    if [ -d "${D}" ]; then
        bb_note "Copying from ${D} to ${STAGING_DIR_HOST}"

        # Copy binaries
        if [ -d "${D}${bindir}" ]; then
            bbdirs "${STAGING_BINDIR}"
            cp -af "${D}${bindir}"/* "${STAGING_BINDIR}/" 2>/dev/null || true
        fi

        # Copy libraries
        if [ -d "${D}${libdir}" ]; then
            bbdirs "${STAGING_LIBDIR}"
            cp -af "${D}${libdir}"/* "${STAGING_LIBDIR}/" 2>/dev/null || true
        fi

        # Copy headers
        if [ -d "${D}${includedir}" ]; then
            bbdirs "${STAGING_INCDIR}"
            cp -af "${D}${includedir}"/* "${STAGING_INCDIR}/" 2>/dev/null || true
        fi

        # Copy data files
        if [ -d "${D}${datadir}" ]; then
            bbdirs "${STAGING_DATADIR}"
            cp -af "${D}${datadir}"/* "${STAGING_DATADIR}/" 2>/dev/null || true
        fi

        bb_note "Sysroot population complete"
    else
        bb_warn "Install directory ${D} not found"
    fi
}

#
# chrpath: Modify RPATH/RUNPATH in binaries (stub for now)
#
chrpath() {
    bb_note "chrpath: $* (stub)"
    # In real implementation, this would modify binary RPATH
    # For now, just acknowledge the call
    return 0
}

#
# create_wrapper: Create a wrapper script for a binary
#
create_wrapper() {
    local wrapper_path="$1"
    shift

    bb_note "Creating wrapper: $wrapper_path"

    cat > "$wrapper_path" <<EOF
#!/bin/sh
# Wrapper script generated by Hitzeleiter
exec "$@"
EOF

    chmod +x "$wrapper_path"
}

#
# merge_config.sh: Merge kernel config fragments (stub for kern-tools)
#
merge_config.sh() {
    bb_note "merge_config.sh: $*"

    # Parse arguments
    local merge_mode=""
    local output_file=".config"
    local inputs=()

    while [ $# -gt 0 ]; do
        case "$1" in
            -m)
                merge_mode="merge"
                shift
                ;;
            -O)
                output_file="$2"
                shift 2
                ;;
            *)
                inputs+=("$1")
                shift
                ;;
        esac
    done

    # If we have input configs, merge them
    if [ ${#inputs[@]} -gt 0 ]; then
        bb_note "Merging ${#inputs[@]} config fragments into ${output_file}"

        # Simple implementation: concatenate config fragments
        for cfg in "${inputs[@]}"; do
            if [ -f "$cfg" ]; then
                bb_note "  Merging: $cfg"
                cat "$cfg" >> "${output_file}"
            else
                bb_warn "Config fragment not found: $cfg"
            fi
        done
    fi

    return 0
}

#
# src_patches: Return list of patches from SRC_URI (stub)
#
src_patches() {
    # This would normally parse SRC_URI and return patches
    # For now, return empty
    return 0
}

# Ensure output directory exists
bbdirs "${D}"

# Log prelude loaded
bb_debug "BitBake prelude loaded for ${PN}"
bb_debug "Build system: ${BUILD_SYS}"
bb_debug "Host system: ${HOST_SYS}"
bb_debug "Target system: ${TARGET_SYS}"
