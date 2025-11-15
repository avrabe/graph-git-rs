//! KAS configuration-based build command
//!
//! This command builds BitBake recipes using KAS configuration files.
//! It handles repository fetching, configuration generation, and build execution.

use convenient_bitbake::{
    BuildContext, ExtractionConfig, RecipeExtractor, RecipeGraph,
    TaskGraphBuilder, InteractiveExecutor, InteractiveOptions, TaskSpec,
    TaskExtractor, TaskImplementation,
    Pipeline, PipelineConfig,
    SignatureCache, SignatureStats, EnhancedTaskSignature,
};
use convenient_bitbake::executor::types::{NetworkPolicy, ResourceLimits};
use convenient_kas::{ConfigGenerator, include_graph::KasIncludeGraph};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Execute build using KAS configuration
pub async fn execute(
    kas_file: &Path,
    build_dir: &Path,
    _target: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let repos_dir = build_dir.join("repos");
    let conf_dir = build_dir.join("conf");

    if !kas_file.exists() {
        eprintln!("❌ KAS file not found: {}", kas_file.display());
        return Err("KAS file not found".into());
    }

    fs::create_dir_all(build_dir)?;

    // ========== Step 1: Load KAS Configuration ==========
    tracing::info!("Loading KAS configuration from {}", kas_file.display());
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

    let config_gen = ConfigGenerator::new(build_dir, kas_config.clone(), layer_paths.clone());
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

    // ========== Step 5: Parse Recipes with Parallel Pipeline ==========
    println!("🔍 Parsing BitBake recipes with parallel pipeline...");

    // Configure parallel pipeline
    let pipeline_config = PipelineConfig {
        max_io_parallelism: 32,
        max_cpu_parallelism: num_cpus::get(),
        enable_cache: true,
        cache_dir: build_dir.join("bitzel-cache/pipeline"),
    };

    println!("  Parallelism: {} I/O tasks, {} CPU cores",
             pipeline_config.max_io_parallelism,
             pipeline_config.max_cpu_parallelism);

    let pipeline = Pipeline::new(pipeline_config, build_context);

    // Stage 1: Discover recipes in parallel
    let (recipe_files, discover_hash) = pipeline.discover_recipes(&layer_paths).await?;
    pipeline.save_stage_hash(&discover_hash).await?;

    // Stage 2: Parse recipes in parallel
    let (parsed_recipes, parse_hash) = pipeline.parse_recipes(recipe_files).await?;
    pipeline.save_stage_hash(&parse_hash).await?;

    // Extract task implementations from parsed recipes
    let mut recipe_task_impls: HashMap<String, HashMap<String, TaskImplementation>> = HashMap::new();
    for parsed in &parsed_recipes {
        if !parsed.task_impls.is_empty() {
            recipe_task_impls.insert(parsed.file.name.clone(), parsed.task_impls.clone());
        }
    }

    println!("  Extracted {} task implementations from {} recipes",
             recipe_task_impls.values().map(|m| m.len()).sum::<usize>(),
             recipe_task_impls.len());
    println!();

    // ========== Step 6: Build Recipe Graph ==========
    println!("🔗 Building recipe dependency graph...");

    let mut extraction_config = ExtractionConfig::default();
    extraction_config.extract_tasks = true;
    extraction_config.resolve_providers = true;
    extraction_config.use_python_executor = false;

    let extractor = RecipeExtractor::new(extraction_config);

    // Stage 3: Build recipe graph (sequential - needs all recipes)
    let (graph, graph_hash) = pipeline.build_recipe_graph(&parsed_recipes, &extractor)?;
    pipeline.save_stage_hash(&graph_hash).await?;

    let stats = graph.statistics();
    println!("  Recipes:       {}", stats.recipe_count);
    println!("  Tasks:         {}", stats.task_count);
    println!("  Dependencies:  {}", stats.total_dependencies);
    println!("  Providers:     {}", stats.provider_count);
    println!();

    // ========== Execution would continue here ==========
    println!("✅ KAS build configured successfully!");
    println!("   (Execution phase not yet implemented)");

    Ok(())
}
