# BitBake Accuracy Measurement with Kas

This directory contains kas configurations and tools for measuring Phase 10 accuracy improvements on real BitBake recipes.

## Prerequisites

```bash
# Install kas
pip3 install kas

# Build the measurement tool
cd ../accuracy-measurement
cargo build --release
```

## Kas Configurations

Four configurations with increasing complexity:

### 1. Basic Poky (`01-basic-poky.yml`)
- Minimal Poky setup
- ~500 recipes
- Tests basic patterns
- Fastest to set up

### 2. Poky + OpenEmbedded (`02-poky-openembedded.yml`)
- Adds meta-openembedded layers
- ~2,500 recipes
- Tests common OE patterns
- Moderate setup time

### 3. Poky + Custom Meta (`03-poky-custom-meta.yml`)
- Adds custom test layer
- Tests PACKAGECONFIG overrides
- Tests bbappends
- Good for controlled testing

### 4. Full Complexity (`04-poky-full-complexity.yml`)
- Maximum layer count
- ~4,000+ recipes
- Tests all patterns
- Longest setup time

## Quick Start

### Option A: Test with Local Meta Layer

```bash
# 1. Set up the build environment
kas shell 03-poky-custom-meta.yml

# 2. Inside the shell, list available recipes
bitbake-layers show-recipes

# 3. Exit and scan the recipes
exit

# 4. Run measurement
cd ../accuracy-measurement
cargo run --release -- scan \
    --dir /tmp/kas/meta-test/recipes-test \
    --output ../results/meta-test \
    --compare

# 5. View report
cargo run --release -- report --input ../results/meta-test
```

### Option B: Test with Full Poky

```bash
# 1. Set up basic poky
kas checkout 01-basic-poky.yml

# 2. Scan all poky recipes
cd ../accuracy-measurement
cargo run --release -- scan \
    --dir /tmp/kas/poky/meta/recipes-* \
    --output ../results/poky-basic \
    --compare

# 3. View results
cargo run --release -- report --input ../results/poky-basic
```

### Option C: Test with OpenEmbedded

```bash
# 1. Set up with OE layers (takes longer)
kas checkout 02-poky-openembedded.yml

# 2. Scan OE recipes
cd ../accuracy-measurement
cargo run --release -- scan \
    --dir /tmp/kas/meta-openembedded/meta-oe/recipes-* \
    --output ../results/openembedded \
    --compare

# 3. View results
cargo run --release -- report --input ../results/openembedded
```

## Running Tests

### Single Configuration Test

```bash
# Test basic poky
./run-test.sh 01-basic-poky

# Test with OpenEmbedded
./run-test.sh 02-poky-openembedded

# Test custom meta
./run-test.sh 03-poky-custom-meta

# Test full complexity
./run-test.sh 04-poky-full-complexity
```

### Run All Tests

```bash
./run-all-tests.sh
```

## Manual Testing

### 1. Set up kas environment

```bash
# Choose a configuration
kas shell 01-basic-poky.yml
```

### 2. Inside the kas shell

```bash
# List all recipes
bitbake-layers show-recipes

# Show recipe info
bitbake-layers show-recipes systemd

# Build a specific recipe
bitbake systemd

# Generate dependency graph
bitbake -g systemd

# View recipe file
bitbake -e systemd | grep "^DEPENDS="
```

### 3. Extract and test

```bash
# Exit kas shell
exit

# Run measurement on specific recipe
cd accuracy-measurement
cargo run --release -- scan \
    --dir /tmp/kas/poky/meta/recipes-core/systemd \
    --compare
```

## Understanding Results

### Output Files

After running measurements, check:

```
results/
├── <config-name>/
│   ├── accuracy-report.json      # Machine-readable results
│   ├── ACCURACY_REPORT.md        # Human-readable report
│   └── details/                  # Per-recipe details
```

### Key Metrics

- **Total Recipes**: Number of .bb files analyzed
- **Recipes with Python**: Recipes using `python __anonymous()`
- **Phase 10 Impact**: Recipes where Phase 10 added dependencies
- **DEPENDS Added**: Build-time dependencies discovered
- **RDEPENDS Added**: Runtime dependencies discovered

### Example Output

```
=== Summary ===
Total recipes analyzed: 523
Recipes with Python blocks: 47 (9.0%)

=== Phase 10 Impact ===
Recipes affected by Phase 10: 23 (4.4%)
Total DEPENDS added: 15
Total RDEPENDS added: 31

Top recipes with most changes:
  1. systemd (2 DEPENDS, 3 RDEPENDS)
     Added DEPENDS: systemd, libcap
     Added RDEPENDS: systemd, udev, systemd-serialgetty
```

## Expected Results

Based on Phase 10 analysis:

### Conservative Estimate
- **Recipes Affected**: 4-6%
- **Dependencies Added**: 30-50 per 1,000 recipes
- **Accuracy Improvement**: +1.4-1.8%

### Optimistic Estimate (with RustPython)
- **Recipes Affected**: 8-10%
- **Dependencies Added**: 80-120 per 1,000 recipes
- **Accuracy Improvement**: +3-5%

## Troubleshooting

### Kas fails to fetch

```bash
# Clean kas cache
rm -rf ~/.cache/kas

# Try with specific commit
kas checkout --update 01-basic-poky.yml
```

### Measurement tool errors

```bash
# Rebuild with verbose output
cd accuracy-measurement
cargo build --release --verbose

# Run with debug logging
RUST_LOG=debug cargo run --release -- scan --dir <path>
```

### No recipes found

```bash
# Check kas setup completed
ls -la /tmp/kas/

# Check build directory
kas shell 01-basic-poky.yml -c 'bitbake-layers show-layers'
```

## Advanced Usage

### Custom Recipe Testing

Create test recipes in `meta-test/recipes-test/` and measure:

```bash
cd accuracy-measurement
cargo run --release -- scan \
    --dir ../meta-test/recipes-test \
    --compare
```

### Compare Different Configurations

```bash
# Run all 4 configs
for config in 01 02 03 04; do
    ./run-test.sh ${config}*
done

# Compare results
diff results/01-basic-poky/accuracy-report.json \
     results/04-full-complexity/accuracy-report.json
```

### Export for Analysis

```bash
# Export to CSV for spreadsheet analysis
cd accuracy-measurement
cargo run --release -- scan --dir <path> --format csv
```

## Next Steps

After measurement:

1. **Analyze patterns**: Which Python patterns are most common?
2. **Identify gaps**: What patterns aren't recognized?
3. **Prioritize enhancements**: Focus on high-impact patterns
4. **Iterate**: Add patterns and re-measure

See `../docs/PHASE_10_ACCURACY_ANALYSIS.md` for detailed methodology.
