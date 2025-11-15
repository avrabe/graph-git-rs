//! Native BitBake build command
//!
//! This command builds BitBake recipes using standard BitBake configuration
//! from a build directory (conf/bblayers.conf and conf/local.conf).

use convenient_bitbake::{
    BuildEnvironment, ExtractionConfig, RecipeExtractor,
    Pipeline, PipelineConfig,
};
use std::collections::HashMap;
use std::path::Path;

/// Execute build using native BitBake configuration
pub async fn execute(
    build_dir: &Path,
    target: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🏗️  Loading BitBake build environment...");

    // Load build environment from build directory
    let env = BuildEnvironment::from_build_dir(build_dir)?;

    println!("📋 Configuration:");
    println!("  TOPDIR:      {:?}", env.topdir);
    println!("  MACHINE:     {:?}", env.get_machine().unwrap_or("unknown"));
    println!("  DISTRO:      {:?}", env.get_distro().unwrap_or("unknown"));
    println!("  DL_DIR:      {:?}", env.dl_dir);
    println!("  TMPDIR:      {:?}", env.tmpdir);
    println!("  Layers:      {}", env.layers.len());
    println!();

    println!("🏗️  Layers:");
    for (i, layer) in env.layers.iter().enumerate() {
        println!("  {}. {:?}", i + 1, layer);
    }
    println!();

    // Create build context from environment
    println!("🔧 Creating build context...");
    let build_context = env.create_build_context()?;

    println!("  Loaded {} layers with priorities", build_context.layers.len());
    for layer in &build_context.layers {
        println!("    • {} (priority: {})", layer.collection, layer.priority);
    }
    println!();

    // ========== Step 1: Parse Recipes with Parallel Pipeline ==========
    println!("🔍 Discovering and parsing BitBake recipes...");

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

    // Create layer_paths map from environment layers
    let mut layer_paths: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();
    for (i, layer) in env.layers.iter().enumerate() {
        let layer_name = format!("layer{}", i);
        layer_paths.insert(layer_name, vec![layer.clone()]);
    }

    // Stage 1: Discover recipes in parallel
    let (recipe_files, discover_hash) = pipeline.discover_recipes(&layer_paths).await?;
    pipeline.save_stage_hash(&discover_hash).await?;

    println!("  Discovered {} recipe files", recipe_files.len());

    // Stage 2: Parse recipes in parallel
    let (parsed_recipes, parse_hash) = pipeline.parse_recipes(recipe_files).await?;
    pipeline.save_stage_hash(&parse_hash).await?;

    println!("  Parsed {} recipes", parsed_recipes.len());
    println!();

    // ========== Step 2: Build Recipe Graph ==========
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

    // ========== Step 3: Find Target Recipe ==========
    println!("🎯 Looking for target recipe: {}", target);

    // Find the recipe in the graph
    let recipe = graph.recipes()
        .find(|r| r.name == target || r.name.starts_with(target));

    if let Some(recipe) = recipe {
        let version = recipe.version.as_deref().unwrap_or("unknown");
        println!("  Found recipe: {} {}", recipe.name, version);
        println!();

        // ========== Step 4: Show Recipe Tasks ==========
        println!("📊 Analyzing recipe tasks...");

        // Get tasks from recipe graph
        let recipe_tasks = graph.get_recipe_tasks(recipe.id);
        println!("  Found {} tasks for {}", recipe_tasks.len(), recipe.name);
        println!();

        // Show tasks that would be executed
        println!("  Tasks to execute:");
        for (i, task) in recipe_tasks.iter().enumerate().take(15) {
            let network = if task.name.contains("fetch") { "network" } else { "isolated" };
            println!("    {}. {} [{}]", i + 1, task.name, network);
        }
        if recipe_tasks.len() > 15 {
            println!("    ... and {} more tasks", recipe_tasks.len() - 15);
        }
        println!();

        // ========== Step 5: Show Build Plan ==========
        println!("🚀 Build Plan:");
        println!("   Target: {} {}", recipe.name, version);
        println!("   DL_DIR: {:?}", env.dl_dir);
        println!("   TMPDIR: {:?}", env.tmpdir);
        println!();

        // Create necessary directories
        std::fs::create_dir_all(&env.dl_dir)?;
        std::fs::create_dir_all(&env.tmpdir)?;
        std::fs::create_dir_all(env.tmpdir.join("work"))?;
        std::fs::create_dir_all(env.tmpdir.join("deploy"))?;
        std::fs::create_dir_all(env.tmpdir.join("stamps"))?;

        println!("  Created build directories:");
        println!("    ✓ {:?}", env.dl_dir);
        println!("    ✓ {:?}", env.tmpdir.join("work"));
        println!("    ✓ {:?}", env.tmpdir.join("deploy"));
        println!("    ✓ {:?}", env.tmpdir.join("stamps"));
        println!();

        println!("⚠️  Note: Task execution not yet implemented");
        println!("   Next steps to complete:");
        println!("     • Extract task scripts from parsed recipes");
        println!("     • Execute Python functions via RustPython");
        println!("     • Set up per-recipe work directories");
        println!("     • Manage stamp files for incremental builds");
        println!("     • Handle task dependencies and execution order");
        println!();

        println!("✅ Build infrastructure ready!");
        println!("   {} tasks identified for {}", recipe_tasks.len(), recipe.name);
        println!("   Environment configured for native BitBake builds");
    } else {
        eprintln!("❌ Recipe not found: {}", target);
        eprintln!("   Available recipes:");
        for recipe in graph.recipes().take(10) {
            eprintln!("     • {}", recipe.name);
        }
        return Err(format!("Recipe not found: {}", target).into());
    }

    Ok(())
}
