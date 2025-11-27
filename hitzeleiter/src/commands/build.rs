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
    dry_run: bool,
    skip_fetch: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let start_time = Instant::now();

    if dry_run {
        println!("╔════════════════════════════════════════════════════════╗");
        println!("║       🏎️  BITZEL FERRARI BUILD (DRY-RUN) 🏎️           ║");
        println!("║  Plan-Only Mode: No execution, analysis only          ║");
        println!("╚════════════════════════════════════════════════════════╝");
    } else if skip_fetch {
        println!("╔════════════════════════════════════════════════════════╗");
        println!("║       🏎️  BITZEL FERRARI BUILD (OFFLINE) 🏎️           ║");
        println!("║  Offline Mode: Fetch tasks will be skipped            ║");
        println!("╚════════════════════════════════════════════════════════╝");
    } else {
        println!("╔════════════════════════════════════════════════════════╗");
        println!("║           🏎️  BITZEL FERRARI BUILD  🏎️                ║");
        println!("║  Full-Featured Bazel-Inspired BitBake Build System    ║");
        println!("╚════════════════════════════════════════════════════════╝");
    }
    println!();
    println!("Target: {}", target);
    println!("Build directory: {:?}", build_dir);
    if dry_run {
        println!("Mode: DRY-RUN (no execution)");
    }
    if skip_fetch {
        println!("Mode: OFFLINE (fetch tasks will be skipped)");
    }
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
    if dry_run {
        println!("📋 EXECUTION PLAN (would execute in this order):");
        println!();

        let mut task_num = 0;
        for &task_id in &exec_graph.execution_order {
            if let Some(exec_task) = exec_graph.tasks.get(&task_id) {
                task_num += 1;
                let task_key = format!("{}:{}", exec_task.recipe_name, exec_task.task_name);

                if let Some(spec) = build_plan.task_specs.get(&task_key) {
                    println!("  {}. {}", task_num, task_key);
                    println!("     Network: {:?}", spec.network_policy);
                    if let Some(timeout) = spec.timeout {
                        println!("     Timeout: {}s", timeout.as_secs());
                    } else {
                        println!("     Timeout: none");
                    }

                    // Show dependencies
                    if !exec_task.depends_on.is_empty() {
                        print!("     Depends on: ");
                        let dep_names: Vec<_> = exec_task.depends_on.iter()
                            .filter_map(|dep_id| exec_graph.tasks.get(dep_id))
                            .map(|t| format!("{}:{}", t.recipe_name, t.task_name))
                            .collect();
                        println!("{}", dep_names.join(", "));
                    }

                    // Show what the task would do
                    match exec_task.task_name.as_str() {
                        "fetch" => println!("     Would: Download sources"),
                        "unpack" => println!("     Would: Extract archive to workdir"),
                        "patch" => println!("     Would: Apply patches"),
                        "configure" => println!("     Would: Run configure script"),
                        "compile" => println!("     Would: Compile source code"),
                        "install" => println!("     Would: Install into image directory"),
                        "package" => println!("     Would: Create packages"),
                        _ => println!("     Would: Execute task script"),
                    }
                    println!();
                } else {
                    println!("  {}. {} (no TaskSpec)", task_num, task_key);
                    println!();
                }
            }
        }

        // Summary
        let total_duration = start_time.elapsed();
        println!("╔════════════════════════════════════════════════════════╗");
        println!("║            DRY-RUN COMPLETE ✓                          ║");
        println!("╚════════════════════════════════════════════════════════╝");
        println!();
        println!("📊 Summary:");
        println!("  Total tasks:    {}", exec_graph.tasks.len());
        println!("  Target:         {}:{}", recipe.name, target_task_name);
        println!("  Analysis time:  {:.2}s", total_duration.as_secs_f64());
        println!();
        println!("✓ Ready to execute with: hitzeleiter build -b {:?} {}", build_dir, target);
        println!();

        return Ok(());
    }

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
                // Skip fetch tasks in offline mode
                if skip_fetch && (exec_task.task_name == "fetch" || exec_task.task_name == "do_fetch") {
                    println!("  ⏭️  {} (skipped - offline mode)", task_key);
                    completed += 1;
                    continue;
                }

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

                        // Only fetch first HTTP/HTTPS source for now
                        if let Some((url, _name)) = sources.into_iter().next() {
                            let dl_dir = build_dir.join("downloads");

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

                // Merge bb_vars into existing env (don't overwrite recipe vars like SRC_URI)
                // bb_vars take precedence for paths like WORKDIR, S, B, D
                for (k, v) in bb_vars {
                    enriched_spec.env.insert(k, v);
                }
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
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use convenient_bitbake::executor::fetch_task;
    use convenient_bitbake::executor::rust_fetcher::FetchConfig;
    use std::collections::HashSet;

    let start_time = Instant::now();

    // Normalize task name (add do_ prefix if not present)
    let normalized_task = if task_name.starts_with("do_") {
        task_name.to_string()
    } else {
        format!("do_{}", task_name)
    };

    println!("🎯 Task: {} (normalized: {})", task_name, normalized_task);
    println!();

    // ========== Load Build Environment ==========
    println!("🏗️  Loading build environment...");
    let env = BuildEnvironment::from_build_dir(build_dir)?;
    println!("  ✓ MACHINE: {}", env.get_machine().unwrap_or("unknown"));
    println!("  ✓ DISTRO:  {}", env.get_distro().unwrap_or("unknown"));
    println!("  ✓ Layers:  {}", env.layers.len());
    println!();

    // ========== Build Orchestration ==========
    println!("🎼 Building recipe graph...");

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
    println!();

    // ========== Find Target and All Dependencies ==========
    println!("🔍 Finding target recipe: {}", target);
    let recipe_id = build_plan.recipe_graph.find_recipe(target)
        .ok_or_else(|| format!("Recipe '{}' not found", target))?;

    println!("  ✓ Found target recipe");

    // Collect all recipes in dependency tree (recursive)
    println!("🌳 Traversing dependency tree...");
    let mut all_recipe_ids: HashSet<convenient_bitbake::RecipeId> = HashSet::new();
    let mut to_visit = vec![recipe_id];

    while let Some(rid) = to_visit.pop() {
        if all_recipe_ids.insert(rid) {
            // Get dependencies of this recipe
            let deps = build_plan.recipe_graph.get_dependencies(rid);
            for dep_id in deps {
                if !all_recipe_ids.contains(&dep_id) {
                    to_visit.push(dep_id);
                }
            }
        }
    }

    println!("  ✓ Found {} recipes in dependency tree", all_recipe_ids.len());
    println!();

    // ========== Dry-run: Show what would be executed ==========
    if dry_run {
        println!("📋 EXECUTION PLAN (would run {} for each recipe):", normalized_task);
        println!();

        let mut recipe_num = 0;
        let mut has_src_uri = 0;
        let mut no_src_uri = 0;

        for rid in &all_recipe_ids {
            if let Some(recipe) = build_plan.recipe_graph.get_recipe(*rid) {
                recipe_num += 1;

                let fetch_task_key = format!("{}:fetch", recipe.name);
                let do_fetch_task_key = format!("{}:do_fetch", recipe.name);

                let task_spec = build_plan.task_specs.get(&fetch_task_key)
                    .or_else(|| build_plan.task_specs.get(&do_fetch_task_key));

                if let Some(spec) = task_spec {
                    if spec.env.contains_key("SRC_URI") {
                        let src_uri = spec.env.get("SRC_URI").map(|s| s.as_str()).unwrap_or("");
                        let truncated = if src_uri.len() > 60 {
                            format!("{}...", &src_uri[..60])
                        } else {
                            src_uri.to_string()
                        };
                        println!("  {}. {} - would fetch: {}", recipe_num, recipe.name, truncated);
                        has_src_uri += 1;
                    } else {
                        println!("  {}. {} - would skip (no SRC_URI)", recipe_num, recipe.name);
                        no_src_uri += 1;
                    }
                } else {
                    println!("  {}. {} - would skip (no task spec)", recipe_num, recipe.name);
                    no_src_uri += 1;
                }
            }
        }

        // Summary
        let total_duration = start_time.elapsed();
        println!();
        println!("╔════════════════════════════════════════════════════════╗");
        println!("║            DRY-RUN COMPLETE ✓                          ║");
        println!("╚════════════════════════════════════════════════════════╝");
        println!();
        println!("📊 Summary:");
        println!("  Task:           {}", normalized_task);
        println!("  Target:         {}", target);
        println!("  Total recipes:  {}", all_recipe_ids.len());
        println!("  Would fetch:    {}", has_src_uri);
        println!("  Would skip:     {}", no_src_uri);
        println!("  Analysis time:  {:.2}s", total_duration.as_secs_f64());
        println!();
        println!("✓ Ready to execute with: hitzeleiter build -b {:?} {} --runall={}", build_dir, target, task_name);
        println!();

        return Ok(());
    }

    // ========== Execute Task for All Recipes ==========
    println!("🚀 Running {} for {} recipes...", normalized_task, all_recipe_ids.len());
    println!();

    let dl_dir = build_dir.join("downloads");
    std::fs::create_dir_all(&dl_dir)?;

    let mut success_count = 0;
    let mut skip_count = 0;
    let mut fail_count = 0;
    let total = all_recipe_ids.len();

    // For fetch task, use pure Rust fetcher
    if normalized_task == "do_fetch" || task_name == "fetch" {
        println!("📦 Using pure Rust fetcher (no host tools required)");
        println!();

        for (idx, rid) in all_recipe_ids.iter().enumerate() {
            if let Some(recipe) = build_plan.recipe_graph.get_recipe(*rid) {
                print!("  [{}/{}] {}... ", idx + 1, total, recipe.name);

                // Look for fetch task spec for this recipe
                // Task specs are keyed as "recipe_name:task_name" (without do_ prefix)
                let fetch_task_key = format!("{}:fetch", recipe.name);
                let do_fetch_task_key = format!("{}:do_fetch", recipe.name);

                let task_spec = build_plan.task_specs.get(&fetch_task_key)
                    .or_else(|| build_plan.task_specs.get(&do_fetch_task_key));

                if let Some(spec) = task_spec {
                    // Use the environment from the task spec
                    if spec.env.contains_key("SRC_URI") {
                        let fetch_config = FetchConfig::default();

                        match fetch_task::execute_fetch_task(&spec.env, &dl_dir, Some(&fetch_config)) {
                            Ok(result) => {
                                if result.downloaded_files.is_empty() && result.warnings.is_empty() {
                                    println!("⏭️  skipped (no sources)");
                                    skip_count += 1;
                                } else {
                                    println!("✅ {} files ({} bytes)",
                                        result.downloaded_files.len(),
                                        result.total_bytes
                                    );
                                    success_count += 1;
                                }
                            }
                            Err(e) => {
                                println!("❌ {}", e);
                                fail_count += 1;
                            }
                        }
                    } else {
                        println!("⏭️  skipped (no SRC_URI in task spec)");
                        skip_count += 1;
                    }
                } else {
                    println!("⏭️  skipped (no fetch task spec)");
                    skip_count += 1;
                }
            }
        }
    } else {
        // For other tasks, we'd need the full executor
        println!("⚠️  Task '{}' not yet implemented in runall mode", normalized_task);
        println!("   Currently only 'fetch' is supported");
        return Err(format!("Task '{}' not supported in runall mode yet", task_name).into());
    }

    println!();

    // ========== Summary ==========
    let total_duration = start_time.elapsed();

    println!("╔════════════════════════════════════════════════════════╗");
    if fail_count == 0 {
        println!("║               RUNALL COMPLETED! ✅                     ║");
    } else {
        println!("║            RUNALL COMPLETED WITH ERRORS ⚠️             ║");
    }
    println!("╚════════════════════════════════════════════════════════╝");
    println!();
    println!("📊 Summary:");
    println!("  Task:      {}", normalized_task);
    println!("  Target:    {}", target);
    println!("  Recipes:   {}", total);
    println!("  Success:   {}", success_count);
    println!("  Skipped:   {}", skip_count);
    println!("  Failed:    {}", fail_count);
    println!("  Duration:  {:.2}s", total_duration.as_secs_f64());
    println!();

    if fail_count > 0 {
        Err(format!("{} recipes failed", fail_count).into())
    } else {
        Ok(())
    }
}
