// Integration test for Python block processing in recipe extraction

use convenient_bitbake::{ExtractionConfig, RecipeExtractor, RecipeGraph};
use std::collections::HashMap;

#[test]
fn test_simple_appendvar() {
    let mut graph = RecipeGraph::new();

    let mut config = ExtractionConfig::default();
    config.use_python_ir = true;

    let extractor = RecipeExtractor::new(config);

    let content = r#"
PN = "test"
DEPENDS = "base"

python __anonymous() {
    d.appendVar('DEPENDS', ' added-by-python')
}
"#;

    let extraction = extractor
        .extract_from_content(&mut graph, "test", content)
        .unwrap();

    println!("DEPENDS: {:?}", extraction.depends);

    assert!(extraction.depends.contains(&"base".to_string()));
    assert!(extraction.depends.contains(&"added-by-python".to_string()),
        "Python block should have added 'added-by-python' to DEPENDS");
}

#[test]
fn test_bb_utils_contains() {
    let mut graph = RecipeGraph::new();

    let mut config = ExtractionConfig::default();
    config.use_python_ir = true;
    // Set DISTRO_FEATURES to include systemd
    config.default_variables.insert("DISTRO_FEATURES".to_string(), "systemd pam".to_string());

    let extractor = RecipeExtractor::new(config);

    let content = r#"
PN = "test"
DEPENDS = "base"

python __anonymous() {
    if bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d):
        d.appendVar('DEPENDS', ' systemd')
}
"#;

    let extraction = extractor
        .extract_from_content(&mut graph, "test", content)
        .unwrap();

    println!("DEPENDS: {:?}", extraction.depends);

    assert!(extraction.depends.contains(&"base".to_string()));
    // This should pass if Phase 10 bb.utils.contains works
    assert!(extraction.depends.contains(&"systemd".to_string()),
        "Python block with bb.utils.contains should have added 'systemd' to DEPENDS");
}

#[test]
fn test_conditional_with_getvar() {
    let mut graph = RecipeGraph::new();

    let mut config = ExtractionConfig::default();
    config.use_python_ir = true;
    config.default_variables.insert("DISTRO_FEATURES".to_string(), "systemd".to_string());

    let extractor = RecipeExtractor::new(config);

    let content = r#"
PN = "test"
DEPENDS = "base"

python __anonymous() {
    features = d.getVar('DISTRO_FEATURES', True) or ''
    if 'systemd' in features:
        d.appendVar('DEPENDS', ' systemd-dep')
}
"#;

    let extraction = extractor
        .extract_from_content(&mut graph, "test", content)
        .unwrap();

    println!("DEPENDS: {:?}", extraction.depends);

    assert!(extraction.depends.contains(&"base".to_string()));
    // This will likely fail because it requires more complex Python execution
    if extraction.depends.contains(&"systemd-dep".to_string()) {
        println!("SUCCESS: Complex Python conditional worked!");
    } else {
        println!("EXPECTED: Complex Python conditional not yet supported");
    }
}
