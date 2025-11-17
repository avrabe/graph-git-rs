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
    SimplePythonEvaluator, TaskGraphBuilder,
};
use convenient_bitbake::executor::{
    TaskExecutor, CacheManager,
};

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

/// Expand BitBake script with full Python support
fn expand_script(
    script: &str,
    env: &HashMap<String, String>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let evaluator = SimplePythonEvaluator::new(env.clone());

    // Try to evaluate each ${@...} expression
    let mut result = script.to_string();
    let mut changed = true;

    while changed {
        changed = false;
        if let Some(start) = result.find("${@") {
            if let Some(end) = result[start..].find('}') {
                let expr = &result[start..start + end + 1];

                match evaluator.evaluate(expr) {
                    Some(value) => {
                        result = format!("{}{}{}", &result[..start], value, &result[start + end + 1..]);
                        changed = true;
                    }
                    None => {
                        // Try simple expansion for other ${VAR}
                        break;
                    }
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Fallback simple ${VAR} expansion
    result = simple_expand(&result, env);

    Ok(result)
}

/// Fallback simple expansion
fn simple_expand(script: &str, env: &HashMap<String, String>) -> String {
    let mut result = script.to_string();
    loop {
        if let Some(start) = result.find("${") {
            if let Some(end) = result[start..].find('}') {
                let var_name = &result[start + 2..start + end];
                if var_name.starts_with('@') {
                    // Skip Python expressions in fallback
                    break;
                }
                let replacement = env.get(var_name).cloned().unwrap_or_default();
                result = format!("{}{}{}", &result[..start], replacement, &result[start + end + 1..]);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    result
}

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
    let cache_dir = build_dir.join("bitzel-cache");
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
        .ok_or_else(|| format!("Recipe not found in graph"))?;

    println!("  ✓ Found: {} {}", recipe.name, recipe.version.as_deref().unwrap_or("unknown"));

    // Find the target task
    let target_task_name = "do_install";
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
    println!();

    // ========== Execute Tasks (Sequential for now) ==========
    println!("🚀 Executing task graph...");
    println!();

    let cache_dir = build_dir.join("bitzel-cache");
    let mut executor = TaskExecutor::new(&cache_dir)?;

    let mut completed = 0;
    let mut from_cache = 0;
    let mut failed = 0;

    for &task_id in &exec_graph.execution_order {
        if let Some(exec_task) = exec_graph.tasks.get(&task_id) {
            let task_key = format!("{}:{}", exec_task.recipe_name, exec_task.task_name);

            if let Some(spec) = build_plan.task_specs.get(&task_key) {
                println!("  Executing: {}", task_key);

                match executor.execute_task(spec.clone()) {
                    Ok(output) => {
                        if output.exit_code == 0 {
                            completed += 1;
                            // Check cache hit via executor stats
                            let current_stats = executor.stats();
                            if current_stats.cache_hits > from_cache {
                                from_cache = current_stats.cache_hits;
                                println!("    ✓ Completed (from cache)");
                            } else {
                                println!("    ✓ Completed ({:.2}s)", output.duration_ms as f64 / 1000.0);
                            }
                        } else {
                            failed += 1;
                            println!("    ✗ Failed (exit code: {})", output.exit_code);

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
                            break;
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        println!("    ✗ Error: {}", e);
                        break;
                    }
                }
            } else {
                println!("  ⚠ No TaskSpec for {}, skipping", task_key);
            }
        }
    }

    println!();

    // ========== Display Build Statistics ==========
    let exec_stats = executor.stats();

    println!("📊 Build Statistics:");
    println!("  Tasks completed:  {}", completed);
    println!("  From cache:       {}", from_cache);
    println!("  Failed:           {}", failed);
    if exec_stats.tasks_executed > 0 {
        println!("  Cache hit rate:   {:.1}%", exec_stats.cache_hit_rate() * 100.0);
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
