#!/bin/bash
# Automated validation script for BitBake dependency extraction

set -e

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KAS_FILE="${PROJECT_ROOT}/convenient-kas/fmu-project.yml"
BUILD_DIR="${PROJECT_ROOT}/build"

echo "=== BitBake Dependency Extraction Validation ==="
echo ""

# Check prerequisites
echo "Checking prerequisites..."

if ! command -v kas &> /dev/null; then
    echo "❌ KAS not found. Install with: pip3 install kas"
    exit 1
fi
echo "✓ KAS found: $(kas --version)"

if ! command -v bitbake &> /dev/null; then
    echo "⚠ BitBake not found in PATH (will be available in KAS shell)"
fi

if ! command -v cargo &> /dev/null; then
    echo "❌ Cargo not found. Install Rust from https://rustup.rs/"
    exit 1
fi
echo "✓ Cargo found: $(cargo --version | head -n1)"

echo ""

# Step 1: Clone repositories
echo "Step 1: Cloning repositories with KAS..."
echo ""

if [ -d "${PROJECT_ROOT}/poky" ]; then
    echo "⚠ poky/ already exists, skipping checkout"
else
    echo "Running: kas checkout ${KAS_FILE}"
    kas checkout "${KAS_FILE}"
    echo "✓ Repositories cloned"
fi

echo ""

# Step 2: Generate BitBake graph
echo "Step 2: Generating BitBake dependency graph..."
echo ""

if [ -f "${BUILD_DIR}/task-depends.dot" ]; then
    echo "⚠ ${BUILD_DIR}/task-depends.dot already exists"
    read -p "Regenerate? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Skipping BitBake graph generation"
    else
        rm -f "${BUILD_DIR}"/*.dot "${BUILD_DIR}/pn-buildlist"
        echo "Running: kas shell ${KAS_FILE} -c 'bitbake -g fmu-image'"
        kas shell "${KAS_FILE}" -c "bitbake -g fmu-image"
        echo "✓ BitBake graph generated"
    fi
else
    echo "Running: kas shell ${KAS_FILE} -c 'bitbake -g fmu-image'"
    kas shell "${KAS_FILE}" -c "bitbake -g fmu-image"
    echo "✓ BitBake graph generated"
fi

echo ""

# Step 3: Run our validation tool
echo "Step 3: Running our validation tool..."
echo ""

cd "${PROJECT_ROOT}"
cargo build --example kas_validation --release

echo ""
echo "=== VALIDATION RESULTS ==="
echo ""

./target/release/examples/kas_validation "${BUILD_DIR}"

echo ""
echo "=== Validation Complete ==="
echo ""
echo "Generated files:"
echo "  BitBake graph: ${BUILD_DIR}/task-depends.dot"
echo "  BitBake recipes: ${BUILD_DIR}/pn-buildlist"
echo "  Recipe dependencies: ${BUILD_DIR}/pn-depends.dot"
echo ""
echo "To visualize graphs:"
echo "  dot -Tpng ${BUILD_DIR}/pn-depends.dot -o bitbake-graph.png"
echo ""
echo "To export our graph:"
echo "  cargo run --example end_to_end_extraction > our-graph.dot"
echo "  dot -Tpng our-graph.dot -o our-graph.png"
