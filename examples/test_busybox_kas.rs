//! Test program to verify kas parsing for busybox example
//!
//! This demonstrates the complete kas workflow:
//! 1. Load kas configuration
//! 2. Build include graph
//! 3. Merge configurations
//! 4. Display parsed config

use convenient_kas::include_graph::{KasFile, KasIncludeGraph};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let kas_path = Path::new("examples/busybox-qemux86-64.yml");

    println!("=== Testing Kas Configuration Parsing ===\n");
    println!("Loading: {}", kas_path.display());

    // Step 1: Load the kas file
    let kas_file = KasFile::load(kas_path).await?;
    println!("✓ Successfully loaded kas file");
    println!("  Checksum: {}\n", kas_file.checksum);

    // Step 2: Build include graph
    println!("Building include graph...");
    let graph = KasIncludeGraph::build(kas_path).await?;
    println!("✓ Include graph built successfully");
    println!("  Files in graph: {}\n", graph.files().len());

    // Step 3: Merge configurations
    println!("Merging configurations...");
    let merged = graph.merge_config();
    println!("✓ Configuration merged\n");

    // Step 4: Display parsed configuration
    println!("=== Parsed Configuration ===\n");
    println!("Header version: {}", merged.header.version);
    println!("Machine: {}", merged.machine.as_ref().unwrap_or(&"<none>".to_string()));
    println!("Distro: {}", merged.distro.as_ref().unwrap_or(&"<none>".to_string()));

    if let Some(targets) = &merged.target {
        println!("Targets: {}", targets.join(", "));
    }

    println!("\nRepositories:");
    for (name, repo) in &merged.repos {
        println!("  - {}", name);
        if let Some(url) = &repo.url {
            println!("    URL: {}", url);
        }
        if let Some(branch) = &repo.branch {
            println!("    Branch: {}", branch);
        }
        if !repo.layers.is_empty() {
            println!("    Layers:");
            for (layer_name, _) in &repo.layers {
                println!("      - {}", layer_name);
            }
        }
    }

    if let Some(env) = &merged.env {
        println!("\nEnvironment variables:");
        for (key, value) in env {
            println!("  {}={}", key, value);
        }
    }

    if let Some(headers) = &merged.local_conf_header {
        println!("\nLocal conf headers: {} section(s)", headers.len());
    }

    if let Some(headers) = &merged.bblayers_conf_header {
        println!("BBLayers conf headers: {} section(s)", headers.len());
    }

    println!("\n=== Combined Checksum ===");
    println!("{}", graph.combined_checksum());

    println!("\n✓ All tests passed!");

    Ok(())
}
