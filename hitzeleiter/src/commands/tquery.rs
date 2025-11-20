//! Task query command for dependency exploration

use convenient_bitbake::{BuildEnvironment, BuildOrchestrator, OrchestratorConfig};
use convenient_bitbake::query::{QueryParser, TaskQueryEngine};
use std::collections::HashMap;
use std::path::Path;

/// Execute a task query against the task graph
pub async fn execute(
    build_dir: &Path,
    query: &str,
    format: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🔍 Task Query: {}", query);
    println!();

    // Load environment and build task graph
    println!("Loading build environment...");
    let env = BuildEnvironment::from_build_dir(build_dir)?;

    let config = OrchestratorConfig {
        build_dir: build_dir.to_path_buf(),
        machine: env.get_machine().map(|s| s.to_string()),
        distro: env.get_distro().map(|s| s.to_string()),
        max_io_parallelism: 32,
        max_cpu_parallelism: num_cpus::get(),
    };

    let orchestrator = BuildOrchestrator::new(config);

    // Create layer paths
    let mut layer_paths: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();
    for (i, layer) in env.layers.iter().enumerate() {
        layer_paths.insert(format!("layer_{}", i), vec![layer.clone()]);
    }

    println!("Building task graph...");
    let build_plan = orchestrator.build_plan(layer_paths).await?;

    println!("  ✓ Loaded {} tasks", build_plan.task_graph.tasks.len());
    println!();

    // Parse and execute query
    println!("Parsing query...");
    let query_expr = QueryParser::parse(query)?;

    println!("Executing query...");
    let engine = TaskQueryEngine::new(&build_plan.task_graph, &build_plan.task_specs);
    let results = engine.execute(&query_expr)?;

    println!();

    // Handle special output formats
    match format {
        "script" => {
            // For script() queries, show the actual scripts
            if matches!(query_expr, convenient_bitbake::query::QueryExpr::Script(_)) {
                for target in &results {
                    let task_key = format!("{}:{}", target.recipe, target.task);
                    if let Some(spec) = build_plan.task_specs.get(&task_key) {
                        println!("# Script for {}:{}:{}", target.layer, target.recipe, target.task);
                        println!("{}", spec.script);
                        println!();
                    }
                }
                println!("Showed {} scripts", results.len());
                return Ok(());
            }
        }
        "env" => {
            // For env() queries, show environment variables
            if matches!(query_expr, convenient_bitbake::query::QueryExpr::Env(_)) {
                for target in &results {
                    let task_key = format!("{}:{}", target.recipe, target.task);
                    if let Some(spec) = build_plan.task_specs.get(&task_key) {
                        println!("# Environment for {}:{}:{}", target.layer, target.recipe, target.task);
                        for (key, value) in &spec.env {
                            println!("{}={}", key, value);
                        }
                        println!();
                    }
                }
                println!("Showed {} environments", results.len());
                return Ok(());
            }
        }
        _ => {}
    }

    // Standard output formats
    println!("Results:");
    match format {
        "json" => {
            // JSON format
            let json = serde_json::to_string_pretty(&results)?;
            println!("{}", json);
        }
        "dot" | "graph" => {
            // GraphViz DOT format
            println!("digraph TaskDependencies {{");
            println!("  rankdir=LR;");
            for target in &results {
                let node_id = format!("{}_{}", target.recipe, target.task);
                println!("  \"{}\" [label=\"{}:{}:{}\"];",
                    node_id, target.layer, target.recipe, target.task);
            }

            // Add edges based on dependencies
            for target in &results {
                if let Some(task) = build_plan.task_graph.tasks.get(&target.task_id) {
                    for &dep_id in &task.depends_on {
                        if let Some(dep_task) = build_plan.task_graph.tasks.get(&dep_id) {
                            // Check if dependency is in results
                            if results.iter().any(|t| t.task_id == dep_id) {
                                let from_id = format!("{}_{}", dep_task.recipe_name, dep_task.task_name);
                                let to_id = format!("{}_{}", target.recipe, target.task);
                                println!("  \"{}\" -> \"{}\";", from_id, to_id);
                            }
                        }
                    }
                }
            }
            println!("}}");
        }
        "label" => {
            // Just task names
            for target in &results {
                println!("{}:{}:{}", target.layer, target.recipe, target.task);
            }
        }
        _ => {
            // Text format (default)
            for target in &results {
                println!("  {}:{}:{}", target.layer, target.recipe, target.task);
            }
        }
    }

    println!();
    println!("Found {} tasks", results.len());

    Ok(())
}

/// Show task query help and examples
pub fn help() {
    println!("╔════════════════════════════════════════════════════════╗");
    println!("║         BITZEL TASK QUERY LANGUAGE (tquery)            ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!();
    println!("Task query functions (operates on configured task graph):");
    println!();
    println!("  deps(target, max_depth)");
    println!("    Find all task dependencies");
    println!("    Example: deps(*:busybox:install, 5)");
    println!();
    println!("  rdeps(universe, target)");
    println!("    Find reverse dependencies (what depends on task)");
    println!("    Example: rdeps(//..., *:glibc:populate_sysroot)");
    println!();
    println!("  somepath(from, to)");
    println!("    Find a dependency path between two tasks");
    println!("    Example: somepath(*:busybox:install, *:glibc:configure)");
    println!();
    println!("  kind(pattern, expr)");
    println!("    Filter by execution mode");
    println!("    Example: kind('Shell', deps(*:busybox:install, 3))");
    println!("    Example: kind('DirectRust', //...)");
    println!();
    println!("  attr(name, value, expr)");
    println!("    Filter by task attributes");
    println!("    Example: attr('network_policy', 'Isolated', //...)");
    println!();
    println!("  script(expr)");
    println!("    Show task script content (use --format script)");
    println!("    Example: script(*:kern-tools-native:configure)");
    println!();
    println!("  env(expr)");
    println!("    Show task environment variables (use --format env)");
    println!("    Example: env(*:busybox:configure)");
    println!();
    println!("Wildcard patterns:");
    println!("  *:busybox              - Find busybox in any layer");
    println!("  *:busybox:configure    - Find busybox:configure in any layer");
    println!("  *:busybox:*            - All tasks for busybox in any layer");
    println!();
    println!("Output formats:");
    println!("  --format text   - Human-readable list (default)");
    println!("  --format json   - Machine-readable JSON");
    println!("  --format dot    - GraphViz DOT format");
    println!("  --format label  - Just task names");
    println!("  --format script - Script content (for script() queries)");
    println!("  --format env    - Environment variables (for env() queries)");
    println!();
    println!("Examples:");
    println!("  # Find all tasks needed for busybox:install");
    println!("  hitzeleiter tquery 'deps(*:busybox:install, 100)'");
    println!();
    println!("  # Show the failing script");
    println!("  hitzeleiter tquery 'script(*:kern-tools-native:configure)' --format script");
    println!();
    println!("  # Find all Shell mode tasks");
    println!("  hitzeleiter tquery 'kind(\"Shell\", //...)'");
    println!();
    println!("  # Export task dependency graph");
    println!("  hitzeleiter tquery 'deps(*:busybox:install, 3)' --format dot > tasks.dot");
    println!("  dot -Tpng tasks.dot -o tasks.png");
    println!();
    println!("  # Find tasks with full network access");
    println!("  hitzeleiter tquery 'attr(\"network\", \"FullNetwork\", //...)'");
    println!();
    println!("Difference from 'query' command:");
    println!("  - query:  Operates on recipe graph (unconfigured)");
    println!("  - tquery: Operates on task graph (configured, actual execution)");
}
