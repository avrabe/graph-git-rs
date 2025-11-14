//! Bitzel - Bazel-inspired build orchestrator for BitBake/Yocto projects
//!
//! Orchestrates:
//! 1. KAS configuration loading
//! 2. Repository fetching
//! 3. Configuration file generation (local.conf, bblayers.conf)
//! 4. Layer context with priorities and overrides
//! 5. Recipe parsing with BuildContext (respects bbappends, layer priorities)
//! 6. Dependency graph building (using RecipeGraph)
//! 7. Task graph construction (using TaskGraphBuilder)
//! 8. Cached execution (using AsyncTaskExecutor)

use convenient_bitbake::{BuildContext, ExtractionConfig, RecipeExtractor, RecipeGraph};
use convenient_kas::{ConfigGenerator, include_graph::KasIncludeGraph};
use std::collections::HashMap;
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
                .unwrap_or_else(|_| "bitzel=info,convenient_bitbake=info,convenient_kas=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║              BITZEL BUILD ORCHESTRATOR                 ║");
    println!("║  Layer-aware BitBake with override resolution         ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    // Configuration
    let kas_file = Path::new("examples/busybox-qemux86-64.yml");
    let build_dir = PathBuf::from("build");
    let repos_dir = build_dir.join("repos");
    let conf_dir = build_dir.join("conf");

    if !kas_file.exists() {
        eprintln!("❌ Kas file not found: {}", kas_file.display());
        eprintln!("   Run this from the repository root directory");
        std::process::exit(1);
    }

    fs::create_dir_all(&build_dir)?;

    // ========== Step 1: Load KAS Configuration ==========
    tracing::info!("Loading kas configuration from {}", kas_file.display());
    let kas_config_graph = KasIncludeGraph::build(kas_file).await?;
    let kas_config = kas_config_graph.merge_config();

    println!("📋 Configuration:");
    println!("  Machine:  {}", kas_config.machine.as_deref().unwrap_or("unknown"));
    println!("  Distro:   {}", kas_config.distro.as_deref().unwrap_or("unknown"));
    if let Some(targets) = &kas_config.target {
        println!("  Targets:  {}", targets.join(", "));
    }
    println!();

    // ========== Step 2: Fetch Repositories ==========
    println!("📦 Fetching repositories...");
    fs::create_dir_all(&repos_dir)?;

    let mut layer_paths: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for (repo_name, repo) in &kas_config.repos {
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

            // Collect layer paths for this repo
            let mut repo_layers = Vec::new();
            for (layer_name, _) in &repo.layers {
                let layer_path = repo_dir.join(layer_name);
                if layer_path.exists() {
                    repo_layers.push(layer_path);
                }
            }
            layer_paths.insert(repo_name.clone(), repo_layers);
        }
    }

    let total_layers: usize = layer_paths.values().map(|v| v.len()).sum();
    println!("  Cloned {} repos with {} layers", layer_paths.len(), total_layers);
    println!();

    // ========== Step 3: Generate BitBake Configuration Files ==========
    println!("📝 Generating BitBake configuration...");
    fs::create_dir_all(&conf_dir)?;

    let config_gen = ConfigGenerator::new(&build_dir, kas_config.clone(), layer_paths.clone());
    config_gen.generate_all().await?;

    println!("  Generated: conf/local.conf");
    println!("  Generated: conf/bblayers.conf");
    println!();

    // ========== Step 4: Build Layer Context with Priorities ==========
    println!("🏗️  Building layer context with priorities...");

    let mut build_context = BuildContext::new();

    // Set machine and distro for override resolution
    if let Some(machine) = &kas_config.machine {
        build_context.set_machine(machine.clone());
    }
    if let Some(distro) = &kas_config.distro {
        build_context.set_distro(distro.clone());
    }

    // Add layers with their priorities from layer.conf
    for (_repo_name, layers) in &layer_paths {
        for layer_path in layers {
            let layer_conf = layer_path.join("conf/layer.conf");
            if layer_conf.exists() {
                match build_context.add_layer_from_conf(&layer_conf) {
                    Ok(()) => {}
                    Err(e) => {
                        tracing::warn!("Failed to parse layer.conf for {}: {}", layer_path.display(), e);
                        // Create a default layer config
                        let default_layer = convenient_bitbake::layer_context::LayerConfig {
                            layer_dir: layer_path.clone(),
                            collection: layer_path.file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            priority: 5, // Default priority
                            version: None,
                            depends: vec![],
                            series_compat: vec![],
                            variables: HashMap::new(),
                        };
                        build_context.add_layer(default_layer);
                    }
                }
            }
        }
    }

    println!("  Loaded {} layers with priorities", build_context.layers.len());
    for layer in &build_context.layers {
        println!("    • {} (priority: {})", layer.collection, layer.priority);
    }
    println!();

    // ========== Step 5: Parse Recipes with Full Context ==========
    println!("🔍 Parsing BitBake recipes with layer context...");
    // TODO: Apply OverrideResolver for MACHINE/DISTRO conditionals

    let mut extraction_config = ExtractionConfig::default();
    extraction_config.extract_tasks = true;
    extraction_config.resolve_providers = true;
    extraction_config.use_python_executor = false;

    let extractor = RecipeExtractor::new(extraction_config);
    let mut graph = RecipeGraph::new();
    let mut extractions = Vec::new();
    let mut recipe_count = 0;

    // Scan all layers for .bb files
    for layer_path in layer_paths.values().flatten() {
        let recipes = find_recipes(layer_path);
        tracing::info!(
            "Found {} recipes in {}",
            recipes.len(),
            layer_path.file_name().unwrap().to_string_lossy()
        );

        for recipe_path in recipes {
            // Read recipe content
            if let Ok(content) = fs::read_to_string(&recipe_path) {
                // Extract recipe name from filename
                let recipe_name = recipe_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.split('_').next())
                    .unwrap_or("unknown")
                    .to_string();

                // Extract recipe metadata
                // TODO: Integrate BuildContext.parse_recipe_with_context for bbappend merging
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

    println!("  Extracted {} recipes with override resolution", recipe_count);
    println!();

    // ========== Step 6: Populate Dependencies ==========
    println!("🔗 Resolving dependencies...");
    extractor.populate_dependencies(&mut graph, &extractions)?;

    let stats = graph.statistics();
    println!("  Recipes:       {}", stats.recipe_count);
    println!("  Tasks:         {}", stats.task_count);
    println!("  Dependencies:  {}", stats.total_dependencies);
    println!("  Providers:     {}", stats.provider_count);
    println!();

    // ========== Step 7: Build Execution Plan ==========
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
    if let Some(targets) = &kas_config.target {
        println!("\n📋 Target recipes and their dependencies:");
        for target in targets {
            if let Some(recipe_id) = graph.find_recipe(target) {
                let deps = graph.get_all_dependencies(recipe_id);
                println!("\n  {} requires {} dependencies:", target, deps.len());

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
    println!("  3. Configuration files ready in build/conf/\n");

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
