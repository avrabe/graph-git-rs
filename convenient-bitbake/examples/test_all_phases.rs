use convenient_bitbake::{RecipeExtractor, ExtractionConfig, RecipeGraph};

fn main() {
    println!("=== Testing All 3 Phases Together ===\n");

    let recipe_content = r#"
SUMMARY = "Comprehensive test combining all phases"
PV = "2.5.0"

# Phase 1: Python expression evaluation
DEPENDS = "zlib ${@bb.utils.contains('DISTRO_FEATURES', 'pam', 'libpam', '', d)}"

# Phase 2: PACKAGECONFIG conditional dependencies
PACKAGECONFIG ??= "${@bb.utils.filter('DISTRO_FEATURES', 'systemd', d)}"
PACKAGECONFIG[systemd] = "--with-systemd,--without-systemd,systemd"

# Phase 3: Variable expansion
RDEPENDS = "${PN}-base lib${PN}"
PROVIDES = "${BPN} ${P}"
SRC_URI = "http://example.com/${BPN}-${PV}.tar.gz"
"#;

    let mut config = ExtractionConfig::default();
    config.use_simple_python_eval = true;
    config.extract_tasks = false;

    let extractor = RecipeExtractor::new(config);
    let mut graph = RecipeGraph::new();

    let result = extractor.extract_from_content(&mut graph, "myservice", recipe_content).unwrap();

    println!("Recipe: myservice (version 2.5.0)");
    println!("Default DISTRO_FEATURES: systemd pam ipv6 usrmerge\n");

    println!("Phase 1: Python Expression Evaluation");
    println!("  Input:  DEPENDS = \"zlib ${{@bb.utils.contains('DISTRO_FEATURES', 'pam', 'libpam', '', d)}}\"");
    println!("  Output: DEPENDS = {}", result.depends.join(", "));
    if result.depends.contains(&"libpam".to_string()) {
        println!("  ✓ bb.utils.contains correctly evaluated (pam in DISTRO_FEATURES → libpam)");
    }

    println!("\nPhase 2: PACKAGECONFIG Conditional Dependencies");
    if let Some(pc) = result.variables.get("PACKAGECONFIG") {
        println!("  Input:  PACKAGECONFIG = \"${{@bb.utils.filter('DISTRO_FEATURES', 'systemd', d)}}\"");
        println!("  Evaluated: PACKAGECONFIG = {}", pc);
        println!("  Config: PACKAGECONFIG[systemd] = \"--with-systemd,--without-systemd,systemd\"");
        if result.depends.contains(&"systemd".to_string()) {
            println!("  ✓ PACKAGECONFIG[systemd] correctly added systemd to DEPENDS");
        }
    }

    println!("\nPhase 3: Variable Expansion");
    println!("  Variables: PN=myservice, PV=2.5.0, BPN=myservice");
    println!("  Input:  RDEPENDS = \"${{PN}}-base lib${{PN}}\"");
    println!("  Output: RDEPENDS = {}", result.rdepends.join(", "));
    if result.rdepends.contains(&"myservice-base".to_string()) &&
       result.rdepends.contains(&"libmyservice".to_string()) {
        println!("  ✓ ${{PN}} correctly expanded to 'myservice'");
    }

    if let Some(provides) = result.variables.get("PROVIDES") {
        println!("  Input:  PROVIDES = \"${{BPN}} ${{P}}\"");
        println!("  Output: PROVIDES = {}", provides);
        if provides.contains("myservice-2.5.0") {
            println!("  ✓ ${{P}} correctly expanded to 'myservice-2.5.0'");
        }
    }

    if let Some(src_uri) = result.variables.get("SRC_URI") {
        println!("  Input:  SRC_URI = \"http://example.com/${{BPN}}-${{PV}}.tar.gz\"");
        println!("  Output: SRC_URI = {}", src_uri);
        if src_uri.contains("myservice-2.5.0.tar.gz") {
            println!("  ✓ ${{BPN}} and ${{PV}} correctly expanded");
        }
    }

    println!("\n=== Final Results ===");
    println!("BUILD DEPENDS: {}", result.depends.join(", "));
    println!("  Expected: zlib, libpam (Phase 1), systemd (Phase 2)");

    let expected_deps = vec!["zlib", "libpam", "systemd"];
    let all_found = expected_deps.iter().all(|d| result.depends.contains(&d.to_string()));

    if all_found {
        println!("\n✓ SUCCESS: All 3 phases working correctly together!");
    } else {
        println!("\n✗ FAILED: Some dependencies missing");
        for dep in expected_deps {
            if !result.depends.contains(&dep.to_string()) {
                println!("  Missing: {}", dep);
            }
        }
    }
}
