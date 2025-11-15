//! Native BitBake build command
//!
//! This command builds BitBake recipes using standard BitBake configuration
//! from a build directory (conf/bblayers.conf and conf/local.conf).

use convenient_bitbake::{
    BuildEnvironment, ExtractionConfig, RecipeExtractor,
    Pipeline, PipelineConfig, TaskImplementation, PythonExecutor,
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
    // Prefer exact match, then prefix match
    let recipe = graph.recipes()
        .find(|r| r.name == target)
        .or_else(|| {
            // Strip version suffix from target (e.g., "busybox_1.37.0" -> "busybox")
            let target_base = target.split('_').next().unwrap_or(target);
            graph.recipes().find(|r| r.name == target_base)
        })
        .or_else(|| {
            // Fall back to starts_with match
            graph.recipes().find(|r| r.name.starts_with(target))
        });

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

        // ========== Step 5: Create Work Directories ==========
        println!("🚀 Setting up build environment:");
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

        // Get machine architecture
        let machine = env.get_machine().unwrap_or("unknown");

        // Create per-recipe work directory: tmp/work/MACHINE/RECIPE/VERSION/
        let recipe_workdir = env.tmpdir.join(format!("work/{}/{}/{}", machine, recipe.name, version));
        std::fs::create_dir_all(&recipe_workdir)?;
        std::fs::create_dir_all(recipe_workdir.join("temp"))?;  // For logs

        println!("  Created build directories:");
        println!("    ✓ {:?}", env.dl_dir);
        println!("    ✓ {:?}", env.tmpdir.join("work"));
        println!("    ✓ {:?}", recipe_workdir);
        println!("    ✓ {:?}", env.tmpdir.join("deploy"));
        println!("    ✓ {:?}", env.tmpdir.join("stamps"));
        println!();

        // ========== Step 6: Get Task Implementation ==========
        println!("🔍 Looking for task implementation...");

        // Find any task implementation for this recipe (try do_fetch first, then do_compile, then any)
        let (task_name, task_impl) = recipe_task_impls.get(&recipe.name)
            .and_then(|impls| {
                impls.get("do_fetch").map(|t| ("do_fetch", t))
                    .or_else(|| impls.get("do_compile").map(|t| ("do_compile", t)))
                    .or_else(|| impls.iter().next().map(|(name, t)| (name.as_str(), t)))
            })
            .map(|(name, impl_ref)| (name.to_string(), impl_ref.clone()))
            .unzip();

        if let (Some(task_name), Some(task_impl)) = (task_name, task_impl) {
            println!("  Found {} implementation for {}",
                     match task_impl.impl_type {
                         convenient_bitbake::TaskImplementationType::Shell => "shell",
                         convenient_bitbake::TaskImplementationType::Python => "Python",
                         convenient_bitbake::TaskImplementationType::FakerootShell => "fakeroot shell",
                     }, task_name);
            println!("  Code length: {} bytes", task_impl.code.len());
            println!();

            // ========== Step 7: Execute Task ==========
            println!("⚡ Executing {}...", task_name);

            // Build initial variables for execution
            let mut initial_vars = HashMap::new();
            initial_vars.insert("PN".to_string(), recipe.name.clone());
            initial_vars.insert("PV".to_string(), version.to_string());
            initial_vars.insert("WORKDIR".to_string(), recipe_workdir.to_string_lossy().to_string());
            initial_vars.insert("B".to_string(), recipe_workdir.to_string_lossy().to_string());  // Build directory
            initial_vars.insert("S".to_string(), recipe_workdir.to_string_lossy().to_string());  // Source directory
            initial_vars.insert("D".to_string(), recipe_workdir.join("image").to_string_lossy().to_string());  // Destination/install directory
            initial_vars.insert("DL_DIR".to_string(), env.dl_dir.to_string_lossy().to_string());
            initial_vars.insert("TMPDIR".to_string(), env.tmpdir.to_string_lossy().to_string());
            if let Some(machine) = env.get_machine() {
                initial_vars.insert("MACHINE".to_string(), machine.to_string());
            }
            if let Some(distro) = env.get_distro() {
                initial_vars.insert("DISTRO".to_string(), distro.to_string());
                initial_vars.insert("DISTRO_NAME".to_string(), distro.to_string());
                initial_vars.insert("DISTRO_VERSION".to_string(), "unknown".to_string());
            }
            // Add OS_RELEASE specific variables
            initial_vars.insert("OS_RELEASE_FIELDS".to_string(), "ID NAME VERSION PRETTY_NAME".to_string());
            initial_vars.insert("OS_RELEASE_UNQUOTED_FIELDS".to_string(), "ID VERSION_ID".to_string());

            // Execute based on task type
            match task_impl.impl_type {
                convenient_bitbake::TaskImplementationType::Python => {
                    println!("  Executing Python task with RustPython...");
                    let executor = PythonExecutor::new();
                    let result = executor.execute(&task_impl.code, &initial_vars);

                    if result.success {
                        println!("  ✓ Python task succeeded");
                        println!("    Variables set: {}", result.variables_set.len());
                        for (key, value) in result.variables_set.iter().take(5) {
                            println!("      {} = {}", key, value);
                        }
                        if result.variables_set.len() > 5 {
                            println!("      ... and {} more", result.variables_set.len() - 5);
                        }

                        // Create stamp file
                        let stamp_dir = env.tmpdir.join("stamps").join(machine).join(recipe.name.clone());
                        std::fs::create_dir_all(&stamp_dir)?;
                        let stamp_file = stamp_dir.join(format!("{}.{}", task_name, version));
                        std::fs::write(&stamp_file, "")?;
                        println!("  ✓ Created stamp file: {:?}", stamp_file);
                    } else {
                        eprintln!("  ✗ Python task failed: {:?}", result.error);
                        return Err(format!("{} failed: {:?}", task_name, result.error).into());
                    }
                }
                convenient_bitbake::TaskImplementationType::Shell |
                convenient_bitbake::TaskImplementationType::FakerootShell => {
                    println!("  ⚠️  Shell task execution not yet implemented");
                    println!("     Task would execute: {} bytes of shell code", task_impl.code.len());
                    println!("     Working directory: {:?}", recipe_workdir);
                    println!();
                    println!("     Next: Implement shell task execution");
                }
            }
        } else {
            println!("  ⚠️  No do_fetch implementation found for {}", recipe.name);
            println!("     This is expected for recipes that inherit do_fetch");
            println!("     Task graph shows {} total tasks available", recipe_tasks.len());
        }
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
