// Test parsing OpenEmbedded Core / Poky recipes

use convenient_bitbake::BitbakeRecipe;
use std::path::Path;
use walkdir::WalkDir;

fn main() {
    let poky_path = Path::new("/tmp/poky-recipes");

    if !poky_path.exists() {
        eprintln!("Poky recipes not found at /tmp/poky-recipes");
        std::process::exit(1);
    }

    println!("=== Testing BitBake Parser with OpenEmbedded Core / Poky Recipes ===\n");

    let mut total = 0;
    let mut success = 0;
    let mut errors = 0;
    let mut warnings = 0;

    for entry in WalkDir::new(poky_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str());

        // Only process BitBake files
        if !matches!(ext, Some("bb") | Some("bbappend") | Some("inc") | Some("conf") | Some("bbclass")) {
            continue;
        }

        total += 1;
        let relative = path.strip_prefix(poky_path).unwrap_or(path);

        match BitbakeRecipe::parse_file(path) {
            Ok(recipe) => {
                success += 1;

                if !recipe.parse_errors.is_empty() {
                    errors += recipe.parse_errors.len();
                    println!("⚠️  {} (parsed with {} errors)", relative.display(), recipe.parse_errors.len());
                    for err in &recipe.parse_errors {
                        println!("    - {}", err);
                    }
                } else if !recipe.parse_warnings.is_empty() {
                    warnings += recipe.parse_warnings.len();
                    println!("⚠️  {} (parsed with {} warnings)", relative.display(), recipe.parse_warnings.len());
                } else {
                    println!("✓  {}", relative.display());
                }

                // Show interesting details for .bb files
                if ext == Some("bb") {
                    if let Some(summary) = recipe.variables.get("SUMMARY") {
                        println!("    Summary: {}", summary);
                    }
                    if let Some(license) = recipe.variables.get("LICENSE") {
                        println!("    License: {}", license);
                    }
                    if !recipe.sources.is_empty() {
                        println!("    Sources: {} URIs", recipe.sources.len());
                        for (i, src) in recipe.sources.iter().enumerate().take(5) {
                            println!("      {}. {:?}: {}", i+1, src.scheme,
                                if src.url.len() > 60 {
                                    format!("{}...", &src.url[..60])
                                } else {
                                    src.url.clone()
                                });
                            if let Some(branch) = &src.branch {
                                println!("         branch: {}", branch);
                            }
                            if let Some(protocol) = &src.protocol {
                                println!("         protocol: {}", protocol);
                            }
                        }
                        if recipe.sources.len() > 5 {
                            println!("      ... and {} more", recipe.sources.len() - 5);
                        }
                    }
                    if !recipe.includes.is_empty() {
                        println!("    Includes: {} files", recipe.includes.len());
                    }
                    if !recipe.inherits.is_empty() {
                        println!("    Inherits: {}", recipe.inherits.join(", "));
                    }

                    // Show some variable resolution examples
                    if recipe.variables.contains_key("S") {
                        println!("    S (source dir): {}", recipe.variables.get("S").unwrap());
                    }
                    if recipe.variables.contains_key("PACKAGE_ARCH") {
                        println!("    Package arch: {}", recipe.variables.get("PACKAGE_ARCH").unwrap());
                    }

                    println!();
                }
            }
            Err(e) => {
                println!("✗  {} - ERROR: {}", relative.display(), e);
                errors += 1;
            }
        }
    }

    println!("\n=== Summary ===");
    println!("Total files: {}", total);
    println!("Successfully parsed: {} ({:.1}%)", success, (success as f64 / total as f64) * 100.0);
    println!("Parse errors: {}", errors);
    println!("Parse warnings: {}", warnings);

    if success == total && errors == 0 {
        println!("\n✓ All files parsed successfully!");
    } else if success > 0 {
        println!("\n⚠️  Some files had errors or warnings (but {} parsed successfully)", success);
    } else {
        println!("\n✗ All files failed to parse");
    }
}
