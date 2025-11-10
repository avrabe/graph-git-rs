// Test parsing all BitBake files in meta-fmu

use convenient_bitbake::BitbakeRecipe;
use std::path::Path;
use walkdir::WalkDir;

fn main() {
    let meta_fmu_path = Path::new("/tmp/meta-fmu");

    if !meta_fmu_path.exists() {
        eprintln!("meta-fmu not found at /tmp/meta-fmu");
        eprintln!("Please clone it first: git clone https://github.com/avrabe/meta-fmu /tmp/meta-fmu");
        std::process::exit(1);
    }

    println!("=== Testing BitBake Parser with meta-fmu ===\n");

    let mut total = 0;
    let mut success = 0;
    let mut errors = 0;
    let mut warnings = 0;

    for entry in WalkDir::new(meta_fmu_path)
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
        let relative = path.strip_prefix(meta_fmu_path).unwrap_or(path);

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
                if ext == Some("bb") && recipe.parse_errors.is_empty() {
                    if let Some(name) = &recipe.package_name {
                        println!("    Package: {}", name);
                    }
                    if !recipe.sources.is_empty() {
                        println!("    Sources: {} URIs", recipe.sources.len());
                        for src in &recipe.sources {
                            println!("      - {:?}: {}", src.scheme, src.url);
                            if let Some(branch) = &src.branch {
                                println!("        branch: {}", branch);
                            }
                            if let Some(srcrev) = &src.srcrev {
                                println!("        srcrev: {}", srcrev);
                            }
                        }
                    }
                    if !recipe.includes.is_empty() {
                        println!("    Includes: {} files", recipe.includes.len());
                        for inc in &recipe.includes {
                            println!("      - {}{}", inc.path, if inc.required { " (required)" } else { "" });
                        }
                    }
                    if !recipe.inherits.is_empty() {
                        println!("    Inherits: {}", recipe.inherits.join(", "));
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
    } else {
        println!("\n⚠️  Some files had errors or warnings");
    }
}
