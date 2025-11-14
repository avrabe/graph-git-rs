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
        mkdir -p "$dir"
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

# Ensure output directory exists
bbdirs "${D}"

# Log prelude loaded
bb_debug "BitBake prelude loaded for ${PN}"
