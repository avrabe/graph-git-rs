use convenient_bitbake::{RecipeExtractor, ExtractionConfig, RecipeGraph};

fn main() {
    let dropbear_content = r#"
SUMMARY = "Dropbear SSH"
DEPENDS = "zlib virtual/crypt"

PACKAGECONFIG ?= "disable-weak-ciphers ${@bb.utils.filter('DISTRO_FEATURES', 'pam', d)}"
PACKAGECONFIG[pam] = "--enable-pam,--disable-pam,libpam,${PAM_PLUGINS}"
PACKAGECONFIG[system-libtom] = "--disable-bundled-libtom,--enable-bundled-libtom,libtommath libtomcrypt"
PACKAGECONFIG[disable-weak-ciphers] = ""
"#;

    let mut config = ExtractionConfig::default();
    config.use_simple_python_eval = true;
    config.extract_tasks = false;

    let extractor = RecipeExtractor::new(config);
    let mut graph = RecipeGraph::new();

    let result = extractor.extract_from_content(&mut graph, "dropbear", dropbear_content).unwrap();

    println!("dropbear extraction:");
    println!("  DEPENDS: {}", result.depends.join(", "));
    if let Some(pc) = result.variables.get("PACKAGECONFIG") {
        println!("  PACKAGECONFIG = {}", pc);
    }

    // Expected: zlib, virtual/crypt (from DEPENDS) + libpam (from PACKAGECONFIG[pam])
    println!("\nDependency analysis:");
    for dep in &["zlib", "virtual/crypt", "libpam"] {
        if result.depends.contains(&dep.to_string()) {
            println!("  ✓ {} found", dep);
        } else {
            println!("  ✗ {} MISSING", dep);
        }
    }

    let all_found = result.depends.contains(&"zlib".to_string()) &&
                    result.depends.contains(&"virtual/crypt".to_string()) &&
                    result.depends.contains(&"libpam".to_string());

    if all_found {
        println!("\n✓ SUCCESS: PACKAGECONFIG extraction working correctly!");
    } else {
        println!("\n✗ FAILED: Some dependencies missing");
    }
}
