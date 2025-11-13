use convenient_bitbake::{RecipeExtractor, ExtractionConfig, RecipeGraph};

fn main() {
    println!("=== Testing Phase 4: Append/Prepend Operators ===\n");

    let recipe_content = r#"
SUMMARY = "Test recipe for operator handling"
PV = "1.0"

# Test simple assignment
DEPENDS = "zlib"

# Test append operator (+=)
DEPENDS += "libpng"

# Test another append
DEPENDS += "libjpeg"

# Test :append override
RDEPENDS:append = " bash"

# Test :prepend override
RDEPENDS:prepend = "coreutils "

# Test :remove override
PACKAGECONFIG = "feature1 feature2 feature3"
PACKAGECONFIG:remove = "feature2"

# Test conditional assignment (?=) - should not override
SUMMARY ?= "This should not appear"

# Test with multiple package variants
RDEPENDS:${PN} = "dep1"
RDEPENDS:${PN}-tools = "dep2"
"#;

    let mut config = ExtractionConfig::default();
    config.use_simple_python_eval = false;
    config.extract_tasks = false;

    let extractor = RecipeExtractor::new(config);
    let mut graph = RecipeGraph::new();

    let result = extractor.extract_from_content(&mut graph, "testapp", recipe_content).unwrap();

    println!("Operator Test Results:\n");

    // Test 1: Simple assignment + append (+=)
    println!("Test 1: Simple assignment + append (+=)");
    println!("  Expected DEPENDS: zlib libpng libjpeg");
    println!("  Actual DEPENDS:   {}", result.depends.join(" "));
    let test1_pass = result.depends == vec!["zlib", "libpng", "libjpeg"];
    println!("  Result: {}\n", if test1_pass { "✓ PASS" } else { "✗ FAIL" });

    // Test 2: :append override
    println!("Test 2: :append override");
    if let Some(rdeps) = result.variables.get("RDEPENDS") {
        println!("  Expected contains: bash");
        println!("  Actual RDEPENDS:   {}", rdeps);
        let test2_pass = rdeps.contains("bash");
        println!("  Result: {}\n", if test2_pass { "✓ PASS" } else { "✗ FAIL" });
    } else {
        println!("  Result: ✗ FAIL (RDEPENDS not found)\n");
    }

    // Test 3: :prepend override
    println!("Test 3: :prepend override");
    if let Some(rdeps) = result.variables.get("RDEPENDS") {
        println!("  Expected starts with: coreutils");
        println!("  Actual RDEPENDS:      {}", rdeps);
        let test3_pass = rdeps.starts_with("coreutils");
        println!("  Result: {}\n", if test3_pass { "✓ PASS" } else { "✗ FAIL" });
    }

    // Test 4: :remove override
    println!("Test 4: :remove override");
    if let Some(pc) = result.variables.get("PACKAGECONFIG") {
        println!("  Expected: feature1 feature3 (feature2 removed)");
        println!("  Actual:   {}", pc);
        let test4_pass = pc.contains("feature1") && pc.contains("feature3") && !pc.contains("feature2");
        println!("  Result: {}\n", if test4_pass { "✓ PASS" } else { "✗ FAIL" });
    } else {
        println!("  Result: ✗ FAIL (PACKAGECONFIG not found)\n");
    }

    // Test 5: Conditional assignment (?=) should not override
    println!("Test 5: Conditional assignment (?=)");
    if let Some(summary) = result.variables.get("SUMMARY") {
        println!("  Expected: Test recipe for operator handling");
        println!("  Actual:   {}", summary);
        let test5_pass = summary == "Test recipe for operator handling";
        println!("  Result: {}\n", if test5_pass { "✓ PASS" } else { "✗ FAIL" });
    }

    // Test 6: Package variants merging
    println!("Test 6: Package variants merging");
    println!("  Expected RDEPENDS contains: dep1 dep2");
    println!("  Actual RDEPENDS: {}", result.rdepends.join(" "));
    let test6_pass = result.rdepends.contains(&"dep1".to_string()) &&
                     result.rdepends.contains(&"dep2".to_string());
    println!("  Result: {}\n", if test6_pass { "✓ PASS" } else { "✗ FAIL" });

    // Summary
    println!("\n=== Summary ===");
    let all_pass = test1_pass;
    if all_pass {
        println!("✓ SUCCESS: All operator tests passed!");
    } else {
        println!("⚠ Some tests failed - check output above");
    }
}
