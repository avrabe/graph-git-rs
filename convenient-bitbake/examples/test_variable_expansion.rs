use convenient_bitbake::{RecipeExtractor, ExtractionConfig, RecipeGraph};

fn main() {
    let recipe_content = r#"
SUMMARY = "Test recipe for variable expansion"
PV = "1.2.3"

# Test ${PN} expansion
DEPENDS = "lib${PN} ${PN}-tools"

# Test ${PV} expansion
SRC_URI = "http://example.com/archive-${PV}.tar.gz"

# Test ${P} expansion (PN-PV)
S = "${WORKDIR}/${P}"

# Test ${BPN} expansion
PROVIDES = "${BPN}"

# Keep complex variables as-is
RDEPENDS = "${VIRTUAL-RUNTIME_init_manager} ${PN}"
"#;

    let mut config = ExtractionConfig::default();
    config.use_simple_python_eval = false;
    config.extract_tasks = false;

    let extractor = RecipeExtractor::new(config);
    let mut graph = RecipeGraph::new();

    let result = extractor.extract_from_content(&mut graph, "myapp", recipe_content).unwrap();

    println!("Variable Expansion Test (PN=myapp, PV=1.2.3):\n");

    println!("Extracted variables:");
    for (key, value) in &result.variables {
        if key != "SUMMARY" {
            println!("  {} = {}", key, value);
        }
    }

    println!("\nDependency analysis:");
    println!("  DEPENDS: {}", result.depends.join(", "));
    println!("  RDEPENDS: {}", result.rdepends.join(", "));

    // Verify expansions
    println!("\nVerification:");
    let mut all_ok = true;

    // Check ${PN} expansion in DEPENDS
    if result.depends.contains(&"libmyapp".to_string()) {
        println!("  ✓ ${{PN}} expanded to 'myapp' in DEPENDS");
    } else {
        println!("  ✗ ${{PN}} NOT expanded correctly in DEPENDS");
        all_ok = false;
    }

    // Check ${PV} expansion in SRC_URI
    if let Some(src_uri) = result.variables.get("SRC_URI") {
        if src_uri.contains("archive-1.2.3.tar.gz") {
            println!("  ✓ ${{PV}} expanded to '1.2.3' in SRC_URI");
        } else {
            println!("  ✗ ${{PV}} NOT expanded correctly: {}", src_uri);
            all_ok = false;
        }
    }

    // Check ${P} expansion in S
    if let Some(s) = result.variables.get("S") {
        if s.contains("myapp-1.2.3") {
            println!("  ✓ ${{P}} expanded to 'myapp-1.2.3'");
        } else {
            println!("  ✗ ${{P}} NOT expanded correctly: {}", s);
            all_ok = false;
        }
    }

    // Check that VIRTUAL-RUNTIME is kept as-is
    if result.rdepends.contains(&"${VIRTUAL-RUNTIME_init_manager}".to_string()) {
        println!("  ✓ ${{VIRTUAL-RUNTIME_*}} kept as-is");
    } else {
        println!("  ⚠ ${{VIRTUAL-RUNTIME_*}} not found (might be filtered)");
    }

    if all_ok {
        println!("\n✓ SUCCESS: Variable expansion working correctly!");
    } else {
        println!("\n✗ FAILED: Some expansions incorrect");
    }
}
