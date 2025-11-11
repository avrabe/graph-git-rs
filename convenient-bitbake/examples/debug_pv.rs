// Debug PV extraction to understand the discrepancy
use convenient_bitbake::{BuildContext, BitbakeRecipe};
use std::path::Path;

fn main() {
    let meta_fmu_path = Path::new("/tmp/meta-fmu");
    let recipe_path = meta_fmu_path.join("recipes-application/fmu/fmu-rs_0.2.0.bb");

    println!("=== Debugging PV Extraction ===\n");
    println!("Recipe file: {}", recipe_path.display());
    println!();

    // Parse without context first
    println!("--- Direct parsing (no includes) ---");
    let recipe_simple = BitbakeRecipe::parse_file(&recipe_path).unwrap();

    println!("From filename parsing:");
    println!("  package_name: {:?}", recipe_simple.package_name);
    println!("  package_version: {:?}", recipe_simple.package_version);
    println!();

    println!("From recipe variables:");
    println!("  PV: {:?}", recipe_simple.variables.get("PV"));
    println!("  PV:append: {:?}", recipe_simple.variables.get("PV:append"));
    println!("  PV_append: {:?}", recipe_simple.variables.get("PV_append"));
    println!();

    // Parse with full context
    println!("--- Parsing with full context (includes) ---");
    let mut context = BuildContext::new();
    context.add_layer_from_conf(meta_fmu_path.join("conf/layer.conf")).unwrap();
    context.set_machine("qemuarm64".to_string());

    let recipe_full = context.parse_recipe_with_context(&recipe_path).unwrap();

    println!("From filename parsing:");
    println!("  package_name: {:?}", recipe_full.package_name);
    println!("  package_version: {:?}", recipe_full.package_version);
    println!();

    println!("From recipe variables:");
    println!("  PV: {:?}", recipe_full.variables.get("PV"));
    println!("  PV:append: {:?}", recipe_full.variables.get("PV:append"));
    println!("  PV_append: {:?}", recipe_full.variables.get("PV_append"));
    println!();

    // Check what the resolver returns
    println!("--- SimpleResolver behavior ---");
    let resolver = recipe_full.create_resolver();
    println!("  resolver.get(\"PN\"): {:?}", resolver.get("PN"));
    println!("  resolver.get(\"PV\"): {:?}", resolver.get("PV"));
    println!("  resolver.get(\"BPN\"): {:?}", resolver.get("BPN"));
    println!("  resolver.get(\"BP\"): {:?}", resolver.get("BP"));
    println!();

    // Analysis
    println!("=== Analysis ===");
    println!();
    println!("Expected behavior:");
    println!("  1. Filename 'fmu-rs_0.2.0.bb' should give package_version = '0.2.0'");
    println!("  2. Recipe has 'PV:append = \".AUTOINC+${{SRCPV}}\"'");
    println!("  3. SimpleResolver should return base PV = '0.2.0'");
    println!("  4. OverrideResolver should apply :append to get '0.2.0.AUTOINC+${{SRCPV}}'");
    println!();

    if recipe_full.package_version.as_deref() != Some("0.2.0") {
        println!("⚠ ISSUE FOUND:");
        println!("  package_version is not '0.2.0' as expected from filename");
        println!("  This suggests the parser may be overwriting it with PV variable value");
    } else {
        println!("✓ package_version correctly set to '0.2.0' from filename");
    }
}
