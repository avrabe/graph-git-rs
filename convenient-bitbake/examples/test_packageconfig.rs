use convenient_bitbake::{RecipeExtractor, ExtractionConfig, RecipeGraph};

fn main() {
    let pciutils_content = r#"
SUMMARY = "PCI utilities"
DEPENDS = "zlib kmod make-native"

PACKAGECONFIG ??= "${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'hwdb', '', d)}"
PACKAGECONFIG[hwdb] = "HWDB=yes,HWDB=no,udev"
"#;

    let config = ExtractionConfig {
        use_simple_python_eval: true,
        extract_tasks: true,
        ..Default::default()
    };

    let extractor = RecipeExtractor::new(config);
    let mut graph = RecipeGraph::new();

    let result = extractor.extract_from_content(&mut graph, "pciutils", pciutils_content).unwrap();

    println!("pciutils extraction:");
    println!("  DEPENDS: {}", result.depends.join(", "));
    println!("  Variables:");
    if let Some(pc) = result.variables.get("PACKAGECONFIG") {
        println!("    PACKAGECONFIG = {}", pc);
    }

    // Should include: zlib, kmod, make-native (from DEPENDS) + udev (from PACKAGECONFIG[hwdb])
    let expected = vec!["zlib", "kmod", "make-native", "udev"];
    let missing: Vec<_> = expected.iter()
        .filter(|dep| !result.depends.contains(&dep.to_string()))
        .collect();

    if missing.is_empty() {
        println!("\n✓ SUCCESS: All expected dependencies found!");
    } else {
        println!("\n✗ MISSING dependencies: {:?}", missing);
    }
}
