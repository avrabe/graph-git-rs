//! Query command for dependency exploration

use convenient_bitbake::{BuildEnvironment, BuildOrchestrator, OrchestratorConfig};
use convenient_bitbake::query::{RecipeQueryEngine, OutputFormat};
use std::collections::HashMap;
use std::path::Path;

/// Execute a query against the recipe graph
pub async fn execute(
    build_dir: &Path,
    query: &str,
    format: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🔍 Query: {}", query);
    println!();

    // Load environment and build recipe graph
    println!("Loading build environment...");
    let env = BuildEnvironment::from_build_dir(build_dir)?;

    let cache_dir = build_dir.join("bitzel-cache");
    let config = OrchestratorConfig {
        cache_dir,
        parallel_parse: true,
        enable_incremental: false, // Don't need incremental for queries
        max_io_parallelism: 32,
        max_cpu_parallelism: num_cpus::get(),
    };

    let orchestrator = BuildOrchestrator::new(config);

    // Create layer paths
    let mut layer_paths: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();
    for (i, layer) in env.layers.iter().enumerate() {
        layer_paths.insert(format!("layer_{}", i), vec![layer.clone()]);
    }

    println!("Building recipe graph...");
    let build_plan = orchestrator.build_plan(layer_paths).await?;

    println!("  ✓ Loaded {} recipes", build_plan.recipe_graph.recipe_count());
    println!();

    // Parse and execute query
    use convenient_bitbake::query::QueryParser;

    println!("Parsing query...");
    let query_expr = QueryParser::parse(query)?;

    println!("Executing query...");
    let engine = RecipeQueryEngine::new(&build_plan.recipe_graph);
    let results = engine.execute(&query_expr)?;

    println!();
    println!("Results:");

    // Format output based on requested format
    match format {
        "json" => {
            // JSON format
            println!("[");
            for (i, target) in results.iter().enumerate() {
                if i > 0 {
                    println!(",");
                }
                print!("  {{\"recipe\": \"{}\"}}", target.recipe_name);
            }
            println!();
            println!("]");
        }
        "graph" | "dot" => {
            // GraphViz DOT format
            println!("digraph RecipeDependencies {{");
            println!("  rankdir=LR;");
            for target in &results {
                println!("  \"{}\";", target.recipe_name);
            }
            println!("}}");
        }
        "label" => {
            // Just recipe names
            for target in &results {
                println!("{}", target.recipe_name);
            }
        }
        _ => {
            // Text format (default)
            for target in &results {
                println!("  {}", target.recipe_name);
            }
        }
    }

    println!();
    println!("Found {} results", results.len());

    Ok(())
}

/// Show query help and examples
pub fn help() {
    println!("╔════════════════════════════════════════════════════════╗");
    println!("║              BITZEL QUERY LANGUAGE                     ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!();
    println!("Query functions:");
    println!();
    println!("  deps(target, max_depth)");
    println!("    Find all dependencies of target");
    println!("    Example: deps(busybox, 2)");
    println!();
    println!("  rdeps(universe, target)");
    println!("    Find reverse dependencies (what depends on target)");
    println!("    Example: rdeps(*, zlib)");
    println!();
    println!("  somepath(from, to)");
    println!("    Find a dependency path between two recipes");
    println!("    Example: somepath(glibc, linux-kernel)");
    println!();
    println!("  allpaths(from, to)");
    println!("    Find all dependency paths");
    println!("    Example: allpaths(glibc, linux-kernel)");
    println!();
    println!("  kind(pattern, expr)");
    println!("    Filter by target type/pattern");
    println!("    Example: kind(\"*-native\", deps(busybox, 1))");
    println!();
    println!("  filter(pattern, expr)");
    println!("    Filter results by label pattern");
    println!("    Example: filter(\"lib*\", deps(*, 1))");
    println!();
    println!("Output formats:");
    println!("  --format text   - Human-readable list (default)");
    println!("  --format json   - Machine-readable JSON");
    println!("  --format graph  - GraphViz DOT format");
    println!("  --format label  - Just recipe names");
    println!();
    println!("Examples:");
    println!("  # Find all dependencies of busybox");
    println!("  bitzel query 'deps(busybox, 10)'");
    println!();
    println!("  # Find what depends on zlib");
    println!("  bitzel query 'rdeps(*, zlib)'");
    println!();
    println!("  # Export dependency graph");
    println!("  bitzel query 'deps(busybox, 3)' --format graph > busybox.dot");
    println!("  dot -Tpng busybox.dot -o busybox.png");
    println!();
    println!("  # Find native dependencies");
    println!("  bitzel query 'kind(\"*-native\", deps(gcc, 2))'");
}
