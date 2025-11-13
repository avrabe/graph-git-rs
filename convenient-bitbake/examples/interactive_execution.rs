//! Interactive task execution with monitoring and debugging
//!
//! Demonstrates:
//! 1. Real-time task monitoring with state tracking
//! 2. Human-readable and machine-readable (JSON) output
//! 3. Interactive pause/resume/debug capabilities
//! 4. Break-on-failure for detailed error inspection
//! 5. Performance statistics and timing analysis

use convenient_bitbake::executor::{InteractiveExecutor, InteractiveOptions, TaskSpec};
use convenient_bitbake::recipe_graph::{RecipeGraph, TaskDependency};
use convenient_bitbake::task_graph::TaskGraphBuilder;
use std::collections::HashMap;
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for detailed timing information
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║  Bitzel Interactive Execution Demo                           ║");
    println!("║  Real-time monitoring, debugging, and performance analysis   ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    // Create temporary directories
    let tmp = TempDir::new()?;
    let cache_dir = tmp.path().join("cache");
    let work_dir = tmp.path().join("work");
    let json_output = tmp.path().join("execution_report.json");
    std::fs::create_dir_all(&work_dir)?;

    // Build recipe graph with realistic dependencies
    println!("📋 Building recipe graph...");
    let recipe_graph = build_recipe_graph();
    let builder = TaskGraphBuilder::new(recipe_graph);
    let task_graph = builder.build_full_graph()?;
    let stats = task_graph.stats();

    println!("   ✓ Recipes: 4");
    println!("   ✓ Total tasks: {}", stats.total_tasks);
    println!("   ✓ Root tasks: {}", stats.root_tasks);
    println!("   ✓ Dependency depth: {}\n", stats.max_depth);

    // Configure interactive options
    let options = InteractiveOptions {
        break_on_failure: true,      // Pause and debug on any failure
        interactive_mode: false,      // Set to true for step-by-step execution
        show_progress: true,          // Show progress bar
        export_json: Some(json_output.clone()), // Export execution report
    };

    // Create interactive executor
    let mut executor = InteractiveExecutor::new(&cache_dir, options)?;

    // Get control handle for potential external control
    let _control = executor.control_handle();

    // Create task specifications
    println!("⚙️  Configuring task specifications...");
    let task_specs = create_task_specs(&task_graph, &work_dir);
    println!("   ✓ {} task specs ready\n", task_specs.len());

    // Execute with real-time monitoring
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║  Starting Execution                                           ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");

    let _results = executor.execute_graph(&task_graph, task_specs)?;

    // Display final task list
    executor.monitor().print_tasks();

    // Show JSON output location
    println!("\n📄 Execution Report");
    println!("──────────────────────────────────────────────────────────");
    println!("   Location: {}", json_output.display());
    println!("   Format: JSON (machine-readable)");
    println!("   Use with: jq, OpenTelemetry, monitoring tools\n");

    // Show sample of JSON
    let json_content = std::fs::read_to_string(&json_output)?;
    let parsed: serde_json::Value = serde_json::from_str(&json_content)?;
    println!("   Sample JSON structure:");
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "version": parsed["version"],
        "timestamp": parsed["timestamp"],
        "statistics": {
            "total_tasks": parsed["statistics"]["total_tasks"],
            "completed": parsed["statistics"]["completed"],
            "cache_hit_rate": parsed["statistics"]["cache_hit_rate"],
        },
        "tasks": format!("[{} task objects...]", parsed["tasks"].as_array().map(|a| a.len()).unwrap_or(0))
    }))?);

    println!("\n╔═══════════════════════════════════════════════════════════════╗");
    println!("║  Interactive Features Available                               ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!("\n1. Break on Failure: Automatically pauses when tasks fail");
    println!("   - Inspect task state and error details");
    println!("   - View task dependencies and outputs");
    println!("   - Continue or abort execution");
    println!("\n2. Real-time Monitoring:");
    println!("   - Task state tracking (pending/running/completed/failed)");
    println!("   - Performance timing per task");
    println!("   - Cache hit/miss tracking");
    println!("   - Progress visualization");
    println!("\n3. Machine-Readable Output:");
    println!("   - JSON export with full execution trace");
    println!("   - Compatible with monitoring tools");
    println!("   - Structured for log analysis");
    println!("\n4. Performance Analysis:");
    println!("   - Total execution time");
    println!("   - Per-task duration");
    println!("   - Identify slowest tasks");
    println!("   - Cache effectiveness metrics");

    println!("\n✨ Try enabling interactive_mode: true for step-by-step execution!");

    Ok(())
}

/// Build a realistic recipe graph with multiple recipes and dependencies
fn build_recipe_graph() -> RecipeGraph {
    let mut graph = RecipeGraph::new();

    // Recipe 1: base-system (no dependencies)
    let base_system = graph.add_recipe("base-system");
    let base_fetch = graph.add_task(base_system, "do_fetch");
    let base_unpack = graph.add_task(base_system, "do_unpack");
    let base_install = graph.add_task(base_system, "do_install");
    let base_populate = graph.add_task(base_system, "do_populate_sysroot");

    if let Some(task) = graph.get_task_mut(base_unpack) {
        task.after.push(base_fetch);
    }
    if let Some(task) = graph.get_task_mut(base_install) {
        task.after.push(base_unpack);
    }
    if let Some(task) = graph.get_task_mut(base_populate) {
        task.after.push(base_install);
    }

    // Recipe 2: toolchain (depends on base-system)
    let toolchain = graph.add_recipe("toolchain");
    if let Some(recipe) = graph.get_recipe_mut(toolchain) {
        recipe.depends.push(base_system);
    }

    let tc_fetch = graph.add_task(toolchain, "do_fetch");
    let tc_unpack = graph.add_task(toolchain, "do_unpack");
    let tc_configure = graph.add_task(toolchain, "do_configure");
    let tc_compile = graph.add_task(toolchain, "do_compile");
    let tc_install = graph.add_task(toolchain, "do_install");
    let tc_populate = graph.add_task(toolchain, "do_populate_sysroot");

    if let Some(task) = graph.get_task_mut(tc_unpack) {
        task.after.push(tc_fetch);
    }
    if let Some(task) = graph.get_task_mut(tc_configure) {
        task.after.push(tc_unpack);
        task.task_depends.push(TaskDependency {
            recipe_id: base_system,
            task_name: "do_populate_sysroot".to_string(),
            task_id: Some(base_populate),
        });
    }
    if let Some(task) = graph.get_task_mut(tc_compile) {
        task.after.push(tc_configure);
    }
    if let Some(task) = graph.get_task_mut(tc_install) {
        task.after.push(tc_compile);
    }
    if let Some(task) = graph.get_task_mut(tc_populate) {
        task.after.push(tc_install);
    }

    // Recipe 3: library (depends on toolchain)
    let library = graph.add_recipe("mylib");
    if let Some(recipe) = graph.get_recipe_mut(library) {
        recipe.depends.push(toolchain);
    }

    let lib_fetch = graph.add_task(library, "do_fetch");
    let lib_unpack = graph.add_task(library, "do_unpack");
    let lib_configure = graph.add_task(library, "do_configure");
    let lib_compile = graph.add_task(library, "do_compile");
    let lib_install = graph.add_task(library, "do_install");
    let lib_populate = graph.add_task(library, "do_populate_sysroot");

    if let Some(task) = graph.get_task_mut(lib_unpack) {
        task.after.push(lib_fetch);
    }
    if let Some(task) = graph.get_task_mut(lib_configure) {
        task.after.push(lib_unpack);
        task.task_depends.push(TaskDependency {
            recipe_id: toolchain,
            task_name: "do_populate_sysroot".to_string(),
            task_id: Some(tc_populate),
        });
    }
    if let Some(task) = graph.get_task_mut(lib_compile) {
        task.after.push(lib_configure);
    }
    if let Some(task) = graph.get_task_mut(lib_install) {
        task.after.push(lib_compile);
    }
    if let Some(task) = graph.get_task_mut(lib_populate) {
        task.after.push(lib_install);
    }

    // Recipe 4: application (depends on library)
    let app = graph.add_recipe("myapp");
    if let Some(recipe) = graph.get_recipe_mut(app) {
        recipe.depends.push(library);
    }

    let app_fetch = graph.add_task(app, "do_fetch");
    let app_unpack = graph.add_task(app, "do_unpack");
    let app_compile = graph.add_task(app, "do_compile");
    let app_install = graph.add_task(app, "do_install");
    let app_package = graph.add_task(app, "do_package");

    if let Some(task) = graph.get_task_mut(app_unpack) {
        task.after.push(app_fetch);
    }
    if let Some(task) = graph.get_task_mut(app_compile) {
        task.after.push(app_unpack);
        task.task_depends.push(TaskDependency {
            recipe_id: library,
            task_name: "do_populate_sysroot".to_string(),
            task_id: Some(lib_populate),
        });
    }
    if let Some(task) = graph.get_task_mut(app_install) {
        task.after.push(app_compile);
    }
    if let Some(task) = graph.get_task_mut(app_package) {
        task.after.push(app_install);
    }

    graph
}

/// Create task specifications with realistic scripts
fn create_task_specs(
    task_graph: &convenient_bitbake::task_graph::TaskGraph,
    work_dir: &std::path::Path,
) -> HashMap<String, TaskSpec> {
    let mut specs = HashMap::new();

    for task in task_graph.tasks.values() {
        let task_key = format!("{}:{}", task.recipe_name, task.task_name);
        let task_work_dir = work_dir.join(&task.recipe_name);

        let (script, sleep_time) = match task.task_name.as_str() {
            "do_fetch" => (
                format!(
                    r#"
mkdir -p "$D"
echo "[{}] Fetching sources..."
echo "SRC_URI=https://example.com/{}.tar.gz" > "$D/fetch.log"
echo "Downloaded successfully" > "$D/fetch.stamp"
sleep {}
                    "#,
                    task.recipe_name, task.recipe_name, 0.1
                ),
                0.1,
            ),
            "do_unpack" => (
                format!(
                    r#"
mkdir -p "$S"
echo "[{}] Unpacking sources..."
echo "int main() {{ return 0; }}" > "$S/main.c"
echo "CC=gcc" > "$S/Makefile"
echo "all:" >> "$S/Makefile"
echo -e "\t$(CC) -o bin main.c" >> "$S/Makefile"
echo "Unpacked" > "$D/unpack.stamp"
sleep {}
                    "#,
                    task.recipe_name, 0.08
                ),
                0.08,
            ),
            "do_configure" => (
                format!(
                    r#"
mkdir -p "$B"
echo "[{}] Configuring..."
echo "Configuration complete" > "$D/configure.log"
echo "Configured" > "$D/configure.stamp"
sleep {}
                    "#,
                    task.recipe_name, 0.12
                ),
                0.12,
            ),
            "do_compile" => (
                format!(
                    r#"
mkdir -p "$D"
echo "[{}] Compiling..."
cd "$S"
if [ -f Makefile ]; then
    make all 2>&1 | tee "$D/compile.log"
fi
echo "Compiled" > "$D/compile.stamp"
sleep {}
                    "#,
                    task.recipe_name, 0.15
                ),
                0.15,
            ),
            "do_install" => (
                format!(
                    r#"
mkdir -p "$D/usr/bin"
echo "[{}] Installing..."
[ -f "$S/bin" ] && cp "$S/bin" "$D/usr/bin/" || true
echo "Installed" > "$D/install.stamp"
sleep {}
                    "#,
                    task.recipe_name, 0.07
                ),
                0.07,
            ),
            "do_populate_sysroot" => (
                format!(
                    r#"
mkdir -p "$D/sysroot"
echo "[{}] Populating sysroot..."
cp -r "$WORKDIR"/* "$D/sysroot/" 2>/dev/null || true
echo "Sysroot ready" > "$D/populate.stamp"
sleep {}
                    "#,
                    task.recipe_name, 0.06
                ),
                0.06,
            ),
            "do_package" => (
                format!(
                    r#"
mkdir -p "$D"
echo "[{}] Creating package..."
cd "$D"
tar czf {}.tar.gz * 2>/dev/null || true
echo "Packaged" > "$D/package.stamp"
sleep {}
                    "#,
                    task.recipe_name, task.recipe_name, 0.09
                ),
                0.09,
            ),
            _ => (
                format!(
                    r#"
mkdir -p "$D"
echo "[{}] Executing {}..."
echo "Done" > "$D/{}.stamp"
                    "#,
                    task.recipe_name, task.task_name, task.task_name
                ),
                0.05,
            ),
        };

        let spec = TaskSpec {
            name: task.task_name.clone(),
            recipe: task.recipe_name.clone(),
            script,
            workdir: task_work_dir,
            env: HashMap::new(),
            outputs: vec![],
            timeout: Some(30),
        };

        specs.insert(task_key, spec);
    }

    specs
}
