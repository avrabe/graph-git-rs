//! BitBake helper functions for task execution
//!
//! Provides shell function implementations that BitBake tasks expect

/// Get BitBake helper functions as a shell script fragment
pub fn get_bb_helpers() -> &'static str {
    r#"#!/bin/bash

# BitBake helper function implementations

oe_runmake() {
    # Run make with parallel jobs
    local jobs="${PARALLEL_MAKE:--j4}"
    make $jobs "$@"
}

bbdebug() {
    # Debug logging (level, message)
    local level=$1
    shift
    echo "[DEBUG $level] $@" >&2
}

bbnote() {
    # Info logging
    echo "[NOTE] $@" >&2
}

bbwarn() {
    # Warning logging
    echo "[WARN] $@" >&2
}

bbfatal() {
    # Fatal error
    echo "[FATAL] $@" >&2
    exit 1
}

bbfatal_log() {
    # Fatal error with log context
    echo "[FATAL] $@" >&2
    exit 1
}

oe_soinstall() {
    # Install shared library with proper symlinks
    # Usage: oe_soinstall libfoo.so.1.2.3 $D/usr/lib
    local lib=$1
    local dest=$2

    if [ ! -f "$lib" ]; then
        bbfatal "oe_soinstall: $lib not found"
    fi

    install -m 0755 "$lib" "$dest/"

    # Create symlinks for versioned libraries
    if echo "$lib" | grep -q '\.so\.[0-9]*\.[0-9]*\.[0-9]*$'; then
        local base=$(basename "$lib" | sed 's/\.[0-9]*\.[0-9]*\.[0-9]*$//')
        local major=$(basename "$lib" | sed 's/.*\.so\.\([0-9]*\)\..*/\1/')
        ln -sf "$(basename $lib)" "$dest/${base}.so.$major"
        ln -sf "$(basename $lib)" "$dest/${base}.so"
    elif echo "$lib" | grep -q '\.so\.[0-9]*$'; then
        local base=$(basename "$lib" | sed 's/\.[0-9]*$//')
        ln -sf "$(basename $lib)" "$dest/${base}.so"
    fi
}

do_install_append() {
    # Hook for appending to do_install
    :
}

do_compile_prepend() {
    # Hook for prepending to do_compile
    :
}

copy_locale_files() {
    # Placeholder for locale file copying
    bbnote "copy_locale_files called (stub implementation)"
}

install_append() {
    # Install with directory creation
    mkdir -p "$(dirname "$2")" 2>/dev/null || true
    install "$@"
}

oe_libinstall() {
    # Install library files
    # Usage: oe_libinstall [-C dir] [-s] libname dest
    local dir=""
    local sudo=""
    local libname=""
    local dest=""

    while [ $# -gt 0 ]; do
        case "$1" in
            -C) dir="$2"; shift 2 ;;
            -s) sudo="sudo"; shift ;;
            *) if [ -z "$libname" ]; then
                   libname="$1"
               else
                   dest="$1"
               fi
               shift ;;
        esac
    done

    [ -n "$dir" ] && cd "$dir"

    # Find and install library
    for lib in lib${libname}.so* lib${libname}.a; do
        if [ -f "$lib" ]; then
            $sudo install -m 0755 "$lib" "$dest/"
        fi
    done
}

create_wrapper() {
    # Create a wrapper script
    # Usage: create_wrapper script_path
    local wrapper=$1
    shift
    cat > "$wrapper" <<EOF
#!/bin/sh
exec "$@"
EOF
    chmod +x "$wrapper"
}

oe_runconf() {
    # Run configure script with common options
    local confscript="${S}/configure"
    if [ -f "$confscript" ]; then
        bbnote "Running configure"
        $confscript \
            --build=${BUILD_SYS:-x86_64-linux} \
            --host=${HOST_SYS:-x86_64-linux} \
            --target=${TARGET_SYS:-x86_64-linux} \
            --prefix=${prefix:-/usr} \
            --exec_prefix=${exec_prefix:-/usr} \
            --bindir=${bindir:-/usr/bin} \
            --sbindir=${sbindir:-/usr/sbin} \
            --libdir=${libdir:-/usr/lib} \
            --datadir=${datadir:-/usr/share} \
            --includedir=${includedir:-/usr/include} \
            --sysconfdir=${sysconfdir:-/etc} \
            --localstatedir=${localstatedir:-/var} \
            --disable-static \
            "$@"
    else
        bbwarn "Configure script not found at $confscript"
    fi
}

autotools_do_configure() {
    # Standard autotools configure step
    cd "${B}" || cd "${S}" || return 1

    if [ -f "${S}/configure" ]; then
        oe_runconf "$@"
    elif [ -f "${S}/configure.ac" ] || [ -f "${S}/configure.in" ]; then
        bbnote "Running autoreconf"
        cd "${S}"
        autoreconf -fi || bbwarn "autoreconf failed"
        cd "${B}" || cd "${S}"
        oe_runconf "$@"
    else
        bbnote "No configure script found, skipping configure"
    fi
}

autotools_do_compile() {
    # Standard autotools compile step
    cd "${B}" || cd "${S}" || return 1
    oe_runmake "$@"
}

autotools_do_install() {
    # Standard autotools install step
    cd "${B}" || cd "${S}" || return 1
    oe_runmake install DESTDIR="${D}" "$@"
}

base_do_configure() {
    # Base configure - usually a no-op
    bbnote "base_do_configure called"
}

base_do_compile() {
    # Base compile
    if [ -f Makefile ] || [ -f makefile ] || [ -f GNUmakefile ]; then
        oe_runmake
    fi
}

base_do_install() {
    # Base install
    bbnote "base_do_install called"
}

# Stub implementations for fetch/unpack to allow task progression
base_do_fetch() {
    bbnote "Stub: base_do_fetch - would fetch from SRC_URI"
    # In real BitBake, this would download sources
    # For now, just create work directory
    mkdir -p "${WORKDIR}" "${DL_DIR}" || true
}

base_do_unpack() {
    bbnote "Stub: base_do_unpack - would extract sources to ${S}"
    # In real BitBake, this would extract archives
    # For now, just create source directory
    mkdir -p "${S}" || true
}

base_do_patch() {
    bbnote "Stub: base_do_patch - would apply patches"
    # In real BitBake, this would apply patches from SRC_URI
}

# Python-style function stubs (sometimes called from shell)
oe_machinstall() {
    # Machine-specific installation
    local target_dir="$1"
    shift
    mkdir -p "$target_dir" || true
    install -m 0644 "$@" "$target_dir/" 2>/dev/null || bbnote "oe_machinstall: some files not found"
}

chrpath() {
    # Stub for chrpath tool (modifies rpath)
    bbnote "Stub: chrpath $@"
}

do_populate_lic() {
    # License population stub
    bbnote "Stub: do_populate_lic"
}

fakeroot() {
    # Run command as fake root
    "$@"
}

# Additional commonly needed functions
oe_multilib_header() {
    # Multilib header handling
    local header="$1"
    bbnote "Stub: oe_multilib_header $header"
}

oe_runmake_call() {
    # Alternative runmake
    oe_runmake "$@"
}

# Installation helpers
install_append() {
    # Install with automatic directory creation
    local file=$1
    local dest=$2
    mkdir -p "$(dirname "$dest")" || true
    install -m 0644 "$file" "$dest" 2>/dev/null || true
}

do_install_append() {
    # Append to install task (often overridden)
    bbnote "do_install_append called"
}

do_deploy() {
    # Deploy task for images/bootloaders
    bbnote "Stub: do_deploy"
    mkdir -p "${DEPLOYDIR}" || true
}

# Kernel/module helpers
kernel_do_install() {
    bbnote "Stub: kernel_do_install"
}

module_do_install() {
    bbnote "Stub: module_do_install"
}

# Package splitting helpers
populate_packages() {
    bbnote "Stub: populate_packages"
}

PACKAGES_prepend() {
    bbnote "Stub: PACKAGES_prepend"
}

# Additional build system helpers
cmake_do_configure() {
    bbnote "Running cmake configure"
    cd "${B}" || cd "${S}" || return 1
    if [ -f "${S}/CMakeLists.txt" ]; then
        cmake "${S}" \
            -DCMAKE_INSTALL_PREFIX=/usr \
            -DCMAKE_BUILD_TYPE=Release \
            "$@" || bbwarn "cmake configure failed"
    else
        bbwarn "No CMakeLists.txt found"
    fi
}

cmake_do_compile() {
    cd "${B}" || cd "${S}" || return 1
    cmake --build . || make
}

cmake_do_install() {
    cd "${B}" || cd "${S}" || return 1
    DESTDIR="${D}" cmake --install . || make install DESTDIR="${D}"
}

"#
}

/// Prepend BitBake helpers to a task script
pub fn add_bb_helpers_to_script(script: &str) -> String {
    format!("{}\n\n{}", get_bb_helpers(), script)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_helpers_not_empty() {
        let helpers = get_bb_helpers();
        assert!(helpers.contains("oe_runmake"));
        assert!(helpers.contains("bbfatal"));
    }

    #[test]
    fn test_add_helpers() {
        let script = "echo 'Hello'";
        let with_helpers = add_bb_helpers_to_script(script);
        assert!(with_helpers.contains("oe_runmake"));
        assert!(with_helpers.contains("echo 'Hello'"));
    }
}
