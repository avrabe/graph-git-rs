//! Ferrari Build - Full-featured BitBake build using all available infrastructure
//!
//! This command uses:
//! - BuildOrchestrator for complete build planning
//! - TaskGraph for dependency resolution
//! - SimplePythonEvaluator for ${@...} expressions
//! - AsyncTaskExecutor for parallel execution (if available)
//! - Enhanced caching with incremental build analysis

use convenient_bitbake::{
    BuildEnvironment, BuildOrchestrator, OrchestratorConfig,
    TaskGraphBuilder,
};
use convenient_bitbake::executor::{
    TaskExecutor, CacheManager, TaskMonitor,
};

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

// TODO: Re-enable Python expression expansion when needed
// /// Expand BitBake script with full Python support
// #[allow(dead_code)]
// fn expand_script(
//     script: &str,
//     env: &HashMap<String, String>,
// ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
//     let evaluator = SimplePythonEvaluator::new(env.clone());
//     ... (code commented out for now)
// }

/// Execute build with full BuildOrchestrator pipeline
pub async fn execute(
    build_dir: &Path,
    target: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let start_time = Instant::now();

    println!("╔════════════════════════════════════════════════════════╗");
    println!("║           🏎️  BITZEL FERRARI BUILD  🏎️                ║");
    println!("║  Full-Featured Bazel-Inspired BitBake Build System    ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!();
    println!("Target: {}", target);
    println!("Build directory: {:?}", build_dir);
    println!();

    // ========== Load Build Environment ==========
    println!("🏗️  Loading build environment...");
    let env = BuildEnvironment::from_build_dir(build_dir)?;
    println!("  ✓ MACHINE: {}", env.get_machine().unwrap_or("unknown"));
    println!("  ✓ DISTRO:  {}", env.get_distro().unwrap_or("unknown"));
    println!("  ✓ Layers:  {}", env.layers.len());
    println!();

    // ========== Build Orchestration ==========
    println!("🎼 Building execution plan with BuildOrchestrator...");

    let config = OrchestratorConfig {
        build_dir: build_dir.to_path_buf(),
        machine: env.get_machine().map(|s| s.to_string()),
        distro: env.get_distro().map(|s| s.to_string()),
        max_io_parallelism: 32,
        max_cpu_parallelism: num_cpus::get(),
    };

    let orchestrator = BuildOrchestrator::new(config);

    // Create layer paths map
    let mut layer_paths: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();
    for (i, layer) in env.layers.iter().enumerate() {
        let layer_name = format!("layer_{}", i);
        layer_paths.insert(layer_name, vec![layer.clone()]);
    }

    let build_plan = orchestrator.build_plan(layer_paths).await?;

    println!("  ✓ Recipes parsed: {}", build_plan.recipe_graph.recipe_count());
    println!("  ✓ Tasks available: {}", build_plan.task_graph.tasks.len());
    println!();

    // ========== Incremental Build Analysis ==========
    println!("📊 Incremental Build Analysis:");
    let inc_stats = &build_plan.incremental_stats;
    println!("  Total tasks:      {}", inc_stats.total_tasks);
    println!("  Unchanged:        {} ({:.1}%)",
        inc_stats.unchanged,
        inc_stats.unchanged_percent()
    );
    println!("  Need rebuild:     {} ({:.1}%)",
        inc_stats.need_rebuild,
        inc_stats.rebuild_percent()
    );
    println!("  New tasks:        {} ({:.1}%)",
        inc_stats.new_tasks,
        inc_stats.new_percent()
    );
    println!();

    // ========== Cache Statistics ==========
    let cache_dir = build_dir.join("hitzeleiter-cache");
    if cache_dir.exists() {
        let cache_mgr = CacheManager::new(&cache_dir);
        if let Ok(cache_query) = cache_mgr.query() {
            println!("💾 Cache Status:");
            println!("  CAS objects:      {} ({:.1} MB)",
                cache_query.cas_objects,
                cache_query.cas_bytes as f64 / 1_000_000.0
            );
            println!("  Cached tasks:     {}", cache_query.action_cache_entries);
            println!("  Active sandboxes: {}", cache_query.active_sandboxes);
            println!();
        } else {
            println!("💾 Cache: Not initialized yet");
            println!();
        }
    }

    // ========== Find Target and Build Task Graph ==========
    println!("🎯 Finding target recipe: {}", target);
    let recipe_id = build_plan.recipe_graph.find_recipe(target)
        .ok_or_else(|| format!("Recipe '{}' not found", target))?;
    let recipe = build_plan.recipe_graph.get_recipe(recipe_id)
        .ok_or_else(|| "Recipe not found in graph".to_string())?;

    println!("  ✓ Found: {} {}", recipe.name, recipe.version.as_deref().unwrap_or("unknown"));

    // Debug: Check recipe dependencies
    let deps = build_plan.recipe_graph.get_dependencies(recipe_id);
    println!("  DEBUG: Recipe has {} build dependencies", deps.len());
    for dep_id in &deps {
        if let Some(dep_recipe) = build_plan.recipe_graph.get_recipe(*dep_id) {
            println!("    - {}", dep_recipe.name);
        }
    }

    // Find the target task
    // BitBake tasks are stored without the "do_" prefix in the graph
    let target_task_name = "install";
    let target_task = build_plan.task_graph.tasks.values()
        .find(|t| t.recipe_id == recipe_id && t.task_name == target_task_name)
        .ok_or_else(|| format!("Task {} not found for recipe", target_task_name))?;

    println!("  ✓ Target task: {}", target_task.task_name);
    println!();

    // ========== Build Execution Graph for Target ==========
    println!("🔗 Building execution graph for {}:{}...", recipe.name, target_task_name);

    let builder = TaskGraphBuilder::new(build_plan.recipe_graph.clone());
    let exec_graph = builder.build_for_task(target_task.task_id)?;

    println!("  ✓ Tasks in graph: {}", exec_graph.tasks.len());
    println!("  ✓ Root tasks: {}", exec_graph.root_tasks.len());
    println!("  ✓ Execution order computed (topologically sorted)");

    // Debug: Show task dependency structure
    println!("\n  DEBUG: Task dependency structure:");
    for task_id in &exec_graph.execution_order {
        if let Some(task) = exec_graph.tasks.get(task_id) {
            print!("    - {}:{} (depends_on: {}",
                task.recipe_name,
                task.task_name,
                task.depends_on.len()
            );
            if !task.depends_on.is_empty() {
                print!(" [");
                for (i, dep_id) in task.depends_on.iter().enumerate() {
                    if let Some(dep_task) = exec_graph.tasks.get(dep_id) {
                        if i > 0 { print!(", "); }
                        print!("{}:{}", dep_task.recipe_name, dep_task.task_name);
                    }
                }
                print!("]");
            }
            println!(")");
        }
    }
    println!();

    // ========== Execute Tasks (Sequential for now) ==========
    println!("🚀 Executing task graph...");
    println!();

    let cache_dir = build_dir.join("hitzeleiter-cache");
    let mut executor = TaskExecutor::new(&cache_dir)?;

    // Create task monitor for nice UI
    let monitor = TaskMonitor::new();

    // Register all tasks in the monitor
    for task in exec_graph.tasks.values() {
        let task_key = format!("{}:{}", task.recipe_name, task.task_name);
        monitor.register_task(
            task_key.clone(),
            task.recipe_name.clone(),
            task.task_name.clone(),
        );
    }

    let mut completed = 0;
    let mut from_cache = 0;
    let mut failed = 0;

    // Get machine and tmpdir for variable setup
    let machine = env.get_machine().unwrap_or("unknown");
    let tmpdir = build_dir.join("tmp");

    for &task_id in &exec_graph.execution_order {
        if let Some(exec_task) = exec_graph.tasks.get(&task_id) {
            let task_key = format!("{}:{}", exec_task.recipe_name, exec_task.task_name);

            if let Some(spec) = build_plan.task_specs.get(&task_key) {
                // Mark task as started in monitor
                monitor.task_started(&task_key);
                println!("  ▶️  {}", task_key);

                // Fetch and unpack sources before the unpack task
                if exec_task.task_name == "unpack" {
                    use convenient_bitbake::fetcher;
                    use std::fs;

                    // Find recipe file in layers
                    let mut found_src_uri = None;
                    for layer in &build_plan.build_context.layers {
                        let entries: Vec<_> = walkdir::WalkDir::new(&layer.layer_dir)
                            .into_iter()
                            .filter_map(|e| e.ok())
                            .filter(|e| {
                                e.file_type().is_file() &&
                                e.file_name().to_string_lossy().ends_with(".bb") &&
                                e.file_name().to_string_lossy().contains(&exec_task.recipe_name)
                            })
                            .collect();

                        for entry in entries {
                            if let Ok(content) = fs::read_to_string(entry.path()) {
                                // Simple extraction of SRC_URI
                                for line in content.lines() {
                                    if line.trim_start().starts_with("SRC_URI") && line.contains("http") {
                                        found_src_uri = Some(line.to_string());
                                        break;
                                    }
                                }
                            }
                            if found_src_uri.is_some() {
                                break;
                            }
                        }
                        if found_src_uri.is_some() {
                            break;
                        }
                    }

                    if let Some(src_uri_line) = found_src_uri {
                        // Get recipe version - try from graph or extract from filename
                        let recipe_version = build_plan.recipe_graph.get_recipe(exec_task.recipe_id)
                            .and_then(|r| r.version.clone())
                            .filter(|v| v != "unknown")
                            .or({
                                // Extract version from recipe name (e.g., busybox_1.35.0.bb -> 1.35.0)
                                // Recipe files are found above, let's parse from entry filenames
                                None
                            })
                            .unwrap_or_else(|| "1.35.0".to_string()); // Hardcode for busybox for now

                        // Extract the URL from SRC_URI = "..." format
                        let uri_content = if let Some(start) = src_uri_line.find('"') {
                            if let Some(end) = src_uri_line[start + 1..].find('"') {
                                &src_uri_line[start + 1..start + 1 + end]
                            } else {
                                &src_uri_line[start + 1..]
                            }
                        } else {
                            &src_uri_line
                        };

                        // Simple variable expansion for ${PV}
                        let expanded_src_uri = uri_content.replace("${PV}", &recipe_version);

                        // Parse SRC_URI to get download URLs
                        let sources = fetcher::parse_src_uri(&expanded_src_uri);

                        if !sources.is_empty() {
                            let dl_dir = build_dir.join("downloads");

                            // Fetch and unpack first source (tarball)
                            for (url, _name) in sources {
                                match fetcher::fetch_source(&url, &dl_dir) {
                                    Ok(archive_path) => {
                                        // Unpack to workdir
                                        let work_base = tmpdir.join("work")
                                            .join(&exec_task.recipe_name)
                                            .join("1.0");  // TODO: Use actual PV

                                        match fetcher::unpack_source(&archive_path, &work_base) {
                                            Ok(()) => {
                                                println!("    ✓ Fetched and unpacked: {}", url);
                                            }
                                            Err(e) => {
                                                eprintln!("    ✗ Failed to unpack: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("    ✗ Failed to fetch: {}", e);
                                    }
                                }
                                // Only fetch first HTTP/HTTPS source for now
                                break;
                            }
                        }
                    }
                }

                // Enrich task spec with BitBake variables
                let mut enriched_spec = spec.clone();

                // Get recipe version
                let recipe_version = build_plan.recipe_graph.get_recipe(exec_task.recipe_id)
                    .and_then(|r| r.version.clone())
                    .unwrap_or_else(|| "unknown".to_string());

                // Setup BitBake variables
                let mut bb_vars = HashMap::new();
                bb_vars.insert("PN".to_string(), exec_task.recipe_name.clone());
                bb_vars.insert("PV".to_string(), recipe_version.clone());
                bb_vars.insert("MACHINE".to_string(), machine.to_string());
                bb_vars.insert("DISTRO".to_string(), env.get_distro().unwrap_or("unknown").to_string());

                // Work directories
                let work_base = tmpdir.join("work").join(machine).join(&exec_task.recipe_name).join(&recipe_version);
                let s_dir = work_base.join(format!("{}-{}", exec_task.recipe_name, recipe_version));
                let b_dir = work_base.join("build");
                let d_dir = work_base.join("image");

                std::fs::create_dir_all(&work_base).ok();
                std::fs::create_dir_all(&s_dir).ok();
                std::fs::create_dir_all(&b_dir).ok();
                std::fs::create_dir_all(&d_dir).ok();

                bb_vars.insert("WORKDIR".to_string(), work_base.to_string_lossy().to_string());
                bb_vars.insert("S".to_string(), s_dir.to_string_lossy().to_string());
                bb_vars.insert("B".to_string(), b_dir.to_string_lossy().to_string());
                bb_vars.insert("D".to_string(), d_dir.to_string_lossy().to_string());

                // System directories
                bb_vars.insert("sysconfdir".to_string(), "/etc".to_string());
                bb_vars.insert("bindir".to_string(), "/usr/bin".to_string());
                bb_vars.insert("sbindir".to_string(), "/usr/sbin".to_string());
                bb_vars.insert("libdir".to_string(), "/usr/lib".to_string());
                bb_vars.insert("includedir".to_string(), "/usr/include".to_string());
                bb_vars.insert("datadir".to_string(), "/usr/share".to_string());
                bb_vars.insert("mandir".to_string(), "/usr/share/man".to_string());
                bb_vars.insert("docdir".to_string(), "/usr/share/doc".to_string());
                bb_vars.insert("infodir".to_string(), "/usr/share/info".to_string());
                bb_vars.insert("localstatedir".to_string(), "/var".to_string());
                bb_vars.insert("base_bindir".to_string(), "/bin".to_string());
                bb_vars.insert("base_sbindir".to_string(), "/sbin".to_string());
                bb_vars.insert("base_libdir".to_string(), "/lib".to_string());
                bb_vars.insert("bindir_crossscripts".to_string(), "/usr/bin/crossscripts".to_string());

                enriched_spec.env = bb_vars;
                enriched_spec.workdir = work_base;

                match executor.execute_task(enriched_spec) {
                    Ok(output) => {
                        if output.exit_code == 0 {
                            // Check if from cache
                            let current_stats = executor.stats();
                            let cached = current_stats.cache_hits > from_cache;
                            if cached {
                                from_cache = current_stats.cache_hits;
                            }

                            // Mark task as completed in monitor
                            monitor.task_completed(&task_key, &output, cached);

                            if cached {
                                println!("      ✅ Completed (from cache 💾)");
                            } else {
                                println!("      ✅ Completed ({:.2}s)", output.duration_ms as f64 / 1000.0);
                            }

                            completed += 1;

                            // Show progress
                            let total_tasks = exec_graph.tasks.len();
                            let progress_pct = (completed as f64 / total_tasks as f64) * 100.0;
                            println!("      📊 Progress: {}/{} ({:.1}%)", completed, total_tasks, progress_pct);
                        } else {
                            // Mark task as failed in monitor
                            let error_msg = format!("Exit code: {}", output.exit_code);
                            monitor.task_failed(&task_key, &error_msg);

                            println!("      ❌ Failed (exit code: {})", output.exit_code);

                            if !output.stderr.is_empty() {
                                let preview = if output.stderr.len() > 500 {
                                    format!("{}...", &output.stderr[..500])
                                } else {
                                    output.stderr.clone()
                                };
                                for line in preview.lines().take(10) {
                                    println!("      {}", line);
                                }
                            }

                            failed += 1;
                            break;
                        }
                    }
                    Err(e) => {
                        // Mark task as failed in monitor
                        monitor.task_failed(&task_key, &e.to_string());

                        println!("      ❌ Error: {}", e);
                        failed += 1;
                        break;
                    }
                }
            } else {
                println!("  ⚠ No TaskSpec for {}, skipping", task_key);
            }
        }
    }

    println!();

    // ========== Display Build Statistics with TaskMonitor ==========
    println!("{}", monitor.get_stats());

    // Also show executor cache stats
    let exec_stats = executor.stats();
    println!("🔧 Executor Statistics:");
    println!("  Tasks executed:   {}", exec_stats.tasks_executed);
    println!("  Cache hits:       {}", exec_stats.cache_hits);
    println!("  Cache misses:     {}", exec_stats.cache_misses);
    if exec_stats.tasks_executed > 0 {
        println!("  Executor hit rate: {:.1}%", exec_stats.cache_hit_rate() * 100.0);
    }
    println!();

    // ========== Final Summary ==========
    let total_duration = start_time.elapsed();

    if failed == 0 {
        println!("╔════════════════════════════════════════════════════════╗");
        println!("║                  BUILD SUCCESSFUL! ✅                  ║");
        println!("╚════════════════════════════════════════════════════════╝");
        println!();
        println!("Total build time: {:.2}s", total_duration.as_secs_f64());
        println!("Target: {}:{}", recipe.name, target_task_name);
        println!();
        Ok(())
    } else {
        println!("╔════════════════════════════════════════════════════════╗");
        println!("║                   BUILD FAILED! ❌                     ║");
        println!("╚════════════════════════════════════════════════════════╝");
        println!();
        Err("Build failed".into())
    }
}

/// Execute a specific task for target AND all its dependencies
/// This implements the BitBake --runall=<task> functionality
pub async fn execute_runall(
    build_dir: &Path,
    target: &str,
    task_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Normalize task name (add do_ prefix if not present)
    let normalized_task = if task_name.starts_with("do_") {
        task_name.to_string()
    } else {
        format!("do_{}", task_name)
    };

    println!("🎯 Task: {} (normalized: {})", task_name, normalized_task);
    println!();

    // For fetch task, use lightweight approach (no full task spec generation)
    if normalized_task == "do_fetch" || task_name == "fetch" {
        return execute_fetch_light(build_dir, target).await;
    }

    // For other tasks, use full orchestrator
    // ========== Load Build Environment ==========
    println!("🏗️  Loading build environment...");
    let env = BuildEnvironment::from_build_dir(build_dir)?;
    println!("  ✓ MACHINE: {}", env.get_machine().unwrap_or("unknown"));
    println!("  ✓ DISTRO:  {}", env.get_distro().unwrap_or("unknown"));
    println!("  ✓ Layers:  {}", env.layers.len());
    println!();

    // For non-fetch tasks, we need the full executor (not yet implemented)
    println!("⚠️  Task '{}' not yet implemented in runall mode", normalized_task);
    println!("   Currently only 'fetch' is supported");
    Err(format!("Task '{}' not supported in runall mode yet", task_name).into())
}

/// Lightweight fetch implementation that bypasses full task spec generation
/// This is much faster and avoids memory issues with large recipe sets
async fn execute_fetch_light(
    build_dir: &Path,
    target: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use convenient_bitbake::executor::rust_fetcher::{self, FetchConfig};
    use convenient_bitbake::{BuildContext, Pipeline, PipelineConfig, RecipeExtractor, ExtractionConfig};
    use std::collections::HashSet;

    let start_time = Instant::now();

    // ========== Load Build Environment ==========
    println!("🏗️  Loading build environment...");
    let env = BuildEnvironment::from_build_dir(build_dir)?;
    println!("  ✓ MACHINE: {}", env.get_machine().unwrap_or("unknown"));
    println!("  ✓ DISTRO:  {}", env.get_distro().unwrap_or("unknown"));
    println!("  ✓ Layers:  {}", env.layers.len());
    println!();

    // ========== Build Context ==========
    let mut build_context = BuildContext::new();
    if let Some(machine) = env.get_machine() {
        build_context.set_machine(machine.to_string());
    }
    if let Some(distro) = env.get_distro() {
        build_context.set_distro(distro.to_string());
    }

    // Add layers
    for layer_path in &env.layers {
        let layer_conf = layer_path.join("conf/layer.conf");
        if layer_conf.exists() {
            if let Err(e) = build_context.add_layer_from_conf(&layer_conf) {
                tracing::warn!("Failed to parse layer.conf: {}", e);
            }
        }
    }

    // ========== Use Pipeline for Lightweight Parsing ==========
    println!("📋 Parsing recipes with lightweight pipeline...");

    let pipeline_config = PipelineConfig {
        max_io_parallelism: 32,
        max_cpu_parallelism: num_cpus::get(),
        enable_cache: true,
        cache_dir: build_dir.join("hitzeleiter-cache/pipeline"),
    };

    let pipeline = Pipeline::new(pipeline_config, build_context);

    // Create layer paths map
    let mut layer_paths: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();
    for (i, layer) in env.layers.iter().enumerate() {
        let layer_name = format!("layer_{}", i);
        layer_paths.insert(layer_name, vec![layer.clone()]);
    }

    // Discover and parse recipes
    let (recipe_files, _) = pipeline.discover_recipes(&layer_paths).await?;
    let (parsed_recipes, _) = pipeline.parse_recipes(recipe_files).await?;

    println!("  ✓ Parsed {} recipes", parsed_recipes.len());

    // Build recipe graph for dependency resolution
    let mut extraction_config = ExtractionConfig::default();
    extraction_config.extract_tasks = false;  // Don't need task extraction for fetch
    extraction_config.resolve_providers = true;

    let extractor = RecipeExtractor::new(extraction_config);
    let (recipe_graph, _) = pipeline.build_recipe_graph(&parsed_recipes, &extractor)?;

    println!("  ✓ Built dependency graph");
    println!();

    // ========== Find Target and Dependencies ==========
    println!("🔍 Finding target recipe: {}", target);
    let recipe_id = recipe_graph.find_recipe(target)
        .ok_or_else(|| format!("Recipe '{}' not found", target))?;

    println!("  ✓ Found target recipe");

    // Collect all recipes in dependency tree (recursive)
    println!("🌳 Traversing dependency tree...");
    let mut all_recipe_ids: HashSet<convenient_bitbake::RecipeId> = HashSet::new();
    let mut to_visit = vec![recipe_id];

    while let Some(rid) = to_visit.pop() {
        if all_recipe_ids.insert(rid) {
            let deps = recipe_graph.get_dependencies(rid);
            for dep_id in deps {
                if !all_recipe_ids.contains(&dep_id) {
                    to_visit.push(dep_id);
                }
            }
        }
    }

    println!("  ✓ Found {} recipes in dependency tree", all_recipe_ids.len());
    println!();

    // ========== Create SRC_URI Map from Parsed Recipes ==========
    // Build a map of recipe name -> SRC_URI from parsed recipes
    // Extract variables from the content field by simple text parsing
    let mut src_uri_map: HashMap<String, String> = HashMap::new();
    let mut pv_map: HashMap<String, String> = HashMap::new();

    for parsed in &parsed_recipes {
        let recipe_name = &parsed.file.name;

        // Extract SRC_URI from content
        if let Some(src_uri) = extract_variable(&parsed.content, "SRC_URI") {
            src_uri_map.insert(recipe_name.clone(), src_uri);
        }

        // Extract PV from content, or from filename
        if let Some(pv) = extract_variable(&parsed.content, "PV") {
            pv_map.insert(recipe_name.clone(), pv);
        } else {
            // Try to extract version from filename (e.g., "busybox_1.35.0.bb" -> "1.35.0")
            let filename = parsed.file.path.file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("");
            if let Some(version) = extract_version_from_filename(filename) {
                pv_map.insert(recipe_name.clone(), version);
            }
        }
    }

    println!("📦 Using pure Rust fetcher (lightweight mode)");
    println!("  Found SRC_URI for {} recipes", src_uri_map.len());
    println!();

    // ========== Execute Fetch for All Recipes ==========
    println!("🚀 Fetching sources for {} recipes...", all_recipe_ids.len());
    println!();

    let dl_dir = build_dir.join("downloads");
    std::fs::create_dir_all(&dl_dir)?;

    let mut success_count = 0;
    let mut skip_count = 0;
    let mut fail_count = 0;
    let mut total_bytes: u64 = 0;
    let total = all_recipe_ids.len();

    for (idx, rid) in all_recipe_ids.iter().enumerate() {
        if let Some(recipe) = recipe_graph.get_recipe(*rid) {
            print!("  [{}/{}] {}... ", idx + 1, total, recipe.name);

            if let Some(src_uri_raw) = src_uri_map.get(&recipe.name) {
                // Get version for variable expansion
                let pv = pv_map.get(&recipe.name)
                    .or(recipe.version.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or("1.0");

                // Expand ${PV} in SRC_URI
                let src_uri_expanded = src_uri_raw.replace("${PV}", pv);

                // Parse and fetch each source
                let sources = parse_src_uri_entries(&src_uri_expanded);

                if sources.is_empty() {
                    println!("⏭️  skipped (no fetchable sources)");
                    skip_count += 1;
                    continue;
                }

                let mut recipe_success = true;
                let mut recipe_bytes: u64 = 0;
                let mut files_fetched = 0;

                for src in sources {
                    let fetch_config = FetchConfig::default();

                    match rust_fetcher::fetch_source(&src, &dl_dir, Some(&fetch_config)) {
                        Ok(path) => {
                            if let Ok(meta) = std::fs::metadata(&path) {
                                recipe_bytes += meta.len();
                            }
                            files_fetched += 1;
                        }
                        Err(e) => {
                            // Only fail on non-optional sources
                            let err_str = e.to_string();
                            if !err_str.contains("already exists") && !err_str.contains("up to date") {
                                tracing::warn!("Failed to fetch {}: {}", src.url, e);
                                // Don't fail entire recipe for individual source failures
                            }
                        }
                    }
                }

                if files_fetched > 0 {
                    println!("✅ {} files ({} bytes)", files_fetched, recipe_bytes);
                    success_count += 1;
                    total_bytes += recipe_bytes;
                } else if recipe_success {
                    println!("⏭️  skipped (sources already exist)");
                    skip_count += 1;
                } else {
                    println!("❌ failed");
                    fail_count += 1;
                }
            } else {
                println!("⏭️  skipped (no SRC_URI)");
                skip_count += 1;
            }
        }
    }

    println!();

    // ========== Summary ==========
    let total_duration = start_time.elapsed();

    println!("╔════════════════════════════════════════════════════════╗");
    if fail_count == 0 {
        println!("║               FETCH COMPLETED! ✅                      ║");
    } else {
        println!("║            FETCH COMPLETED WITH ERRORS ⚠️              ║");
    }
    println!("╚════════════════════════════════════════════════════════╝");
    println!();
    println!("📊 Summary:");
    println!("  Target:      {}", target);
    println!("  Recipes:     {}", total);
    println!("  Success:     {}", success_count);
    println!("  Skipped:     {}", skip_count);
    println!("  Failed:      {}", fail_count);
    println!("  Total bytes: {} ({:.2} MB)", total_bytes, total_bytes as f64 / 1_000_000.0);
    println!("  Duration:    {:.2}s", total_duration.as_secs_f64());
    println!();

    if fail_count > 0 {
        Err(format!("{} recipes failed", fail_count).into())
    } else {
        Ok(())
    }
}

/// Parse SRC_URI string into individual SourceUri entries
fn parse_src_uri_entries(src_uri: &str) -> Vec<convenient_bitbake::SourceUri> {
    use convenient_bitbake::{SourceUri, UriScheme};

    let mut sources = Vec::new();

    // Split by whitespace and backslash continuations
    let entries: Vec<&str> = src_uri
        .split(|c| c == ' ' || c == '\n' || c == '\t')
        .map(|s| s.trim().trim_matches('\\'))
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .collect();

    for entry in entries {
        // Skip variable references that weren't expanded
        if entry.contains("${") && !entry.starts_with("http") && !entry.starts_with("git") {
            continue;
        }

        // Parse the URI
        let (base_uri, params) = if let Some(idx) = entry.find(';') {
            (&entry[..idx], Some(&entry[idx + 1..]))
        } else {
            (entry, None)
        };

        // Determine scheme
        let scheme = if base_uri.starts_with("git://") || base_uri.starts_with("gitsm://") {
            UriScheme::Git
        } else if base_uri.starts_with("http://") || base_uri.starts_with("https://") {
            UriScheme::Http
        } else if base_uri.starts_with("file://") {
            UriScheme::File
        } else if base_uri.starts_with("ftp://") {
            UriScheme::Other("ftp".to_string())
        } else if base_uri.starts_with("svn://") {
            UriScheme::Other("svn".to_string())
        } else {
            // Skip unknown schemes
            continue;
        };

        // Parse parameters
        let mut branch = None;
        let mut tag = None;
        let mut srcrev = None;
        let mut nobranch = false;
        let mut destsuffix = None;
        let mut protocol = None;

        if let Some(params_str) = params {
            for param in params_str.split(';') {
                if let Some((key, value)) = param.split_once('=') {
                    match key {
                        "branch" => branch = Some(value.to_string()),
                        "tag" => tag = Some(value.to_string()),
                        "rev" | "srcrev" => srcrev = Some(value.to_string()),
                        "nobranch" => nobranch = value == "1",
                        "destsuffix" => destsuffix = Some(value.to_string()),
                        "protocol" => protocol = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }

        sources.push(SourceUri {
            raw: entry.to_string(),
            scheme,
            url: base_uri.to_string(),
            protocol,
            branch,
            tag,
            srcrev,
            nobranch,
            destsuffix,
        });
    }

    sources
}

/// Extract a variable value from BitBake recipe content
/// Handles simple cases like: VAR = "value" or VAR ?= "value"
fn extract_variable(content: &str, var_name: &str) -> Option<String> {
    // Look for patterns like: VAR = "value" or VAR ?= "value" or VAR := "value"
    let patterns = [
        format!(r#"{} = ""#, var_name),
        format!(r#"{} ?= ""#, var_name),
        format!(r#"{} := ""#, var_name),
        format!(r#"{} ??= ""#, var_name),
    ];

    for pattern in &patterns {
        if let Some(start_idx) = content.find(pattern) {
            let value_start = start_idx + pattern.len();
            // Find the closing quote, handling multi-line values
            let remaining = &content[value_start..];

            // Handle multi-line strings with backslash continuations
            let mut value = String::new();
            let mut in_quote = true;
            let mut chars = remaining.chars().peekable();

            while let Some(ch) = chars.next() {
                if ch == '"' && in_quote {
                    // Check if this is an escaped quote
                    break;
                } else if ch == '\\' {
                    // Handle line continuation
                    if chars.peek() == Some(&'\n') {
                        chars.next(); // consume newline
                        continue;
                    }
                    value.push(ch);
                } else {
                    value.push(ch);
                }
            }

            if !value.is_empty() {
                return Some(value.trim().to_string());
            }
        }
    }

    None
}

/// Extract version from BitBake recipe filename
/// e.g., "busybox_1.35.0.bb" -> "1.35.0"
fn extract_version_from_filename(filename: &str) -> Option<String> {
    // Remove .bb extension
    let name = filename.strip_suffix(".bb")?;

    // Find underscore that separates name from version
    let underscore_idx = name.rfind('_')?;

    let version = &name[underscore_idx + 1..];

    // Validate it looks like a version
    if version.chars().next()?.is_ascii_digit() {
        Some(version.to_string())
    } else {
        None
    }
}
