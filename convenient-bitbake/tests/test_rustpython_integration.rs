// Integration test for RustPython execution in recipe extraction
#![cfg(feature = "python-execution")]

use convenient_bitbake::{ExtractionConfig, RecipeExtractor, RecipeGraph};

#[test]
fn test_complex_python_with_in_operator_integration() {
    let mut graph = RecipeGraph::new();

    let mut config = ExtractionConfig::default();
    config.use_python_ir = true;
    config.use_python_executor = true;  // Enable RustPython
    config.default_variables.insert("PACKAGECONFIG".to_string(), "feature1 feature2".to_string());

    let extractor = RecipeExtractor::new(config);

    let content = r#"
PN = "test"
DEPENDS = "base"

python __anonymous() {
    pkgconfig = d.getVar('PACKAGECONFIG', True) or ''
    if 'feature1' in pkgconfig:
        d.appendVar('DEPENDS', ' feature1-lib')
}
"#;

    let extraction = extractor
        .extract_from_content(&mut graph, "test", content)
        .unwrap();

    println!("DEPENDS: {:?}", extraction.depends);

    assert!(extraction.depends.contains(&"base".to_string()), "Should have base dependency");
    assert!(extraction.depends.contains(&"feature1-lib".to_string()),
        "RustPython should have added 'feature1-lib' via complex Python pattern");
}

#[test]
fn test_complex_python_with_startswith_integration() {
    let mut graph = RecipeGraph::new();

    let mut config = ExtractionConfig::default();
    config.use_python_ir = true;
    config.use_python_executor = true;  // Enable RustPython
    config.default_variables.insert("MACHINE".to_string(), "qemux86-64".to_string());
    config.default_variables.insert("PN".to_string(), "testpkg".to_string());

    let extractor = RecipeExtractor::new(config);

    let content = r#"
PN = "testpkg"
RDEPENDS = "base-runtime"

python __anonymous() {
    machine = d.getVar('MACHINE', True)
    if machine and machine.startswith('qemu'):
        d.appendVar('RDEPENDS:${PN}', ' qemu-helper')
}
"#;

    let extraction = extractor
        .extract_from_content(&mut graph, "test", content)
        .unwrap();

    println!("RDEPENDS: {:?}", extraction.rdepends);

    assert!(extraction.rdepends.contains(&"base-runtime".to_string()), "Should have base-runtime");
    assert!(extraction.rdepends.contains(&"qemu-helper".to_string()),
        "RustPython should have added 'qemu-helper' via complex Python pattern with startswith");
}

#[test]
fn test_bb_utils_contains_via_rustpython() {
    let mut graph = RecipeGraph::new();

    let mut config = ExtractionConfig::default();
    config.use_python_ir = true;
    config.use_python_executor = true;  // Enable RustPython
    config.default_variables.insert("DISTRO_FEATURES".to_string(), "systemd pam".to_string());
    config.default_variables.insert("PN".to_string(), "testpkg".to_string());

    let extractor = RecipeExtractor::new(config);

    let content = r#"
PN = "testpkg"
RDEPENDS = "base-runtime"

python __anonymous() {
    # This should work via RustPython's bb.utils implementation
    result = bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d)
    if result:
        d.appendVar('RDEPENDS:${PN}', ' systemd')
}
"#;

    let extraction = extractor
        .extract_from_content(&mut graph, "test", content)
        .unwrap();

    println!("RDEPENDS: {:?}", extraction.rdepends);

    assert!(extraction.rdepends.contains(&"base-runtime".to_string()), "Should have base-runtime");
    assert!(extraction.rdepends.contains(&"systemd".to_string()),
        "RustPython should have added 'systemd' via bb.utils.contains");
}
