//! Bitzel - BitBake build tool with Bazel-inspired features
//!
//! Usage:
//!   hitzeleiter build <kas-file>              Build from Kas YAML
//!   hitzeleiter build --recipe <recipe>       Build specific recipe
//!   hitzeleiter clean                         Clean build artifacts
//!   hitzeleiter gc                            Run garbage collection

use convenient_bitbake::{
    BuildOrchestrator, OrchestratorConfig,
    TaskExecutor, AsyncTaskExecutor,
};
use convenient_kas::{KasFile, RepositoryManager};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "hitzeleiter")]
#[command(about = "BitBake build tool with Bazel-inspired features", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Build directory (default: ./build)
    #[arg(short, long, default_value = "build")]
    build_dir: PathBuf,

    /// Workspace directory (default: current directory)
    #[arg(short, long, default_value = ".")]
    workspace: PathBuf,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Build from Kas YAML configuration
    Build {
        /// Kas YAML file
        kas_file: PathBuf,

        /// Target recipe (optional, uses kas target if not specified)
        #[arg(short, long)]
        recipe: Option<String>,

        /// Execute tasks after planning (default: true)
        #[arg(long, default_value = "true")]
        execute: bool,

        /// Number of parallel I/O operations
        #[arg(long, default_value = "8")]
        io_parallel: usize,

        /// Number of parallel CPU operations
        #[arg(long)]
        cpu_parallel: Option<usize>,

        /// Maximum number of tasks to execute (0 = no limit)
        #[arg(long, default_value = "0")]
        max_tasks: usize,
    },

    /// Clean build artifacts
    Clean,

    /// Run garbage collection
    Gc {
        /// Target size in GB
        #[arg(long, default_value = "10")]
        target_gb: u64,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Setup logging
    let log_level = if cli.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .init();

    let result = match cli.command {
        Commands::Build {
            kas_file,
            recipe,
            execute,
            io_parallel,
            cpu_parallel,
            max_tasks,
        } => {
            build_command(
                &kas_file,
                recipe.as_deref(),
                &cli.build_dir,
                &cli.workspace,
                io_parallel,
                cpu_parallel.unwrap_or_else(num_cpus::get),
                execute,
                max_tasks,
            )
            .await
        }
        Commands::Clean => clean_command(&cli.build_dir).await,
        Commands::Gc { target_gb } => gc_command(&cli.build_dir, target_gb).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

async fn build_command(
    kas_file: &Path,
    _target_recipe: Option<&str>,
    build_dir: &Path,
    workspace: &Path,
    io_parallel: usize,
    cpu_parallel: usize,
    execute: bool,
    max_tasks: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Bitzel Build ===");
    println!("Kas file: {}", kas_file.display());
    println!("Build dir: {}", build_dir.display());
    println!("Workspace: {}", workspace.display());
    println!();

    // Parse Kas configuration
    println!("[1/6] Parsing Kas configuration...");
    let kas = KasFile::load(kas_file).await?;
    println!("  ✓ Machine: {}", kas.config.machine.as_deref().unwrap_or("none"));
    println!("  ✓ Distro: {}", kas.config.distro.as_deref().unwrap_or("none"));
    println!("  ✓ Repos: {}", kas.config.repos.len());

    // Setup repositories
    println!("\n[2/6] Setting up repositories...");
    let repo_manager = RepositoryManager::new(workspace);

    for (name, config) in &kas.config.repos {
        if config.url.is_some() {
            println!("  Setting up {}...", name);
            let repo_path = repo_manager.setup_repository(name, config).await?;
            println!("    ✓ {}", repo_path.display());
        }
    }

    // Build layer paths
    println!("\n[3/6] Discovering layers...");
    let layer_paths = discover_layers(workspace, &kas)?;
    let total_layers: usize = layer_paths.values().map(|v| v.len()).sum();
    println!("  ✓ Found {} layers across {} repos", total_layers, layer_paths.len());

    // Initialize orchestrator
    println!("\n[4/6] Initializing build orchestrator...");
    let config = OrchestratorConfig {
        build_dir: build_dir.to_path_buf(),
        machine: kas.config.machine.clone(),
        distro: kas.config.distro.clone(),
        max_io_parallelism: io_parallel,
        max_cpu_parallelism: cpu_parallel,
    };
    let orchestrator = BuildOrchestrator::new(config);
    println!("  ✓ I/O parallelism: {}", io_parallel);
    println!("  ✓ CPU parallelism: {}", cpu_parallel);

    // Build dependency graph
    println!("\n[5/6] Building dependency graph...");
    let build_plan = orchestrator.build_plan(layer_paths).await
        .map_err(|e| format!("Failed to build plan: {}", e))?;

    println!("  ✓ Recipes: {}", build_plan.recipe_graph.recipes().count());
    println!("  ✓ Tasks: {}", build_plan.task_graph.tasks.len());
    println!("  ✓ Unchanged: {} ({:.1}%)",
        build_plan.incremental_stats.unchanged,
        build_plan.incremental_stats.unchanged_percent()
    );
    println!("  ✓ Need rebuild: {} ({:.1}%)",
        build_plan.incremental_stats.need_rebuild,
        build_plan.incremental_stats.rebuild_percent()
    );

    // Show what would be built
    println!("\n[6/6] Build plan summary:");
    print_build_summary(&build_plan)?;

    println!("\n=== Build Plan Complete ===");

    // Execute tasks if requested
    if execute && !build_plan.task_specs.is_empty() {
        println!("\n=== Executing Tasks ===");

        // Determine how many tasks to execute
        let total_tasks = build_plan.task_graph.tasks.len();
        let tasks_to_execute = if max_tasks > 0 && max_tasks < total_tasks {
            println!("  Limiting execution to first {} tasks (of {})", max_tasks, total_tasks);
            max_tasks
        } else {
            println!("  Executing {} tasks", total_tasks);
            total_tasks
        };

        // Initialize task executor
        let cache_dir = build_dir.join("hitzeleiter-cache");
        let executor = TaskExecutor::new(&cache_dir)
            .map_err(|e| format!("Failed to create executor: {}", e))?;
        let async_executor = AsyncTaskExecutor::new(executor);

        // Execute tasks
        println!("  Starting parallel execution...\n");

        // If max_tasks is set, limit the task specs
        let task_specs = if max_tasks > 0 {
            build_plan.task_specs.iter()
                .take(max_tasks)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        } else {
            build_plan.task_specs.clone()
        };

        let results = async_executor.execute_graph(&build_plan.task_graph, task_specs).await
            .map_err(|e| format!("Execution failed: {}", e))?;

        // Print execution summary
        println!("\n=== Execution Summary ===");
        println!("  ✓ Tasks executed: {}", results.len());

        let successful = results.values().filter(|r| r.exit_code == 0).count();
        let failed = results.values().filter(|r| r.exit_code != 0).count();

        println!("  ✓ Successful: {}", successful);
        if failed > 0 {
            println!("  ✗ Failed: {}", failed);
        }

        let total_time_ms: u64 = results.values().map(|r| r.duration_ms).sum();
        println!("  ⏱ Total time: {:.2}s", total_time_ms as f64 / 1000.0);

        if failed > 0 {
            println!("\n⚠ Some tasks failed. Check logs for details.");
            return Err("Task execution failed".into());
        }
    } else if !execute {
        println!("\nSkipping execution (use --execute to run tasks)");
    } else {
        println!("\nNo tasks to execute");
    }

    Ok(())
}

async fn clean_command(build_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Bitzel Clean ===");
    println!("Cleaning build directory: {}", build_dir.display());

    if build_dir.exists() {
        std::fs::remove_dir_all(build_dir)?;
        println!("  ✓ Removed build directory");
    } else {
        println!("  ! Build directory does not exist");
    }

    println!("\n=== Clean Complete ===");
    Ok(())
}

async fn gc_command(build_dir: &Path, target_gb: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Bitzel Garbage Collection ===");
    println!("Build dir: {}", build_dir.display());
    println!("Target size: {} GB", target_gb);

    let cache_dir = build_dir.join("hitzeleiter-cache");
    if !cache_dir.exists() {
        println!("  ! No cache found");
        return Ok(());
    }

    // TODO: Implement actual GC using ContentAddressableStore
    println!("  ! GC not yet implemented in CLI");
    println!("\n=== GC Complete ===");

    Ok(())
}

fn discover_layers(
    workspace: &Path,
    kas: &KasFile,
) -> Result<HashMap<String, Vec<PathBuf>>, Box<dyn std::error::Error>> {
    let mut layer_paths: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for (repo_name, config) in &kas.config.repos {
        // Use config.path if specified, otherwise fall back to workspace/repo_name
        let repo_path = if let Some(ref path) = config.path {
            workspace.join(path)
        } else {
            workspace.join(repo_name)
        };

        if !repo_path.exists() {
            continue;
        }

        let mut layers = Vec::new();

        // Add explicitly configured layers
        for layer_name in config.layers.keys() {
            let layer_path = if layer_name.is_empty() || layer_name == "." {
                repo_path.clone()
            } else {
                repo_path.join(layer_name)
            };

            if layer_path.exists() {
                layers.push(layer_path);
            }
        }

        if !layers.is_empty() {
            layer_paths.insert(repo_name.clone(), layers);
        }
    }

    Ok(layer_paths)
}

fn print_build_summary(build_plan: &convenient_bitbake::BuildPlan) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::HashMap;

    // Count tasks by recipe
    let mut tasks_by_recipe: HashMap<String, usize> = HashMap::new();
    for task in build_plan.task_graph.tasks.values() {
        *tasks_by_recipe.entry(task.recipe_name.clone()).or_insert(0) += 1;
    }

    // Find top recipes
    let mut recipe_counts: Vec<_> = tasks_by_recipe.iter().collect();
    recipe_counts.sort_by(|a, b| b.1.cmp(a.1));

    println!("  Top recipes by task count:");
    for (i, (recipe, count)) in recipe_counts.iter().take(10).enumerate() {
        println!("    {}. {}: {} tasks", i + 1, recipe, count);
    }

    // Count tasks by type
    let mut tasks_by_type: HashMap<String, usize> = HashMap::new();
    for task in build_plan.task_graph.tasks.values() {
        *tasks_by_type.entry(task.task_name.clone()).or_insert(0) += 1;
    }

    println!("\n  Task types:");
    let mut type_counts: Vec<_> = tasks_by_type.iter().collect();
    type_counts.sort_by(|a, b| b.1.cmp(a.1));

    for (task_type, count) in type_counts.iter().take(5) {
        println!("    {}: {}", task_type, count);
    }

    Ok(())
}
