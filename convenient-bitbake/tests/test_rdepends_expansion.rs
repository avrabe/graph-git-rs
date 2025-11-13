// Test RDEPENDS:${PN} expansion in Python blocks
#![cfg(feature = "python-execution")]

use convenient_bitbake::{ExtractionConfig, RecipeExtractor, RecipeGraph};

#[test]
fn test_rdepends_with_pn_expansion() {
    let mut graph = RecipeGraph::new();

    let mut config = ExtractionConfig::default();
    config.use_python_ir = true;
    config.use_python_executor = true;
    config.default_variables.insert("DISTRO_FEATURES".to_string(), "systemd".to_string());

    let extractor = RecipeExtractor::new(config);

    let content = r#"
PN = "mypackage"
DEPENDS = "base"

python __anonymous() {
    if bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d):
        d.appendVar('RDEPENDS:${PN}', ' systemd')
}
"#;

    let extraction = extractor
        .extract_from_content(&mut graph, "test", content)
        .unwrap();

    println!("RDEPENDS: {:?}", extraction.rdepends);

    assert!(extraction.rdepends.contains(&"systemd".to_string()),
        "Python block should have added 'systemd' to RDEPENDS");
}

#[test]
fn test_rdepends_with_pn_expansion_and_else() {
    let mut graph = RecipeGraph::new();

    let mut config = ExtractionConfig::default();
    config.use_python_ir = true;
    config.use_python_executor = true;
    config.default_variables.insert("DISTRO_FEATURES".to_string(), "systemd".to_string());

    let extractor = RecipeExtractor::new(config);

    let content = r#"
PN = "mypackage"
DEPENDS = "base"

python __anonymous() {
    if bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d):
        d.appendVar('RDEPENDS:${PN}', ' systemd')
        d.setVar('INIT_SYSTEM', 'systemd')
    else:
        d.setVar('INIT_SYSTEM', 'sysvinit')
}
"#;

    let extraction = extractor
        .extract_from_content(&mut graph, "test", content)
        .unwrap();

    println!("RDEPENDS with else: {:?}", extraction.rdepends);

    assert!(extraction.rdepends.contains(&"systemd".to_string()),
        "Python block with else should have added 'systemd' to RDEPENDS");
}
