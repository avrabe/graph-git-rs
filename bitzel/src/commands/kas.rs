//! KAS configuration-based build command
//!
//! This command builds BitBake recipes using KAS configuration files.
//! It handles repository fetching, configuration generation, and build execution.

use convenient_bitbake::{
    BuildContext, ExtractionConfig, RecipeExtractor, RecipeGraph,
    TaskImplementation,
    Pipeline, PipelineConfig,
};
use convenient_kas::{ConfigGenerator, include_graph::KasIncludeGraph};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        // Determine repo directory - either from path or from clone
        let repo_dir = if let Some(path_str) = &repo.path {
            // Local path - use it directly
            let path = Path::new(path_str);
            let abs_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                // Relative path - resolve from current directory or KAS file location
                std::env::current_dir()?.join(path)
            };
            tracing::info!("Using local repository {} from {:?}", repo_name, abs_path);
            abs_path
        } else if let Some(url) = &repo.url {
            // Remote URL - clone it
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
            repo_dir
        } else {
            tracing::warn!("Repository {} has neither path nor url, skipping", repo_name);
            continue;
        };

        // Collect layer paths for this repo
        let mut repo_layers = Vec::new();
        for (layer_name, _) in &repo.layers {
            let layer_path = repo_dir.join(layer_name);
            if layer_path.exists() {
                tracing::debug!("  Found layer: {:?}", layer_path);
                repo_layers.push(layer_path);
            } else {
                tracing::warn!("  Layer not found: {:?}", layer_path);
            }
        }
        if !repo_layers.is_empty() {
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

    // ========== Step 7: Find Target Recipe ==========
    let target_recipe = kas_config.target
        .as_ref()
        .and_then(|targets| targets.first())
        .ok_or("No target specified in KAS config")?;

    println!("🎯 Finding target recipe: {}", target_recipe);
    let recipe_id = graph.find_recipe(target_recipe)
        .ok_or_else(|| format!("Recipe '{}' not found", target_recipe))?;
    let recipe = graph.get_recipe(recipe_id)
        .ok_or_else(|| format!("Recipe '{}' not found in graph", target_recipe))?;

    println!("  Found: {} {}", recipe.name, recipe.version.as_deref().unwrap_or("unknown"));
    println!();

    // ========== Step 8: Build Task Graph ==========
    println!("🔗 Building task execution graph for {}...", target_recipe);
    use convenient_bitbake::task_graph::TaskGraphBuilder;
    let task_builder = TaskGraphBuilder::new(graph.clone());
    let task_graph = task_builder.build_full_graph()?;
    let task_stats = task_graph.stats();
    println!("  Total tasks: {}", task_stats.total_tasks);
    println!("  Root tasks: {}", task_stats.root_tasks);
    println!("  Leaf tasks: {}", task_stats.leaf_tasks);
    println!();

    // ========== Step 9: Select Random Recipes to Build ==========
    use rand::seq::SliceRandom;
    use rand::thread_rng;

    // Filter recipes that have task implementations
    let buildable_recipes: Vec<String> = recipe_task_impls.keys()
        .filter(|name| {
            let tasks = recipe_task_impls.get(*name).unwrap();
            tasks.contains_key("do_compile") || tasks.contains_key("do_install")
        })
        .cloned()
        .collect();

    println!("📊 Found {} buildable recipes with task implementations", buildable_recipes.len());

    // Randomly select 5 recipes
    let mut rng = thread_rng();
    let selected_count = 5.min(buildable_recipes.len());
    let mut selected_recipes: Vec<String> = buildable_recipes.clone();
    selected_recipes.shuffle(&mut rng);
    selected_recipes.truncate(selected_count);

    println!("🎲 Randomly selected {} recipes to build:", selected_count);
    for (i, recipe_name) in selected_recipes.iter().enumerate() {
        println!("  {}. {}", i + 1, recipe_name);
    }
    println!();

    // ========== Step 10: Execute Tasks for Selected Recipes ==========
    use convenient_bitbake::executor::{TaskExecutor, TaskSpec, NetworkPolicy, ResourceLimits};
    use std::time::Duration;

    let cache_dir = build_dir.join("bitzel-cache");
    let mut executor = TaskExecutor::new(&cache_dir)?;

    let machine = kas_config.machine.as_deref().unwrap_or("qemux86-64");
    let tmpdir = build_dir.join("tmp");
    let dl_dir = tmpdir.join("downloads");
    std::fs::create_dir_all(&dl_dir)?;

    let task_order = vec!["do_fetch", "do_unpack", "do_patch", "do_configure", "do_compile", "do_install"];

    let mut total_recipes = 0;
    let mut total_tasks_executed = 0;
    let mut total_tasks_succeeded = 0;
    let mut total_tasks_failed = 0;
    let mut successful_builds: Vec<String> = Vec::new();
    let mut failed_builds: Vec<String> = Vec::new();

    for recipe_name in &selected_recipes {
        total_recipes += 1;
        println!("\n╔════════════════════════════════════════════════════════╗");
        println!("║  Building Recipe {}/{}:  {}", total_recipes, selected_count, recipe_name);
        println!("╚════════════════════════════════════════════════════════╝\n");

        // Get recipe from graph
        let recipe_id_opt = graph.find_recipe(recipe_name);
        if recipe_id_opt.is_none() {
            println!("⚠️  Recipe {} not found in graph, skipping", recipe_name);
            failed_builds.push(recipe_name.clone());
            continue;
        }

        let recipe_id = recipe_id_opt.unwrap();
        let recipe_opt = graph.get_recipe(recipe_id);
        if recipe_opt.is_none() {
            println!("⚠️  Recipe {} data not found, skipping", recipe_name);
            failed_builds.push(recipe_name.clone());
            continue;
        }

        let recipe = recipe_opt.unwrap();
        let pv = recipe.version.as_deref().unwrap_or("unknown");

        // Get task implementations
        let tasks_opt = recipe_task_impls.get(recipe_name);
        if tasks_opt.is_none() {
            println!("⚠️  No task implementations for {}, skipping", recipe_name);
            failed_builds.push(recipe_name.clone());
            continue;
        }

        let tasks = tasks_opt.unwrap();

        // Setup work directories
        let work_base = tmpdir.join("work").join(machine).join(recipe_name).join(pv);
        let s_dir = work_base.join(format!("{}-{}", recipe_name, pv));
        let b_dir = work_base.join("build");
        let d_dir = work_base.join("image");

        std::fs::create_dir_all(&work_base)?;
        std::fs::create_dir_all(&s_dir)?;
        std::fs::create_dir_all(&b_dir)?;
        std::fs::create_dir_all(&d_dir)?;

        // Setup BitBake variables
        let mut bb_vars = std::collections::HashMap::new();
        bb_vars.insert("PN".to_string(), recipe_name.clone());
        bb_vars.insert("PV".to_string(), pv.to_string());
        bb_vars.insert("WORKDIR".to_string(), work_base.to_string_lossy().to_string());
        bb_vars.insert("S".to_string(), s_dir.to_string_lossy().to_string());
        bb_vars.insert("B".to_string(), b_dir.to_string_lossy().to_string());
        bb_vars.insert("D".to_string(), d_dir.to_string_lossy().to_string());
        bb_vars.insert("DL_DIR".to_string(), dl_dir.to_string_lossy().to_string());
        bb_vars.insert("MACHINE".to_string(), machine.to_string());
        bb_vars.insert("base_bindir".to_string(), "/bin".to_string());
        bb_vars.insert("base_sbindir".to_string(), "/sbin".to_string());
        bb_vars.insert("bindir".to_string(), "/usr/bin".to_string());
        bb_vars.insert("sbindir".to_string(), "/usr/sbin".to_string());
        bb_vars.insert("libdir".to_string(), "/usr/lib".to_string());
        bb_vars.insert("sysconfdir".to_string(), "/etc".to_string());

        let mut recipe_succeeded = 0;
        let mut recipe_failed = 0;
        let mut build_failed = false;

        for task_name in &task_order {
            if let Some(task_impl) = tasks.get(*task_name) {
                println!("  📦 {}...", task_name);

                let network_policy = if *task_name == "do_fetch" {
                    NetworkPolicy::FullNetwork
                } else {
                    NetworkPolicy::Isolated
                };

                // Use the ACTUAL task code from the recipe
                let script = &task_impl.code;

                let task_spec = TaskSpec {
                    name: task_name.to_string(),
                    recipe: recipe_name.clone(),
                    script: script.clone(),
                    workdir: work_base.clone(),
                    env: bb_vars.clone(),
                    outputs: vec![],
                    timeout: Some(Duration::from_secs(600)),
                    execution_mode: convenient_bitbake::executor::types::ExecutionMode::Shell,
                    network_policy,
                    resource_limits: ResourceLimits::default(),
                };

                total_tasks_executed += 1;
                match executor.execute_task(task_spec) {
                    Ok(output) => {
                        if output.exit_code == 0 {
                            recipe_succeeded += 1;
                            total_tasks_succeeded += 1;
                            println!("     ✓ Success ({}ms)", output.duration_ms);
                        } else {
                            recipe_failed += 1;
                            total_tasks_failed += 1;
                            build_failed = true;
                            println!("     ✗ Failed (exit {})", output.exit_code);
                            if !output.stderr.is_empty() {
                                println!("     Error (first 5 lines):");
                                for line in output.stderr.lines().take(5) {
                                    println!("       {}", line);
                                }
                            }
                            break;
                        }
                    }
                    Err(e) => {
                        recipe_failed += 1;
                        total_tasks_failed += 1;
                        build_failed = true;
                        println!("     ✗ Execution error: {}", e);
                        break;
                    }
                }
            }
        }

        if !build_failed {
            successful_builds.push(recipe_name.clone());
            println!("\n  ✅ {} built successfully ({} tasks)", recipe_name, recipe_succeeded);

            // Check for output files
            if let Ok(entries) = std::fs::read_dir(&d_dir) {
                let files: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                if !files.is_empty() {
                    println!("     Output files in {}:", d_dir.display());
                    for entry in files.iter().take(5) {
                        println!("       - {:?}", entry.file_name());
                    }
                }
            }
        } else {
            failed_builds.push(recipe_name.clone());
            println!("\n  ❌ {} build failed ({} succeeded, {} failed)",
                     recipe_name, recipe_succeeded, recipe_failed);
        }
    }

    println!("\n\n╔════════════════════════════════════════════════════════╗");
    println!("║           FINAL BUILD RESULTS                          ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!("Recipes attempted:      {}", total_recipes);
    println!("Successful builds:      {}", successful_builds.len());
    println!("Failed builds:          {}", failed_builds.len());
    println!("Total tasks executed:   {}", total_tasks_executed);
    println!("Total tasks succeeded:  {}", total_tasks_succeeded);
    println!("Total tasks failed:     {}", total_tasks_failed);
    println!();

    if !successful_builds.is_empty() {
        println!("✅ Successfully built:");
        for recipe in &successful_builds {
            println!("   • {}", recipe);
        }
        println!();
    }

    if !failed_builds.is_empty() {
        println!("❌ Failed to build:");
        for recipe in &failed_builds {
            println!("   • {}", recipe);
        }
        println!();
    }

    if successful_builds.len() == selected_count {
        println!("🎉 ALL {} RANDOMLY SELECTED RECIPES BUILT SUCCESSFULLY!", selected_count);
    } else {
        println!("⚠️  {}/{} recipes built successfully", successful_builds.len(), selected_count);
    }

    Ok(())
}
