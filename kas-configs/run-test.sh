#!/bin/bash
# Run accuracy test for a single kas configuration

set -e

if [ -z "$1" ]; then
    echo "Usage: $0 <kas-config-file>"
    echo "Example: $0 01-basic-poky.yml"
    exit 1
fi

CONFIG="$1"
CONFIG_NAME=$(basename "$CONFIG" .yml)
RESULTS_DIR="../results/$CONFIG_NAME"

echo "=== Testing Configuration: $CONFIG ==="

# Check if kas is installed
if ! command -v kas &> /dev/null; then
    echo "Error: kas not found. Install with: pip3 install kas"
    exit 1
fi

# Build measurement tool if needed
if [ ! -f "../accuracy-measurement/target/release/measure-accuracy" ]; then
    echo "Building measurement tool..."
    cd ../accuracy-measurement
    cargo build --release
    cd ../kas-configs
fi

# Set up kas environment
echo "Setting up kas environment..."
kas checkout "$CONFIG"

# Determine recipe directories based on config
case "$CONFIG_NAME" in
    01-basic-poky)
        RECIPE_DIRS=(
            "/tmp/kas/poky/meta/recipes-core"
            "/tmp/kas/poky/meta/recipes-extended"
            "/tmp/kas/poky/meta/recipes-connectivity"
        )
        ;;
    02-poky-openembedded)
        RECIPE_DIRS=(
            "/tmp/kas/poky/meta/recipes-core"
            "/tmp/kas/meta-openembedded/meta-oe/recipes-core"
            "/tmp/kas/meta-openembedded/meta-oe/recipes-extended"
            "/tmp/kas/meta-openembedded/meta-python/recipes-devtools"
        )
        ;;
    03-poky-custom-meta)
        RECIPE_DIRS=(
            "/tmp/kas/poky/meta/recipes-core"
            "/tmp/kas/meta-openembedded/meta-oe/recipes-core"
            "../meta-test/recipes-test"
        )
        ;;
    04-poky-full-complexity)
        RECIPE_DIRS=(
            "/tmp/kas/poky/meta/recipes-*"
            "/tmp/kas/meta-openembedded/meta-*/recipes-*"
            "/tmp/kas/meta-virtualization/recipes-*"
            "/tmp/kas/meta-security/recipes-*"
            "../meta-test/recipes-test"
        )
        ;;
    *)
        RECIPE_DIRS=("/tmp/kas")
        ;;
esac

# Run measurement for each directory
echo "Running measurements..."
mkdir -p "$RESULTS_DIR"

for dir_pattern in "${RECIPE_DIRS[@]}"; do
    # Expand glob pattern
    for dir in $dir_pattern; do
        if [ -d "$dir" ]; then
            echo "Scanning: $dir"
            ../accuracy-measurement/target/release/measure-accuracy scan \
                --dir "$dir" \
                --output "$RESULTS_DIR" \
                --compare || echo "Warning: Failed to scan $dir"
        fi
    done
done

# Generate report
echo "Generating report..."
../accuracy-measurement/target/release/measure-accuracy report \
    --input "$RESULTS_DIR"

echo "=== Test Complete ==="
echo "Results saved to: $RESULTS_DIR"
echo ""
echo "View summary:"
echo "  cat $RESULTS_DIR/ACCURACY_REPORT.md"
echo ""
echo "View JSON:"
echo "  jq . $RESULTS_DIR/accuracy-report.json"
