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
