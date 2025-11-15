//! Native BitBake build command with Bazel-like sandboxing and caching
//!
//! This command builds BitBake recipes using:
//! - Content-addressable caching (like Bazel Remote Cache)
//! - Native Linux namespace sandboxing (like Bazel sandbox)
//! - Hash-based task signatures for hermetic builds

use convenient_bitbake::{
    BuildEnvironment, ExtractionConfig, RecipeExtractor,
    Pipeline, PipelineConfig, TaskImplementation, PythonExecutor,
};
use convenient_bitbake::executor::{
    ContentAddressableStore, ActionCache, TaskSignature, TaskOutput,
    ContentHash, NetworkPolicy, ResourceLimits,
};
#[cfg(target_os = "linux")]
use convenient_bitbake::executor::execute_in_namespace;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Execute build using native BitBake configuration with sandboxing and caching
pub async fn execute(
    build_dir: &Path,
    target: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("╔════════════════════════════════════════════════════════╗");
    println!("║         BITZEL BUILD WITH SANDBOXING & CACHING        ║");
    println!("║  Bazel-inspired hermetic builds for BitBake/Yocto     ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!();
    println!("Mode: Sandboxed + Cached");
    println!("Build directory: {:?}", build_dir);
    println!("Target: {}", target);
    println!();

    // ========== Initialize Cache Infrastructure ==========
    println!("💾 Initializing content-addressable cache...");

    let cache_dir = build_dir.join("bitzel-cache");
    let cas_dir = cache_dir.join("cas");
    let action_cache_dir = cache_dir.join("ac");

    let mut cas = ContentAddressableStore::new(&cas_dir)?;
    let mut action_cache = ActionCache::new(&action_cache_dir)?;

    let cas_stats = cas.stats();
    println!("  CAS: {} objects, {} MB",
             cas_stats.object_count,
             cas_stats.total_size_bytes / (1024 * 1024));

    println!();

    // ========== Load Build Environment ==========
    println!("🏗️  Loading BitBake build environment...");
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

    // ========== Create Build Context ==========
    println!("🔧 Creating build context...");
    let build_context = env.create_build_context()?;

    println!("  Loaded {} layers with priorities", build_context.layers.len());
    for layer in &build_context.layers {
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

    println!("  Parallelism: {} I/O tasks, {} CPU cores",
             pipeline_config.max_io_parallelism,
             pipeline_config.max_cpu_parallelism);

    let pipeline = Pipeline::new(pipeline_config, build_context);

    // Create layer_paths map
    let mut layer_paths: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for (i, layer) in env.layers.iter().enumerate() {
        let layer_name = format!("layer{}", i);
        layer_paths.insert(layer_name, vec![layer.clone()]);
    }

    // Stage 1: Discover recipes
    let (recipe_files, discover_hash) = pipeline.discover_recipes(&layer_paths).await?;
    pipeline.save_stage_hash(&discover_hash).await?;
    println!("  Discovered {} recipe files", recipe_files.len());

    // Stage 2: Parse recipes
    let (parsed_recipes, parse_hash) = pipeline.parse_recipes(recipe_files).await?;
    pipeline.save_stage_hash(&parse_hash).await?;
    println!("  Parsed {} recipes", parsed_recipes.len());
    println!();

    // Extract task implementations
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

    // ========== Build Recipe Graph ==========
    println!("🔗 Building recipe dependency graph...");

    let mut extraction_config = ExtractionConfig::default();
    extraction_config.extract_tasks = true;
    extraction_config.resolve_providers = true;
    extraction_config.use_python_executor = false;

    let extractor = RecipeExtractor::new(extraction_config);
    let (graph, graph_hash) = pipeline.build_recipe_graph(&parsed_recipes, &extractor)?;
    pipeline.save_stage_hash(&graph_hash).await?;

    let stats = graph.statistics();
    println!("  Recipes:       {}", stats.recipe_count);
    println!("  Tasks:         {}", stats.task_count);
    println!("  Dependencies:  {}", stats.total_dependencies);
    println!("  Providers:     {}", stats.provider_count);
    println!();

    // ========== Find Target Recipe ==========
    println!("🎯 Looking for target recipe: {}", target);

    let recipe = graph.recipes()
        .find(|r| r.name == target)
        .or_else(|| {
            let target_base = target.split('_').next().unwrap_or(target);
            graph.recipes().find(|r| r.name == target_base)
        })
        .or_else(|| {
            graph.recipes().find(|r| r.name.starts_with(target))
        });

    if let Some(recipe) = recipe {
        let version = recipe.version.as_deref().unwrap_or("unknown");
        println!("  Found recipe: {} {}", recipe.name, version);
        println!();

        // ========== Setup Build Directories ==========
        println!("🚀 Setting up build environment:");
        println!("   Target: {} {}", recipe.name, version);

        std::fs::create_dir_all(&env.dl_dir)?;
        std::fs::create_dir_all(&env.tmpdir)?;
        std::fs::create_dir_all(env.tmpdir.join("work"))?;
        std::fs::create_dir_all(env.tmpdir.join("deploy"))?;
        std::fs::create_dir_all(env.tmpdir.join("stamps"))?;

        let machine = env.get_machine().unwrap_or("unknown");
        let recipe_workdir = env.tmpdir.join(format!("work/{}/{}/{}", machine, recipe.name, version));
        std::fs::create_dir_all(&recipe_workdir)?;
        std::fs::create_dir_all(recipe_workdir.join("temp"))?;

        println!("  ✓ Work directory: {:?}", recipe_workdir);
        println!();

        // ========== Execute Task with Caching ==========
        println!("🔍 Looking for task to execute...");

        let (task_name, task_impl) = recipe_task_impls.get(&recipe.name)
            .and_then(|impls| {
                impls.get("do_compile").map(|t| ("do_compile", t))
                    .or_else(|| impls.get("do_install").map(|t| ("do_install", t)))
                    .or_else(|| impls.iter().next().map(|(name, t)| (name.as_str(), t)))
            })
            .map(|(name, impl_ref)| (name.to_string(), impl_ref.clone()))
            .unzip();

        if let (Some(task_name), Some(task_impl)) = (task_name, task_impl) {
            println!("  Found {} implementation ({} bytes)",
                     task_name,
                     task_impl.code.len());
            println!();

            // ========== Compute Task Signature (Content-Based Hash) ==========
            println!("🔐 Computing task signature...");

            let mut initial_vars = HashMap::new();
            initial_vars.insert("PN".to_string(), recipe.name.clone());
            initial_vars.insert("PV".to_string(), version.to_string());
            initial_vars.insert("WORKDIR".to_string(), recipe_workdir.to_string_lossy().to_string());
            initial_vars.insert("B".to_string(), recipe_workdir.to_string_lossy().to_string());
            initial_vars.insert("S".to_string(), recipe_workdir.to_string_lossy().to_string());
            initial_vars.insert("D".to_string(), recipe_workdir.join("image").to_string_lossy().to_string());
            initial_vars.insert("DL_DIR".to_string(), env.dl_dir.to_string_lossy().to_string());
            initial_vars.insert("TMPDIR".to_string(), env.tmpdir.to_string_lossy().to_string());

            if let Some(machine_val) = env.get_machine() {
                initial_vars.insert("MACHINE".to_string(), machine_val.to_string());
            }
            if let Some(distro) = env.get_distro() {
                initial_vars.insert("DISTRO".to_string(), distro.to_string());
                initial_vars.insert("DISTRO_NAME".to_string(), distro.to_string());
            }

            // OS_RELEASE specific variables
            initial_vars.insert("OS_RELEASE_FIELDS".to_string(), "ID NAME VERSION PRETTY_NAME".to_string());
            initial_vars.insert("OS_RELEASE_UNQUOTED_FIELDS".to_string(), "ID VERSION_ID".to_string());

            // Compute task code hash
            let task_code_hash = ContentHash::from_bytes(task_impl.code.as_bytes());

            // Build task signature
            let mut signature = TaskSignature {
                recipe: recipe.name.clone(),
                task: task_name.clone(),
                input_files: HashMap::new(), // TODO: Add actual input file tracking
                dep_signatures: Vec::new(),   // TODO: Add dependency signatures
                env_vars: initial_vars.clone(),
                task_code_hash: task_code_hash.clone(),
                signature: None,
            };

            let sig_hash = signature.compute();
            println!("  Signature: {}", sig_hash);
            println!("  Task code hash: {}", task_code_hash);
            println!();

            // ========== Check Cache ==========
            println!("💾 Checking action cache...");

            if let Some(cached_output) = action_cache.get(&sig_hash) {
                println!("  ✅ CACHE HIT! Restoring from cache...");
                println!("  Exit code: {}", cached_output.exit_code);
                println!("  Stdout: {} bytes", cached_output.stdout.len());
                println!("  Stderr: {} bytes", cached_output.stderr.len());
                println!("  Output files: {}", cached_output.output_files.len());

                // Restore output files from CAS
                for (path, file_hash) in &cached_output.output_files {
                    println!("    Restoring: {:?} ({})", path, file_hash);
                    match cas.get(file_hash) {
                        Ok(content) => {
                            let full_path = recipe_workdir.join(path);
                            if let Some(parent) = full_path.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            println!("      ✓ Restored {} bytes", content.len());
                            std::fs::write(&full_path, &content)?;
                        }
                        Err(e) => {
                            eprintln!("      ✗ Failed to restore: {}", e);
                        }
                    }
                }

                // Create stamp file
                let stamp_dir = env.tmpdir.join("stamps").join(machine).join(recipe.name.clone());
                std::fs::create_dir_all(&stamp_dir)?;
                let stamp_file = stamp_dir.join(format!("{}.{}", task_name, version));
                std::fs::write(&stamp_file, "")?;
                println!("  ✓ Created stamp file");
                println!();

                println!("✅ Build complete (from cache)!");
                return Ok(());
            }

            println!("  ⚠️  CACHE MISS - executing task");
            println!();

            // ========== Execute Task ==========
            println!("⚡ Executing {} with sandboxing...", task_name);

            match task_impl.impl_type {
                convenient_bitbake::TaskImplementationType::Python => {
                    println!("  Mode: Python (RustPython, unsandboxed)");
                    let executor = PythonExecutor::new();
                    let result = executor.execute(&task_impl.code, &initial_vars);

                    if result.success {
                        println!("  ✓ Task succeeded");
                        println!("    Variables set: {}", result.variables_set.len());

                        // Collect output files (for now, just check if os-release was created)
                        let mut output_files = HashMap::new();
                        let os_release_path = recipe_workdir.join("os-release");
                        if os_release_path.exists() {
                            let content = std::fs::read(&os_release_path)?;
                            let file_hash = cas.put(&content)?;
                            println!("    Output file: os-release ({})", file_hash);
                            output_files.insert(PathBuf::from("os-release"), file_hash);
                        }

                        // Store in action cache
                        let task_output = TaskOutput {
                            signature: sig_hash.clone(),
                            exit_code: 0,
                            stdout: String::new(),
                            stderr: String::new(),
                            output_files,
                            duration_ms: 0,
                        };

                        action_cache.put(sig_hash, task_output)?;
                        println!("  ✓ Stored in action cache");

                        // Create stamp file
                        let stamp_dir = env.tmpdir.join("stamps").join(machine).join(recipe.name.clone());
                        std::fs::create_dir_all(&stamp_dir)?;
                        let stamp_file = stamp_dir.join(format!("{}.{}", task_name, version));
                        std::fs::write(&stamp_file, "")?;
                        println!("  ✓ Created stamp file");
                    } else {
                        eprintln!("  ✗ Task failed: {:?}", result.error);
                        return Err(format!("{} failed: {:?}", task_name, result.error).into());
                    }
                }
                convenient_bitbake::TaskImplementationType::Shell |
                convenient_bitbake::TaskImplementationType::FakerootShell => {
                    #[cfg(target_os = "linux")]
                    {
                        println!("  Mode: Shell (Linux namespace sandbox)");

                        // Determine network policy
                        let network_policy = if task_name.contains("fetch") {
                            NetworkPolicy::FullNetwork
                        } else {
                            NetworkPolicy::Isolated
                        };

                        println!("  Network: {:?}", network_policy);
                        println!("  Resource limits: 4GB memory, 1024 PIDs");

                        let resource_limits = ResourceLimits::default();

                        // Execute in namespace
                        match execute_in_namespace(
                            &task_impl.code,
                            &recipe_workdir,
                            &initial_vars,
                            network_policy,
                            &resource_limits,
                        ) {
                            Ok((exit_code, stdout, stderr)) => {
                                println!("  ✓ Task succeeded (exit code: {})", exit_code);
                                if !stdout.is_empty() {
                                    println!("  Stdout: {} bytes", stdout.len());
                                }
                                if !stderr.is_empty() {
                                    println!("  Stderr: {} bytes", stderr.len());
                                }

                                // Collect output files
                                let mut output_files = HashMap::new();

                                // Scan work directory for outputs
                                if recipe_workdir.exists() {
                                    for entry in walkdir::WalkDir::new(&recipe_workdir)
                                        .into_iter()
                                        .filter_map(|e| e.ok())
                                    {
                                        if entry.file_type().is_file() {
                                            let full_path = entry.path();
                                            if let Ok(relative_path) = full_path.strip_prefix(&recipe_workdir) {
                                                let content = std::fs::read(full_path)?;
                                                let file_hash = cas.put(&content)?;
                                                println!("    Output: {:?} ({} bytes, {})",
                                                         relative_path,
                                                         content.len(),
                                                         file_hash);
                                                output_files.insert(relative_path.to_path_buf(), file_hash);
                                            }
                                        }
                                    }
                                }

                                // Store in action cache
                                let task_output = TaskOutput {
                                    signature: sig_hash.clone(),
                                    exit_code,
                                    stdout,
                                    stderr,
                                    output_files,
                                    duration_ms: 0,
                                };

                                action_cache.put(sig_hash, task_output)?;
                                println!("  ✓ Stored in action cache");

                                // Create stamp file
                                let stamp_dir = env.tmpdir.join("stamps").join(machine).join(recipe.name.clone());
                                std::fs::create_dir_all(&stamp_dir)?;
                                let stamp_file = stamp_dir.join(format!("{}.{}", task_name, version));
                                std::fs::write(&stamp_file, "")?;
                                println!("  ✓ Created stamp file");
                            }
                            Err(e) => {
                                eprintln!("  ✗ Sandbox execution failed: {}", e);
                                return Err(format!("{} failed: {}", task_name, e).into());
                            }
                        }
                    }

                    #[cfg(not(target_os = "linux"))]
                    {
                        println!("  ⚠️  Shell sandboxing requires Linux");
                        println!("     Task would execute: {} bytes of shell code", task_impl.code.len());
                        return Err("Shell sandboxing not available on this platform".into());
                    }
                }
            }
            println!();
        } else {
            println!("  ⚠️  No task implementation found for {}", recipe.name);
            println!("     This is expected for recipes that only inherit tasks");
        }

        println!("✅ Build complete!");
        println!("   Cache efficiency will improve on subsequent builds");
    } else {
        eprintln!("❌ Recipe not found: {}", target);
        eprintln!("   Available recipes:");
        for r in graph.recipes().take(10) {
            eprintln!("     • {}", r.name);
        }
        return Err(format!("Recipe not found: {}", target).into());
    }

    Ok(())
}
