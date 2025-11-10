// Test variable resolution with meta-fmu recipes

use convenient_bitbake::BitbakeRecipe;
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

    println!("=== Testing Variable Resolution with meta-fmu ===\n");

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

        let relative = path.strip_prefix(meta_fmu_path).unwrap_or(path);

        match BitbakeRecipe::parse_file(path) {
            Ok(recipe) => {
                println!("📄 {}", relative.display());
                println!("   Type: {:?}", recipe.recipe_type);

                // Show original and resolved SRC_URI
                let resolved_uris = if let Some(src_uri_raw) = recipe.variables.get("SRC_URI") {
                    println!("\n   SRC_URI (raw):");
                    // Show first 100 chars to keep output manageable
                    let preview = if src_uri_raw.len() > 100 {
                        format!("{}...", &src_uri_raw[..100])
                    } else {
                        src_uri_raw.clone()
                    };
                    println!("   {}", preview);

                    // Resolve variables
                    let uris = recipe.resolve_src_uri();

                    if !uris.is_empty() {
                        println!("\n   SRC_URI (resolved): {} URIs", uris.len());
                        for (i, uri) in uris.iter().enumerate().take(5) {
                            println!("   {}. {}", i + 1, uri);
                        }
                        if uris.len() > 5 {
                            println!("   ... and {} more", uris.len() - 5);
                        }
                    }
                    uris
                } else {
                    Vec::new()
                };

                // Show resolver details for interesting variables
                let resolver = recipe.create_resolver();

                if let Some(pn) = resolver.get("PN") {
                    println!("\n   Package variables:");
                    println!("   PN:  {}", pn);
                    if let Some(bpn) = resolver.get("BPN") {
                        println!("   BPN: {}", bpn);
                    }
                    if let Some(pv) = resolver.get("PV") {
                        println!("   PV:  {}", pv);
                    }
                    if let Some(bp) = resolver.get("BP") {
                        println!("   BP:  {}", bp);
                    }
                }

                // Test resolution of S (source directory)
                if let Some(s_raw) = recipe.variables.get("S") {
                    let s_resolved = resolver.resolve(s_raw);
                    if s_raw != &s_resolved {
                        println!("\n   S (source directory):");
                        println!("   Raw:      {}", s_raw);
                        println!("   Resolved: {}", s_resolved);
                    }
                }

                // Test resolution of SRCREV if present
                if let Some(srcrev) = recipe.variables.get("SRCREV") {
                    let srcrev_resolved = resolver.resolve(srcrev);
                    println!("\n   SRCREV: {}", srcrev_resolved);
                }

                // Check for unresolved variables in resolved output
                let unresolved_count: usize = resolved_uris
                    .iter()
                    .filter(|uri| uri.contains("${"))
                    .count();

                if unresolved_count > 0 {
                    println!(
                        "\n   ⚠️  {} URIs still contain unresolved variables",
                        unresolved_count
                    );
                }

                println!();
            }
            Err(e) => {
                println!("✗ {} - ERROR: {}", relative.display(), e);
            }
        }
    }

    println!("\n=== Resolution Test Complete ===");
}
