// Test include resolution with meta-fmu recipes

use convenient_bitbake::{BitbakeRecipe, IncludeResolver};
use std::path::Path;
use walkdir::WalkDir;

fn main() {
    let meta_fmu_path = Path::new("/tmp/meta-fmu");

    if !meta_fmu_path.exists() {
        eprintln!("meta-fmu not found at /tmp/meta-fmu");
        eprintln!(
            "Please clone it first: git clone https://github.com/avrabe/meta-fmu /tmp/meta-fmu"
        );
        std::process::exit(1);
    }

    println!("=== Testing Include Resolution with meta-fmu ===\n");

    let mut total = 0;
    let mut with_includes = 0;
    let mut resolved_successfully = 0;

    for entry in WalkDir::new(meta_fmu_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str());

        // Only process .bb recipe files
        if ext != Some("bb") {
            continue;
        }

        total += 1;
        let relative = path.strip_prefix(meta_fmu_path).unwrap_or(path);

        match BitbakeRecipe::parse_file(path) {
            Ok(mut recipe) => {
                if !recipe.includes.is_empty() {
                    with_includes += 1;

                    println!("📄 {}", relative.display());
                    println!("   Package name: {:?}", recipe.package_name);

                    // Create resolver to show variable expansion
                    let test_resolver = recipe.create_resolver();
                    println!("   BPN: {}", test_resolver.get("BPN").unwrap_or("N/A"));

                    println!("   Includes: {}", recipe.includes.len());
                    for inc in &recipe.includes {
                        let resolved = test_resolver.resolve(&inc.path);
                        println!("      - {} → {} ({})",
                            inc.path, resolved,
                            if inc.required { "require" } else { "include" });
                    }

                    // Create include resolver
                    let mut resolver = IncludeResolver::new();

                    // Add search paths for meta-fmu
                    if let Some(recipe_dir) = path.parent() {
                        resolver.add_search_path(recipe_dir);
                    }
                    resolver.add_search_path(meta_fmu_path);
                    resolver.add_search_path(meta_fmu_path.join("recipes-application/fmu"));
                    resolver.add_search_path(meta_fmu_path.join("conf"));

                    // Get variables before resolution
                    let vars_before = recipe.variables.len();
                    let sources_before = recipe.sources.len();

                    // Resolve includes
                    match resolver.resolve_all_includes(&mut recipe) {
                        Ok(()) => {
                            resolved_successfully += 1;

                            let vars_after = recipe.variables.len();
                            let sources_after = recipe.sources.len();

                            println!("   ✓ Resolved successfully");
                            println!("      Variables: {} → {} (+{})",
                                vars_before, vars_after, vars_after - vars_before);
                            println!("      Sources: {} → {} (+{})",
                                sources_before, sources_after, sources_after - sources_before);

                            let (cache_size, search_paths) = resolver.cache_stats();
                            println!("      Cache: {} files, {} search paths",
                                cache_size, search_paths);

                            // Show some resolved variables
                            if let Some(srcrev) = recipe.variables.get("SRCREV") {
                                println!("      SRCREV: {}", if srcrev.len() > 40 {
                                    format!("{}...", &srcrev[..40])
                                } else {
                                    srcrev.clone()
                                });
                            }

                            // Show resolved SRC_URI if it changed
                            if sources_after > sources_before {
                                println!("      Sources now includes:");
                                for (i, src) in recipe.sources.iter().skip(sources_before).take(3).enumerate() {
                                    println!("        {}. {:?}: {}", i+1, src.scheme,
                                        if src.url.len() > 50 {
                                            format!("{}...", &src.url[..50])
                                        } else {
                                            src.url.clone()
                                        });
                                }
                                if sources_after - sources_before > 3 {
                                    println!("        ... and {} more", sources_after - sources_before - 3);
                                }
                            }

                            println!();
                        }
                        Err(e) => {
                            println!("   ✗ Failed to resolve: {}", e);
                            println!();
                        }
                    }
                }
            }
            Err(e) => {
                println!("✗ {} - Parse ERROR: {}", relative.display(), e);
            }
        }
    }

    println!("=== Summary ===");
    println!("Total recipes: {}", total);
    println!("Recipes with includes: {}", with_includes);
    println!("Successfully resolved: {} ({:.1}%)",
        resolved_successfully,
        if with_includes > 0 {
            (resolved_successfully as f64 / with_includes as f64) * 100.0
        } else {
            0.0
        }
    );

    if resolved_successfully == with_includes {
        println!("\n✓ All recipes with includes resolved successfully!");
    }
}
