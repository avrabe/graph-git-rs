#!/bin/bash
# Run accuracy tests for all kas configurations

set -e

echo "=== Running All Accuracy Tests ==="
echo "This will take some time..."
echo ""

CONFIGS=(
    "01-basic-poky.yml"
    "02-poky-openembedded.yml"
    "03-poky-custom-meta.yml"
    "04-poky-full-complexity.yml"
)

RESULTS_SUMMARY="../results/SUMMARY.md"
mkdir -p ../results

# Clear previous summary
cat > "$RESULTS_SUMMARY" << 'EOF'
# Accuracy Measurement Summary - All Configurations

## Test Run Information

**Date:** $(date)
**Phase 10 Status:** Enabled

---

EOF

# Run each configuration
for config in "${CONFIGS[@]}"; do
    echo "========================================"
    echo "Testing: $config"
    echo "========================================"

    ./run-test.sh "$config" || echo "Warning: Test failed for $config"

    echo ""
    echo "Completed: $config"
    echo ""

    # Append results to summary
    config_name=$(basename "$config" .yml)
    if [ -f "../results/$config_name/ACCURACY_REPORT.md" ]; then
        cat >> "$RESULTS_SUMMARY" << EOF

## Configuration: $config_name

$(cat "../results/$config_name/ACCURACY_REPORT.md")

---

EOF
    fi
done

echo "=== All Tests Complete ==="
echo ""
echo "Results saved to: ../results/"
echo ""
echo "Summary report: $RESULTS_SUMMARY"
echo ""

# Generate comparison table
echo "=== Results Summary ===" | tee -a "$RESULTS_SUMMARY"
echo "" | tee -a "$RESULTS_SUMMARY"
echo "| Configuration | Total Recipes | With Python | Phase 10 Impact | DEPENDS Added | RDEPENDS Added |" | tee -a "$RESULTS_SUMMARY"
echo "|--------------|--------------|-------------|-----------------|---------------|----------------|" | tee -a "$RESULTS_SUMMARY"

for config in "${CONFIGS[@]}"; do
    config_name=$(basename "$config" .yml)
    json_file="../results/$config_name/accuracy-report.json"

    if [ -f "$json_file" ]; then
        total=$(jq '.total_recipes' "$json_file")
        python=$(jq '.recipes_with_python' "$json_file")
        impact=$(jq '.phase10_impact_count' "$json_file")
        deps=$(jq '.total_deps_added' "$json_file")
        rdeps=$(jq '.total_rdeps_added' "$json_file")

        echo "| $config_name | $total | $python | $impact | $deps | $rdeps |" | tee -a "$RESULTS_SUMMARY"
    fi
done

echo "" | tee -a "$RESULTS_SUMMARY"
echo "Full results in: ../results/" | tee -a "$RESULTS_SUMMARY"
