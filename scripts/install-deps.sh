#!/bin/bash
set -e

echo "=== Hitzeleiter Build Environment Setup ==="
echo ""

# Check for protoc (required for convenient-cache gRPC)
if ! command -v protoc &> /dev/null; then
    echo "📦 Installing protobuf-compiler..."
    if [ -f /etc/debian_version ]; then
        sudo apt-get update && sudo apt-get install -y protobuf-compiler
    elif command -v brew &> /dev/null; then
        brew install protobuf
    else
        echo "⚠️  Please install protoc manually: https://github.com/protocolbuffers/protobuf/releases"
        exit 1
    fi
    echo "✅ protoc installed"
else
    echo "✅ protoc already installed ($(protoc --version))"
fi

# Verify Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "📦 Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "✅ Rust installed"
else
    echo "✅ Rust already installed ($(rustc --version))"
fi

# Optional: Build project to cache dependencies (only in remote environments)
if [ "$CLAUDE_CODE_REMOTE" = "true" ]; then
    echo ""
    echo "🔨 Pre-building workspace (this may take a few minutes)..."
    echo "   This caches all dependencies for faster subsequent builds."
    cargo build --workspace 2>&1 | head -n 50
    echo "✅ Workspace built successfully"
fi

echo ""
echo "=== Environment Ready ==="
echo ""
echo "You can now:"
echo "  • Build: cargo build --release"
echo "  • Test:  cargo test"
echo "  • Run:   cargo run -p hitzeleiter -- --help"
echo ""
