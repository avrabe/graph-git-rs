// Comprehensive validation tool comparing parser output with expected BitBake behavior
// This tool validates our parser against known BitBake semantics

use convenient_bitbake::{BuildContext, OverrideResolver, OverrideOp, SimpleResolver};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug)]
struct ValidationResult {
    test_name: String,
    expected: String,
    actual: String,
    passed: bool,
}

impl ValidationResult {
    fn new(test_name: &str, expected: String, actual: String) -> Self {
        let passed = expected == actual;
        Self {
            test_name: test_name.to_string(),
            expected,
            actual,
            passed,
        }
    }
}

struct ValidationSuite {
    results: Vec<ValidationResult>,
}

impl ValidationSuite {
    fn new() -> Self {
        Self {
            results: Vec::new(),
        }
    }

    fn test(&mut self, name: &str, expected: String, actual: String) {
        let result = ValidationResult::new(name, expected, actual);
        if result.passed {
            println!("  ✓ {}", name);
        } else {
            println!("  ✗ {}", name);
            println!("    Expected: {}", result.expected);
            println!("    Actual:   {}", result.actual);
        }
        self.results.push(result);
    }

    fn summary(&self) {
        let passed = self.results.iter().filter(|r| r.passed).count();
        let total = self.results.len();
        let percentage = (passed as f64 / total as f64) * 100.0;

        println!("\n{}", "=".repeat(70));
        println!("Validation Summary");
        println!("{}", "=".repeat(70));
        println!("Total tests: {}", total);
        println!("Passed: {} ({:.1}%)", passed, percentage);
        println!("Failed: {}", total - passed);

        if passed == total {
            println!("\n✓ All validation tests passed!");
        } else {
            println!("\n⚠ {} test(s) failed - review above for details", total - passed);
        }
    }
}

fn main() {
    let meta_fmu_path = Path::new("/tmp/meta-fmu");

    if !meta_fmu_path.exists() {
        eprintln!("meta-fmu not found at /tmp/meta-fmu");
        std::process::exit(1);
    }

    println!("=== BitBake Parser Validation Suite ===\n");

    let mut suite = ValidationSuite::new();

    // ========== Test 1: Filename Parsing ==========
    println!("\n[1] Filename-based Variable Derivation");
    println!("{}", "-".repeat(70));

    let recipe_path = meta_fmu_path.join("recipes-application/fmu/fmu-rs_0.2.0.bb");
    let recipe = convenient_bitbake::BitbakeRecipe::parse_file(&recipe_path).unwrap();

    suite.test(
        "PN from filename",
        "fmu-rs".to_string(),
        recipe.package_name.clone().unwrap_or_default(),
    );

    suite.test(
        "PV from filename",
        "0.2.0".to_string(),
        recipe.package_version.clone().unwrap_or_default(),
    );

    // ========== Test 2: Variable Resolution ==========
    println!("\n[2] Variable Resolution");
    println!("{}", "-".repeat(70));

    let resolver = recipe.create_resolver();

    suite.test(
        "BPN derived from PN",
        "fmu-rs".to_string(),
        resolver.get("BPN").unwrap_or("").to_string(),
    );

    suite.test(
        "BP combination",
        "fmu-rs-0.2.0".to_string(),
        resolver.get("BP").unwrap_or("").to_string(),
    );

    // Test default values
    suite.test(
        "WORKDIR default",
        "/tmp/work".to_string(),
        resolver.get("WORKDIR").unwrap_or("").to_string(),
    );

    // ========== Test 3: Include Resolution ==========
    println!("\n[3] Include Resolution");
    println!("{}", "-".repeat(70));

    let mut recipe_with_includes = convenient_bitbake::BitbakeRecipe::parse_file(&recipe_path).unwrap();
    let mut include_resolver = convenient_bitbake::IncludeResolver::new();
    include_resolver.add_search_path(meta_fmu_path);
    include_resolver.add_search_path(meta_fmu_path.join("recipes-application/fmu"));

    match include_resolver.resolve_all_includes(&mut recipe_with_includes) {
        Ok(()) => {
            // Check if SRCREV was loaded from include
            let has_srcrev = recipe_with_includes.variables.contains_key("SRCREV");
            suite.test(
                "SRCREV loaded from include",
                "true".to_string(),
                has_srcrev.to_string(),
            );

            // Check if we have crates from include
            let source_count = recipe_with_includes.sources.len();
            suite.test(
                "Sources from includes (>1)",
                "true".to_string(),
                (source_count > 1).to_string(),
            );

            if let Some(srcrev) = recipe_with_includes.variables.get("SRCREV") {
                // SRCREV should be a git hash (40 characters)
                suite.test(
                    "SRCREV format (40 chars)",
                    "true".to_string(),
                    (srcrev.len() == 40).to_string(),
                );
            }
        }
        Err(e) => {
            println!("  ⚠ Include resolution error: {}", e);
        }
    }

    // ========== Test 4: Layer Context ==========
    println!("\n[4] Layer Context");
    println!("{}", "-".repeat(70));

    let mut context = BuildContext::new();
    let layer_conf = meta_fmu_path.join("conf/layer.conf");

    match context.add_layer_from_conf(&layer_conf) {
        Ok(()) => {
            suite.test(
                "Layer collection name",
                "fmu".to_string(),
                context.layers[0].collection.clone(),
            );

            suite.test(
                "Layer priority",
                "1".to_string(),
                context.layers[0].priority.to_string(),
            );

            suite.test(
                "Layer dependencies",
                "true".to_string(),
                (!context.layers[0].depends.is_empty()).to_string(),
            );
        }
        Err(e) => {
            println!("  ⚠ Layer config error: {}", e);
        }
    }

    context.set_machine("qemuarm64".to_string());
    context.set_distro("container".to_string());

    suite.test(
        "MACHINE setting",
        "qemuarm64".to_string(),
        context.machine.clone().unwrap_or_default(),
    );

    suite.test(
        "DISTRO setting",
        "container".to_string(),
        context.distro.clone().unwrap_or_default(),
    );

    // ========== Test 5: OVERRIDES ==========
    println!("\n[5] OVERRIDES Resolution");
    println!("{}", "-".repeat(70));

    let base_resolver = SimpleResolver::new(&recipe);
    let mut override_resolver = OverrideResolver::new(base_resolver);

    override_resolver.build_overrides_from_context(
        Some("qemuarm64"),
        Some("container"),
        &[],
    );

    let overrides = override_resolver.active_overrides();

    suite.test(
        "qemuarm64 in OVERRIDES",
        "true".to_string(),
        overrides.contains(&"qemuarm64".to_string()).to_string(),
    );

    suite.test(
        "arm in OVERRIDES (auto-detected)",
        "true".to_string(),
        overrides.contains(&"arm".to_string()).to_string(),
    );

    suite.test(
        "64 in OVERRIDES (auto-detected)",
        "true".to_string(),
        overrides.contains(&"64".to_string()).to_string(),
    );

    suite.test(
        "container in OVERRIDES",
        "true".to_string(),
        overrides.contains(&"container".to_string()).to_string(),
    );

    // Test override application
    override_resolver.add_assignment("TEST_VAR", "base".to_string(), OverrideOp::Assign);
    override_resolver.add_assignment("TEST_VAR:append:arm", "arm-addon".to_string(), OverrideOp::Assign);

    let test_result = override_resolver.resolve("TEST_VAR").unwrap_or_default();
    suite.test(
        ":append:arm applied",
        "base arm-addon".to_string(),
        test_result,
    );

    // ========== Test 6: SRC_URI Extraction ==========
    println!("\n[6] SRC_URI Extraction and Resolution");
    println!("{}", "-".repeat(70));

    // Parse recipe with full context
    let full_recipe = context.parse_recipe_with_context(&recipe_path).unwrap();

    // Check main git source
    let git_sources: Vec<_> = full_recipe
        .sources
        .iter()
        .filter(|s| matches!(s.scheme, convenient_bitbake::UriScheme::Git))
        .collect();

    suite.test(
        "Git source found",
        "true".to_string(),
        (!git_sources.is_empty()).to_string(),
    );

    if let Some(git_src) = git_sources.first() {
        suite.test(
            "Git URL",
            "git://github.com/avrabe/fmu-rs".to_string(),
            git_src.url.clone(),
        );

        suite.test(
            "Git branch",
            "main".to_string(),
            git_src.branch.clone().unwrap_or_default(),
        );

        suite.test(
            "Git protocol",
            "https".to_string(),
            git_src.protocol.clone().unwrap_or_default(),
        );
    }

    // ========== Test 7: Variable Expansion in Strings ==========
    println!("\n[7] Variable Expansion");
    println!("{}", "-".repeat(70));

    let full_resolver = context.create_resolver(&full_recipe);

    let test_expansion = full_resolver.resolve("${WORKDIR}/${BPN}-${PV}");
    suite.test(
        "Complex expansion",
        "/tmp/work/fmu-rs-0.2.0".to_string(),
        test_expansion,
    );

    // Test with actual recipe variables
    if let Some(s_var) = full_recipe.variables.get("S") {
        let s_resolved = full_resolver.resolve(s_var);
        let expected_prefix = "/tmp/work/";
        suite.test(
            "S variable starts with WORKDIR",
            "true".to_string(),
            s_resolved.starts_with(expected_prefix).to_string(),
        );
    }

    // ========== Test 8: Metadata Extraction ==========
    println!("\n[8] Recipe Metadata");
    println!("{}", "-".repeat(70));

    suite.test(
        "SUMMARY extracted",
        "true".to_string(),
        full_recipe.variables.contains_key("SUMMARY").to_string(),
    );

    suite.test(
        "LICENSE extracted",
        "MIT".to_string(),
        full_recipe.variables.get("LICENSE").unwrap_or(&String::new()).clone(),
    );

    if let Some(summary) = full_recipe.variables.get("SUMMARY") {
        suite.test(
            "SUMMARY not empty",
            "true".to_string(),
            (!summary.is_empty()).to_string(),
        );
    }

    // ========== Test 9: Inherit and Dependencies ==========
    println!("\n[9] Inherits and Dependencies");
    println!("{}", "-".repeat(70));

    suite.test(
        "Inherits cargo",
        "true".to_string(),
        full_recipe.inherits.contains(&"cargo".to_string()).to_string(),
    );

    suite.test(
        "Has DEPENDS",
        "true".to_string(),
        (!full_recipe.build_depends.is_empty()).to_string(),
    );

    // ========== Test 10: Complete Integration ==========
    println!("\n[10] Complete Integration Test");
    println!("{}", "-".repeat(70));

    // Can we get everything we need for graph-git-rs?
    let has_git_url = !git_sources.is_empty();
    let has_srcrev = full_recipe.variables.contains_key("SRCREV");
    let has_pn = full_recipe.package_name.is_some();

    suite.test(
        "All graph data available",
        "true".to_string(),
        (has_git_url && has_srcrev && has_pn).to_string(),
    );

    // Demonstrate complete extraction for graph-git-rs
    if has_git_url && has_srcrev && has_pn {
        println!("\n  Complete graph data extraction:");
        println!("    Package: {}", full_recipe.package_name.as_ref().unwrap());

        if let Some(git_src) = git_sources.first() {
            println!("    Repository: {}", git_src.url);
            if let Some(branch) = &git_src.branch {
                println!("    Branch: {}", branch);
            }
        }

        if let Some(srcrev) = full_recipe.variables.get("SRCREV") {
            let resolved_srcrev = full_resolver.resolve(srcrev);
            println!("    SRCREV: {}", resolved_srcrev);
        }
    }

    // Print summary
    suite.summary();

    // Exit with appropriate code
    let all_passed = suite.results.iter().all(|r| r.passed);
    if !all_passed {
        std::process::exit(1);
    }
}
