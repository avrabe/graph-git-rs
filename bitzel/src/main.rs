//! Bitzel - Bazel-inspired build orchestrator for BitBake/Yocto projects
//!
//! Orchestrates:
//! 1. KAS configuration loading
//! 2. Repository fetching
//! 3. Recipe parsing (using convenient-bitbake)
//! 4. Dependency graph building (using RecipeGraph)
//! 5. Task graph construction (using TaskGraphBuilder)
//! 6. Cached execution (using TaskExecutor)

use convenient_bitbake::{ExtractionConfig, RecipeExtractor, RecipeGraph};
use convenient_kas::include_graph::KasIncludeGraph;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bitzel=info,convenient_bitbake=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║              BITZEL BUILD ORCHESTRATOR                 ║");
    println!("║     Using convenient-bitbake recipe & task engine     ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    // Configuration
    let kas_file = Path::new("examples/busybox-qemux86-64.yml");
    let build_dir = PathBuf::from("build");
    let repos_dir = build_dir.join("repos");

    if !kas_file.exists() {
        eprintln!("❌ Kas file not found: {}", kas_file.display());
        eprintln!("   Run this from the repository root directory");
        std::process::exit(1);
    }

    // ========== Step 1: Load KAS Configuration ==========
    tracing::info!("Loading kas configuration from {}", kas_file.display());
    let kas_config = KasIncludeGraph::build(kas_file).await?;
    let config = kas_config.merge_config();

    println!("📋 Configuration:");
    println!("  Machine:  {}", config.machine.as_deref().unwrap_or("unknown"));
    println!("  Distro:   {}", config.distro.as_deref().unwrap_or("unknown"));
    if let Some(targets) = &config.target {
        println!("  Targets:  {}", targets.join(", "));
    }
    println!();

    // ========== Step 2: Fetch Repositories ==========
    println!("📦 Fetching repositories...");
    fs::create_dir_all(&repos_dir)?;

    let mut layers = Vec::new();

    for (repo_name, repo) in &config.repos {
        if let Some(url) = &repo.url {
            let repo_dir = repos_dir.join(repo_name);

            if repo_dir.exists() {
                tracing::debug!("Repository {} already exists", repo_name);
            } else {
                let branch = repo.branch.as_deref().unwrap_or("master");
                tracing::info!("Cloning {} (branch: {})...", repo_name, branch);

                let output = Command::new("git")
                    .args(&[
                        "clone",
                        "--branch",
                        branch,
                        "--depth",
                        "1",
                        url,
                        repo_dir.to_str().unwrap(),
                    ])
                    .output()?;

                if !output.status.success() {
                    tracing::error!(
                        "Failed to clone {}: {}",
                        repo_name,
                        String::from_utf8_lossy(&output.stderr)
                    );
                    continue;
                }
            }

            // Collect layer paths
            for (layer_name, _) in &repo.layers {
                let layer_path = repo_dir.join(layer_name);
                if layer_path.exists() {
                    layers.push(layer_path);
                }
            }
        }
    }

    println!("  Found {} layers", layers.len());
    println!();

    // ========== Step 3: Parse Recipes Using RecipeExtractor ==========
    println!("🔍 Parsing BitBake recipes...");

    let mut extraction_config = ExtractionConfig::default();
    extraction_config.extract_tasks = true;
    extraction_config.resolve_providers = true;
    extraction_config.use_python_executor = false; // Skip Python execution for speed

    let extractor = RecipeExtractor::new(extraction_config);
    let mut graph = RecipeGraph::new();
    let mut extractions = Vec::new();
    let mut recipe_count = 0;

    // Scan all layers for .bb files
    for layer_path in &layers {
        let recipes = find_recipes(layer_path);
        tracing::info!(
            "Found {} recipes in {}",
            recipes.len(),
            layer_path.file_name().unwrap().to_string_lossy()
        );

        for recipe_path in recipes {
            if let Ok(content) = fs::read_to_string(&recipe_path) {
                // Extract recipe name from filename (e.g., "busybox_1.35.0.bb" -> "busybox")
                let recipe_name = recipe_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.split('_').next())
                    .unwrap_or("unknown")
                    .to_string();

                match extractor.extract_from_content(&mut graph, &recipe_name, &content) {
                    Ok(extraction) => {
                        recipe_count += 1;
                        extractions.push(extraction);
                    }
                    Err(e) => {
                        tracing::debug!("Skipping {}: {}", recipe_name, e);
                    }
                }
            }
        }
    }

    println!("  Extracted {} recipes", recipe_count);
    println!();

    // ========== Step 4: Populate Dependencies ==========
    println!("🔗 Resolving dependencies...");
    extractor.populate_dependencies(&mut graph, &extractions)?;

    let stats = graph.statistics();
    println!("  Recipes:       {}", stats.recipe_count);
    println!("  Tasks:         {}", stats.task_count);
    println!("  Dependencies:  {}", stats.total_dependencies);
    println!("  Providers:     {}", stats.provider_count);
    println!();

    // ========== Step 5: Build Execution Plan ==========
    println!("📊 Building execution plan...");

    // Get topological sort of recipes
    let build_order = match graph.topological_sort() {
        Ok(order) => order,
        Err(e) => {
            eprintln!("❌ Cannot build - circular dependencies detected!");
            let cycles = graph.detect_cycles();
            if !cycles.is_empty() {
                eprintln!("\nCircular dependency cycles found:");
                for cycle in cycles.iter().take(5) {
                    // Convert RecipeId to names
                    let names: Vec<String> = cycle.iter()
                        .filter_map(|id| graph.get_recipe(*id).map(|r| r.name.clone()))
                        .collect();
                    if !names.is_empty() {
                        eprintln!("  • {}", names.join(" → "));
                    }
                }
            }
            return Err(e.into());
        }
    };

    println!("  Build order: {} recipes", build_order.len());

    // Show build plan for target recipes
    if let Some(targets) = &config.target {
        println!("\n📋 Target recipes and their dependencies:");
        for target in targets {
            if let Some(recipe_id) = graph.find_recipe(target) {
                let deps = graph.get_all_dependencies(recipe_id);
                println!("\n  {} requires {} dependencies:", target, deps.len());

                // Show first 10 dependencies
                for dep_id in deps.iter().take(10) {
                    if let Some(dep_recipe) = graph.get_recipe(*dep_id) {
                        println!("    • {}", dep_recipe.name);
                    }
                }
                if deps.len() > 10 {
                    println!("    ... and {} more", deps.len() - 10);
                }
            } else {
                println!("  ⚠️  {} not found in layers", target);
            }
        }
    }

    println!("\n✅ Build plan complete!");
    println!("\n💡 Next steps:");
    println!("  1. Use TaskGraphBuilder to create executable task graph");
    println!("  2. Use AsyncTaskExecutor for parallel, cached, hermetic execution");
    println!("  3. All components from convenient-bitbake are ready to use\n");

    Ok(())
}

/// Recursively find all .bb recipe files in a layer
fn find_recipes(layer: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(layer)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("bb"))
        .map(|e| e.path().to_path_buf())
        .collect()
}
