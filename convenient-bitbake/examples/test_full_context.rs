// Comprehensive integration test with meta-fmu
// Demonstrates Phases 1-4: Variable resolution, includes, layer context, and OVERRIDES

use convenient_bitbake::{BuildContext, OverrideResolver, OverrideOp};
use std::path::Path;

fn main() {
    let meta_fmu_path = Path::new("/tmp/meta-fmu");

    if !meta_fmu_path.exists() {
        eprintln!("meta-fmu not found at /tmp/meta-fmu");
        eprintln!(
            "Please clone it first: git clone https://github.com/avrabe/meta-fmu /tmp/meta-fmu"
        );
        std::process::exit(1);
    }

    println!("=== BitBake Parser - Full Context Demo with meta-fmu ===\n");

    // ========== Phase 3: Build Context ==========
    println!("Phase 3: Building Layer Context");
    println!("{}", "=".repeat(60));

    let mut context = BuildContext::new();

    // Add meta-fmu layer
    let layer_conf = meta_fmu_path.join("conf/layer.conf");
    if layer_conf.exists() {
        match context.add_layer_from_conf(&layer_conf) {
            Ok(()) => {
                println!("✓ Loaded meta-fmu layer");
                let layers = context.get_layers_info();
                for (name, priority, path) in layers {
                    println!("  Layer: {} (priority: {}, path: {})", name, priority, path.display());
                }
            }
            Err(e) => {
                println!("✗ Failed to load layer: {}", e);
            }
        }
    }

    // Set machine and distro (from kas yaml)
    context.set_machine("qemuarm64".to_string());
    context.set_distro("container".to_string());

    println!("\n✓ Build Context:");
    println!("  MACHINE: {}", context.machine.as_ref().unwrap());
    println!("  DISTRO: {}", context.distro.as_ref().unwrap());

    // Load distro configuration
    let distro_conf = meta_fmu_path.join("conf/distro/container.conf");
    if distro_conf.exists() {
        match context.load_conf_file(&distro_conf) {
            Ok(()) => {
                println!("\n✓ Loaded distro configuration");
                // Show some interesting variables
                if let Some(tclibc) = context.global_variables.get("TCLIBC") {
                    println!("  TCLIBC: {}", tclibc);
                }
                if let Some(features) = context.global_variables.get("DISTRO_FEATURES") {
                    println!("  DISTRO_FEATURES: {}", features);
                }
            }
            Err(e) => {
                println!("✗ Failed to load distro conf: {}", e);
            }
        }
    }

    // ========== Phases 1-2: Parse Recipe with Includes ==========
    println!("\n\nPhases 1-2: Parsing Recipe with Variable Resolution and Includes");
    println!("{}", "=".repeat(60));

    let recipe_path = meta_fmu_path.join("recipes-application/fmu/fmu-rs_0.2.0.bb");
    if !recipe_path.exists() {
        println!("✗ Recipe not found: {}", recipe_path.display());
        std::process::exit(1);
    }

    match context.parse_recipe_with_context(&recipe_path) {
        Ok(recipe) => {
            println!("✓ Parsed recipe: {}", recipe_path.file_name().unwrap().to_string_lossy());

            // Show package metadata
            println!("\nPackage Information:");
            println!("  PN: {:?}", recipe.package_name);
            println!("  PV: {:?}", recipe.package_version);

            if let Some(summary) = recipe.variables.get("SUMMARY") {
                println!("  SUMMARY: {}", summary);
            }
            if let Some(license) = recipe.variables.get("LICENSE") {
                println!("  LICENSE: {}", license);
            }

            // Show includes that were resolved
            println!("\nIncludes (from recipe): {}", recipe.includes.len());
            for inc in &recipe.includes {
                println!("  - {}", inc.path);
            }

            // Show sources
            println!("\nSources: {}", recipe.sources.len());
            for (i, src) in recipe.sources.iter().enumerate().take(5) {
                println!("  {}. {:?}: {}", i+1, src.scheme,
                    if src.url.len() > 60 {
                        format!("{}...", &src.url[..60])
                    } else {
                        src.url.clone()
                    });
            }
            if recipe.sources.len() > 5 {
                println!("  ... and {} more sources", recipe.sources.len() - 5);
            }

            // ========== Phase 1: Variable Resolution ==========
            println!("\n\nPhase 1: Variable Resolution");
            println!("{}", "=".repeat(60));

            let resolver = context.create_resolver(&recipe);

            println!("✓ Created resolver with global context");
            println!("\nBuilt-in variables:");
            println!("  PN:  {:?}", resolver.get("PN"));
            println!("  BPN: {:?}", resolver.get("BPN"));
            println!("  PV:  {:?}", resolver.get("PV"));
            println!("  BP:  {:?}", resolver.get("BP"));

            println!("\nGlobal variables:");
            println!("  MACHINE: {:?}", resolver.get("MACHINE"));
            println!("  DISTRO:  {:?}", resolver.get("DISTRO"));

            // Test variable expansion
            if let Some(src_uri) = recipe.variables.get("SRC_URI") {
                println!("\nVariable Expansion Example:");
                println!("  Raw:      {}", if src_uri.len() > 80 {
                    format!("{}...", &src_uri[..80])
                } else {
                    src_uri.clone()
                });
                let resolved = resolver.resolve(src_uri);
                println!("  Resolved: {}", if resolved.len() > 80 {
                    format!("{}...", &resolved[..80])
                } else {
                    resolved
                });
            }

            // ========== Phase 4: OVERRIDES Resolution ==========
            println!("\n\nPhase 4: OVERRIDES Resolution");
            println!("{}", "=".repeat(60));

            let mut override_resolver = OverrideResolver::new(resolver);

            // Build overrides from context
            override_resolver.build_overrides_from_context(
                context.machine.as_deref(),
                context.distro.as_deref(),
                &vec![], // Additional overrides
            );

            println!("✓ Built override context");
            println!("  Active OVERRIDES: {:?}", override_resolver.active_overrides());

            // Demonstrate override resolution
            println!("\nOverride Resolution Examples:");

            // Example 1: Machine-specific dependency
            override_resolver.add_assignment(
                "DEPENDS",
                "base-dep".to_string(),
                OverrideOp::Assign,
            );
            override_resolver.add_assignment(
                "DEPENDS:append:arm",
                "arm-specific-dep".to_string(),
                OverrideOp::Assign,
            );
            override_resolver.add_assignment(
                "DEPENDS:append:qemuarm64",
                "qemuarm64-dep".to_string(),
                OverrideOp::Assign,
            );

            if let Some(depends) = override_resolver.resolve("DEPENDS") {
                println!("  DEPENDS (with overrides): {}", depends);
            }

            // Example 2: Distro feature removal
            override_resolver.add_assignment(
                "DISTRO_FEATURES",
                "acl ipv4 ipv6 largefile".to_string(),
                OverrideOp::Assign,
            );
            override_resolver.add_assignment(
                "DISTRO_FEATURES:remove:container",
                "largefile".to_string(),
                OverrideOp::Assign,
            );

            if let Some(features) = override_resolver.resolve("DISTRO_FEATURES") {
                println!("  DISTRO_FEATURES (after :remove): {}", features);
            }

            // ========== Summary ==========
            println!("\n\n{}", "=".repeat(60));
            println!("Summary");
            println!("{}", "=".repeat(60));

            println!("\n✓ All phases completed successfully:");
            println!("  Phase 1: Variable Resolution - ✓");
            println!("  Phase 2: Include Resolution - ✓ ({} includes)", recipe.includes.len());
            println!("  Phase 3: Layer Context - ✓ ({} layer)", context.get_layers_info().len());
            println!("  Phase 4: OVERRIDES - ✓ ({} overrides)",
                override_resolver.active_overrides().len());

            println!("\n✓ Comprehensive BitBake static analysis complete!");
            println!("  Total variables: {}", recipe.variables.len());
            println!("  Total sources: {}", recipe.sources.len());
            println!("  Inherits: {}", recipe.inherits.len());
            println!("  Build depends: {}", recipe.build_depends.len());

            // Show final resolved SRC_URI
            if !recipe.sources.is_empty() {
                println!("\n✓ Extracted git repositories:");
                for src in &recipe.sources {
                    if matches!(src.scheme, convenient_bitbake::UriScheme::Git |
                                            convenient_bitbake::UriScheme::GitSubmodule) {
                        println!("  - {}", src.url);
                        if let Some(branch) = &src.branch {
                            println!("    branch: {}", branch);
                        }
                        if let Some(srcrev) = recipe.variables.get("SRCREV") {
                            println!("    SRCREV: {}", srcrev);
                        }
                        break; // Show first git repo
                    }
                }
            }

            println!("\n🎉 Ready for graph-git-rs integration!");
        }
        Err(e) => {
            println!("✗ Failed to parse recipe: {}", e);
            std::process::exit(1);
        }
    }
}
