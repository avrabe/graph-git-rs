//! Native BitBake build command with Bazel-like sandboxing and caching
//!
//! This command builds BitBake recipes using:
//! - Content-addressable caching (like Bazel Remote Cache)
//! - Native Linux namespace sandboxing (like Bazel sandbox)
//! - Hash-based task signatures for hermetic builds
//! - TaskExecutor for orchestration

use convenient_bitbake::{
    BuildEnvironment, ExtractionConfig, RecipeExtractor,
    Pipeline, PipelineConfig, TaskImplementation, PythonExecutor,
};
use convenient_bitbake::layer_context::BuildContext as LayerBuildContext;
use convenient_bitbake::executor::{
    TaskExecutor, TaskSpec, NetworkPolicy, ResourceLimits,
};

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Expand BitBake variables in a script
/// Expands ${VAR} patterns using the provided environment
fn expand_variables(script: &str, env: &HashMap<String, String>) -> String {
    let mut result = script.to_string();

    // Expand ${VAR} patterns
    loop {
        if let Some(start) = result.find("${") {
            if let Some(end) = result[start..].find('}') {
                let var_name = &result[start + 2..start + end];

                // Skip inline Python expressions ${@...}
                if var_name.starts_with('@') {
                    // TODO: Evaluate inline Python expressions
                    // For now, leave them as-is (will cause bash errors)
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

/// Execute build using native BitBake configuration with TaskExecutor
pub async fn execute(
    build_dir: &Path,
    target: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("╔════════════════════════════════════════════════════════╗");
    println!("║         BITZEL BUILD WITH TASK EXECUTOR               ║");
    println!("║  Bazel-inspired hermetic builds for BitBake/Yocto     ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!();
    println!("Mode: Executor-based (sandboxed + cached)");
    println!("Build directory: {:?}", build_dir);
    println!("Target: {}", target);
    println!();

    // ========== Initialize Task Executor ==========
    println!("⚡ Initializing task executor...");
    let cache_dir = build_dir.join("bitzel-cache");
    let mut executor = TaskExecutor::new(&cache_dir)?;
    println!("  ✓ Task executor ready");
    println!();

    // ========== Load Build Environment ==========
    println!("🏗️  Loading BitBake build environment...");
    let env = BuildEnvironment::from_build_dir(build_dir)?;

    println!("📋 Configuration:");
    println!("  TOPDIR:      {:?}", env.topdir);
    println!("  MACHINE:     {}", env.get_machine().unwrap_or("unknown"));
    println!("  DISTRO:      {}", env.get_distro().unwrap_or("unknown"));
    println!("  DL_DIR:      {:?}", env.dl_dir);
    println!("  TMPDIR:      {:?}", env.tmpdir);
    println!("  Layers:      {}", env.layers.len());
    for (i, layer) in env.layers.iter().enumerate() {
        println!("    {}. {:?}", i + 1, layer);
    }
    println!();

    // ========== Create Build Context ==========
    println!("🔧 Creating build context...");
    let layer_build_context = env.create_build_context()?;
    println!("  Loaded {} layers with priorities", layer_build_context.layers.len());
    for layer in &layer_build_context.layers {
        println!("    • {} (priority: {})", layer.collection, layer.priority);
    }
    println!();

    // ========== Parse Recipes with Parallel Pipeline ==========
    println!("🔍 Discovering and parsing BitBake recipes...");
    let pipeline_config = PipelineConfig {
        max_io_parallelism: 32,
        max_cpu_parallelism: num_cpus::get(),
        enable_cache: true,
        cache_dir: build_dir.join("bitzel-cache/pipeline"),
    };

    let pipeline = Pipeline::new(pipeline_config, layer_build_context);

    // Create ExtractionConfig with default BuildContext (different type!)
    let extraction_config = ExtractionConfig {
        use_python_executor: false,
        use_simple_python_eval: true,
        use_python_ir: false,
        default_variables: HashMap::new(),
        extract_tasks: true,
        resolve_providers: true,
        resolve_includes: true,
        resolve_inherit: true,
        extract_class_deps: true,
        class_search_paths: vec![],
        build_context: Default::default(),  // recipe_extractor::BuildContext
    };
    let mut layer_paths: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();
    for (i, layer) in env.layers.iter().enumerate() {
        let layer_name = format!("layer_{}", i);
        layer_paths.insert(layer_name, vec![layer.clone()]);
    }

    // Use Pipeline's 3-stage process: discover -> parse -> build_graph
    let (recipe_files, _discover_hash) = pipeline.discover_recipes(&layer_paths).await?;
    let (parsed_recipes, _parse_hash) = pipeline.parse_recipes(recipe_files).await?;

    let extractor = RecipeExtractor::new(extraction_config);
    let (graph, _graph_hash) = pipeline.build_recipe_graph(&parsed_recipes, &extractor)?;
    let stats = graph.statistics();
    println!("  Recipes:       {}", graph.recipe_count());
    println!("  Tasks:         {}", graph.task_count());
    println!("  Dependencies:  {}", stats.total_dependencies);
    println!();

    // ========== Find Target Recipe ==========
    println!("🎯 Looking for target recipe: {}", target);
    let recipe_id = graph.find_recipe(target)
        .ok_or_else(|| format!("Recipe '{}' not found", target))?;
    let recipe = graph.get_recipe(recipe_id)
        .ok_or_else(|| format!("Recipe '{}' not found in graph", target))?;

    // Find parsed recipe with task implementations
    let parsed_recipe = parsed_recipes.iter()
        .find(|p| p.file.name == recipe.name)
        .ok_or_else(|| format!("Parsed recipe '{}' not found", target))?;

    println!("  Found recipe: {} {}", recipe.name, recipe.version.as_deref().unwrap_or("unknown"));
    println!();

    // ========== Setup Build Environment ==========
    let machine = env.get_machine().unwrap_or("unknown");
    let recipe_workdir = env.tmpdir.join("work").join(machine).join(&recipe.name)
        .join(recipe.version.as_deref().unwrap_or("unknown"));

    println!("🚀 Setting up build environment:");
    println!("   Target: {} {}", recipe.name, recipe.version.as_deref().unwrap_or("unknown"));
    std::fs::create_dir_all(&recipe_workdir)?;
    println!("  ✓ Work directory: {:?}", recipe_workdir);
    println!();

    // ========== Setup BitBake Variables ==========
    let mut initial_vars = HashMap::new();
    initial_vars.insert("PN".to_string(), recipe.name.clone());
    initial_vars.insert("PV".to_string(), recipe.version.clone().unwrap_or_default());
    initial_vars.insert("MACHINE".to_string(), machine.to_string());
    initial_vars.insert("WORKDIR".to_string(), recipe_workdir.to_string_lossy().to_string());
    initial_vars.insert("S".to_string(), recipe_workdir.join("src").to_string_lossy().to_string());
    initial_vars.insert("B".to_string(), recipe_workdir.join("build").to_string_lossy().to_string());
    initial_vars.insert("D".to_string(), recipe_workdir.join("image").to_string_lossy().to_string());
    initial_vars.insert("sysconfdir".to_string(), "/etc".to_string());
    initial_vars.insert("bindir".to_string(), "/usr/bin".to_string());
    initial_vars.insert("libdir".to_string(), "/usr/lib".to_string());
    initial_vars.insert("nonarch_libdir".to_string(), "/usr/lib".to_string());
    initial_vars.insert("localstatedir".to_string(), "/var".to_string());

    // ========== Execute Task ==========
    println!("🔍 Looking for task to execute...");

    // Find a task to execute from parsed recipe (preferring do_compile or do_install)
    let task_impl = parsed_recipe.task_impls.get("do_install")
        .or_else(|| parsed_recipe.task_impls.get("do_compile"))
        .or_else(|| parsed_recipe.task_impls.values().next())
        .ok_or_else(|| format!("No task implementations found in recipe '{}'", target))?;

    println!("  Found {} implementation ({} bytes)", task_impl.name, task_impl.code.len());
    println!();

    // ========== Execute with TaskExecutor ==========
    println!("⚡ Executing {} with TaskExecutor...", task_impl.name);

    // Expand variables in the script
    let expanded_script = expand_variables(&task_impl.code, &initial_vars);

    // Determine network policy based on task name
    let network_policy = if task_impl.name == "do_fetch" {
        NetworkPolicy::FullNetwork
    } else {
        NetworkPolicy::Isolated
    };

    // Create TaskSpec
    let task_spec = TaskSpec {
        name: task_impl.name.clone(),
        recipe: recipe.name.clone(),
        script: expanded_script,
        workdir: recipe_workdir.clone(),
        env: initial_vars.clone(),
        outputs: vec![],  // Auto-detect outputs
        timeout: Some(Duration::from_secs(600)),  // 10 minute timeout
        network_policy,
        resource_limits: ResourceLimits::default(),
    };

    println!("  Mode: Shell (Linux namespace sandbox)");
    println!("  Network: {:?}", task_spec.network_policy);
    println!("  Resource limits: 4GB memory, 1024 PIDs");
    println!();

    // Execute!
    match executor.execute_task(task_spec) {
        Ok(output) => {
            if output.exit_code == 0 {
                println!("  ✓ Task succeeded (exit code: {})", output.exit_code);
            } else {
                println!("  ✗ Task FAILED (exit code: {})", output.exit_code);
            }

            if !output.stdout.is_empty() {
                println!("  Stdout: {} bytes", output.stdout.len());
            }

            if !output.stderr.is_empty() {
                println!("  Stderr ({} bytes):", output.stderr.len());
                let preview = if output.stderr.len() > 500 {
                    format!("{}...\n[{} more bytes]", &output.stderr[..500], output.stderr.len() - 500)
                } else {
                    output.stderr.clone()
                };
                for line in preview.lines() {
                    println!("    {}", line);
                }
            }

            println!("  Output files: {}", output.output_files.len());
            for (path, hash) in &output.output_files {
                println!("    {:?} ({})", path, hash);
            }

            // Create stamp file
            let stamp_dir = env.tmpdir.join("stamps").join(machine).join(&recipe.name);
            std::fs::create_dir_all(&stamp_dir)?;
            let stamp_file = stamp_dir.join(format!("{}.{}", task_impl.name, output.signature));
            std::fs::write(&stamp_file, "")?;
            println!("  ✓ Created stamp file");
            println!();

            if output.exit_code != 0 {
                return Err(format!("Task failed with exit code {}", output.exit_code).into());
            }

            println!("✅ Build complete!");
            println!("   Cache efficiency will improve on subsequent builds");
        }
        Err(e) => {
            eprintln!("❌ Task execution failed: {}", e);
            return Err(Box::new(e));
        }
    }

    Ok(())
}
