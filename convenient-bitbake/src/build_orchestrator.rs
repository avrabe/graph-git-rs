//! High-level build orchestration
//!
//! Provides a clean API for orchestrating the full build pipeline from
//! layer discovery through task graph generation.

use crate::{
    BuildContext, ExtractionConfig, Pipeline, PipelineConfig,
    RecipeExtractor, RecipeGraph, SignatureCache, TaskGraph,
    TaskGraphBuilder, TaskImplementation, TaskSpec,
};
use crate::executor::types::{NetworkPolicy, ResourceLimits};
use crate::executor::ScriptPreprocessor;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, trace, warn};

/// Configuration for build orchestration
pub struct OrchestratorConfig {
    /// Build directory for outputs and cache
    pub build_dir: PathBuf,

    /// MACHINE setting
    pub machine: Option<String>,

    /// DISTRO setting
    pub distro: Option<String>,

    /// Maximum I/O parallelism
    pub max_io_parallelism: usize,

    /// Maximum CPU parallelism
    pub max_cpu_parallelism: usize,
}

/// Result of build orchestration
pub struct BuildPlan {
    /// Build context with all layers
    pub build_context: BuildContext,

    /// Recipe dependency graph
    pub recipe_graph: RecipeGraph,

    /// Task execution graph
    pub task_graph: TaskGraph,

    /// Task specifications ready for execution
    pub task_specs: HashMap<String, TaskSpec>,

    /// Task implementations extracted from recipes
    pub task_implementations: HashMap<String, HashMap<String, TaskImplementation>>,

    /// Helper function implementations extracted from recipes
    pub helper_implementations: HashMap<String, HashMap<String, TaskImplementation>>,

    /// Signature cache for incremental builds
    pub signature_cache: SignatureCache,

    /// Incremental build statistics
    pub incremental_stats: IncrementalStats,
}

/// Statistics about incremental build analysis
#[derive(Debug, Clone)]
pub struct IncrementalStats {
    /// Total number of tasks
    pub total_tasks: usize,

    /// Number of tasks unchanged since last build
    pub unchanged: usize,

    /// Number of tasks that need rebuilding
    pub need_rebuild: usize,

    /// Number of new tasks (not in cache)
    pub new_tasks: usize,
}

impl IncrementalStats {
    /// Calculate percentage of unchanged tasks
    pub fn unchanged_percent(&self) -> f64 {
        if self.total_tasks == 0 {
            0.0
        } else {
            (self.unchanged as f64 / self.total_tasks as f64) * 100.0
        }
    }

    /// Calculate percentage of tasks needing rebuild
    pub fn rebuild_percent(&self) -> f64 {
        if self.total_tasks == 0 {
            0.0
        } else {
            (self.need_rebuild as f64 / self.total_tasks as f64) * 100.0
        }
    }

    /// Calculate percentage of new tasks
    pub fn new_percent(&self) -> f64 {
        if self.total_tasks == 0 {
            0.0
        } else {
            (self.new_tasks as f64 / self.total_tasks as f64) * 100.0
        }
    }
}

/// High-level build orchestrator
pub struct BuildOrchestrator {
    config: OrchestratorConfig,
}

impl BuildOrchestrator {
    /// Create a new build orchestrator
    pub fn new(config: OrchestratorConfig) -> Self {
        Self { config }
    }

    /// Build a complete build plan from layer paths
    pub async fn build_plan(
        &self,
        layer_paths: HashMap<String, Vec<PathBuf>>,
    ) -> Result<BuildPlan, Box<dyn std::error::Error + Send + Sync>> {

        let build_start = Instant::now();

        // Step 1: Build layer context
        let stage_start = Instant::now();
        info!("Building layer context with priorities");
        let mut build_context = BuildContext::new();

        if let Some(machine) = &self.config.machine {
            build_context.set_machine(machine.clone());
        }

        if let Some(distro) = &self.config.distro {
            build_context.set_distro(distro.clone());
        }

        // Add layers
        for layer_conf in self.find_layer_confs(&layer_paths)? {
            build_context.add_layer_from_conf(&layer_conf)?;
        }
        info!("✓ Step 1 completed in {:?}", stage_start.elapsed());

        // Step 2: Parse recipes with parallel pipeline
        let stage_start = Instant::now();
        info!("Parsing BitBake recipes with parallel pipeline");
        let pipeline_config = PipelineConfig {
            max_io_parallelism: self.config.max_io_parallelism,
            max_cpu_parallelism: self.config.max_cpu_parallelism,
            enable_cache: true,
            cache_dir: self.config.build_dir.join("hitzeleiter-cache/pipeline"),
        };

        let pipeline = Pipeline::new(pipeline_config, build_context);

        let (recipe_files, _) = pipeline.discover_recipes(&layer_paths).await?;
        let (parsed_recipes, _) = pipeline.parse_recipes(recipe_files).await?;
        info!("✓ Step 2 completed in {:?} ({} recipes parsed)", stage_start.elapsed(), parsed_recipes.len());

        // Rebuild build_context for return value
        let mut build_context = BuildContext::new();
        if let Some(machine) = &self.config.machine {
            build_context.set_machine(machine.clone());
        }
        if let Some(distro) = &self.config.distro {
            build_context.set_distro(distro.clone());
        }
        for layer_conf in self.find_layer_confs(&layer_paths)? {
            build_context.add_layer_from_conf(&layer_conf)?;
        }

        // Load machine and distro configuration files
        info!("Loading machine and distro configurations");
        build_context.load_machine_config()?;
        build_context.load_distro_config()?;

        // Extract task implementations and helper functions
        let mut task_implementations = HashMap::new();
        let mut helper_implementations = HashMap::new();
        let mut recipe_variables = HashMap::new();

        for parsed in &parsed_recipes {
            if !parsed.task_impls.is_empty() {
                task_implementations.insert(parsed.file.name.clone(), parsed.task_impls.clone());
            }
            if !parsed.helper_funcs.is_empty() {
                helper_implementations.insert(parsed.file.name.clone(), parsed.helper_funcs.clone());
            }

            // Extract variables from recipe content for preprocessing
            // IMPORTANT: Enable resolve_includes to get variables from .inc files
            // (many recipes like libxcrypt only have "require foo.inc" in the .bb file)
            let extractor_for_vars = RecipeExtractor::new(ExtractionConfig {
                extract_tasks: false,
                use_simple_python_eval: false,
                resolve_includes: true,  // Critical: resolve require/include statements
                ..Default::default()
            });

            // Pass the full recipe path to extract both name and version for variable expansion
            let vars = extractor_for_vars.parse_variables_with_includes(&parsed.content, &parsed.file.path);

            recipe_variables.insert(parsed.file.name.clone(), vars);
        }

        // Step 3: Build recipe graph
        let stage_start = Instant::now();
        info!("Building recipe dependency graph");

        // Build class search paths from layer paths (HashMap<String, Vec<PathBuf>>)
        let class_search_paths: Vec<std::path::PathBuf> = layer_paths
            .values()
            .flatten()  // Flatten Vec<PathBuf> from each layer
            .map(|layer_path| layer_path.join("classes"))
            .collect();

        info!("Class search paths configured: {}", class_search_paths.len());
        for (i, path) in class_search_paths.iter().enumerate().take(5) {
            info!("  [{}] {:?}", i, path);
        }

        let extractor = RecipeExtractor::new(ExtractionConfig {
            extract_tasks: true,
            resolve_providers: true,
            resolve_includes: true,   // Enable .inc file processing for variables (CRITICAL for DEPENDS)
            resolve_inherit: true,    // Enable .bbclass processing for standard task ordering
            class_search_paths,       // Provide paths to find base.bbclass and other classes
            ..Default::default()
        });
        let (recipe_graph, _) = pipeline.build_recipe_graph(&parsed_recipes, &extractor)?;
        info!("✓ Step 3 completed in {:?}", stage_start.elapsed());

        // Step 4: Build task execution graph
        let stage_start = Instant::now();
        info!("Building task execution graph");
        let task_builder = TaskGraphBuilder::new(recipe_graph.clone());
        let task_graph = task_builder.build_full_graph()?;
        info!("✓ Step 4 completed in {:?}", stage_start.elapsed());

        // Step 5: Compute task signatures
        let stage_start = Instant::now();
        info!("Computing task signatures");
        let mut recipe_hashes = HashMap::new();
        for parsed in &parsed_recipes {
            recipe_hashes.insert(parsed.file.name.clone(), parsed.hash.clone());
        }

        let mut sig_cache = SignatureCache::new(
            self.config.build_dir.join("hitzeleiter-cache/signatures")
        );

        // Load previous signatures
        let cache_path = self.config.build_dir.join("hitzeleiter-cache/signatures/signatures.json");
        let previous_signatures = if cache_path.exists() {
            let json = tokio::fs::read_to_string(&cache_path).await?;
            serde_json::from_str(&json).unwrap_or_default()
        } else {
            HashMap::new()
        };

        // Compute new signatures
        sig_cache.compute_signatures(
            &task_graph,
            &recipe_hashes,
            &task_implementations,
            self.config.machine.as_deref(),
            self.config.distro.as_deref(),
        ).await?;
        info!("✓ Step 5 completed in {:?}", stage_start.elapsed());

        // Step 6: Analyze incremental build requirements
        let stage_start = Instant::now();
        info!("Analyzing incremental build requirements");
        let incremental_stats = self.analyze_incremental_build(
            &task_graph,
            &sig_cache,
            &previous_signatures,
        );

        // Save updated signatures
        sig_cache.save().await?;
        info!("✓ Step 6 completed in {:?}", stage_start.elapsed());

        // Step 7: Create task specifications
        let stage_start = Instant::now();
        info!("Creating task specifications");
        let task_specs = self.create_task_specs(
            &task_graph,
            &task_implementations,
            &helper_implementations,
            &recipe_variables,
            &build_context,
            &self.config.build_dir,
        )?;
        info!("✓ Step 7 completed in {:?} ({} task specs created)", stage_start.elapsed(), task_specs.len());

        info!("✓ Build plan completed in {:?}", build_start.elapsed());

        Ok(BuildPlan {
            build_context,
            recipe_graph,
            task_graph,
            task_specs,
            task_implementations,
            helper_implementations,
            signature_cache: sig_cache,
            incremental_stats,
        })
    }

    /// Build only the recipe graph without task specs
    ///
    /// This is a lightweight method for queries that only need recipe dependencies,
    /// avoiding the expensive task specification generation that can crash with RustPython.
    pub async fn build_recipe_graph_only(
        &self,
        layer_paths: HashMap<String, Vec<PathBuf>>,
    ) -> Result<(RecipeGraph, BuildContext), Box<dyn std::error::Error + Send + Sync>> {
        let build_start = Instant::now();

        // Step 1: Build layer context
        let stage_start = Instant::now();
        info!("Building layer context with priorities");
        let mut build_context = BuildContext::new();

        if let Some(machine) = &self.config.machine {
            build_context.set_machine(machine.clone());
        }

        if let Some(distro) = &self.config.distro {
            build_context.set_distro(distro.clone());
        }

        for layer_conf in self.find_layer_confs(&layer_paths)? {
            build_context.add_layer_from_conf(&layer_conf)?;
        }
        info!("✓ Step 1 completed in {:?}", stage_start.elapsed());

        // Step 2: Parse recipes with parallel pipeline
        let stage_start = Instant::now();
        info!("Parsing BitBake recipes with parallel pipeline");
        let pipeline_config = PipelineConfig {
            max_io_parallelism: self.config.max_io_parallelism,
            max_cpu_parallelism: self.config.max_cpu_parallelism,
            enable_cache: true,
            cache_dir: self.config.build_dir.join("hitzeleiter-cache/pipeline"),
        };

        let pipeline = Pipeline::new(pipeline_config, build_context);

        let (recipe_files, _) = pipeline.discover_recipes(&layer_paths).await?;
        let (parsed_recipes, _) = pipeline.parse_recipes(recipe_files).await?;
        info!("✓ Step 2 completed in {:?} ({} recipes parsed)", stage_start.elapsed(), parsed_recipes.len());

        // Rebuild build_context for return value (Pipeline consumed the previous one)
        let mut build_context = BuildContext::new();
        if let Some(machine) = &self.config.machine {
            build_context.set_machine(machine.clone());
        }
        if let Some(distro) = &self.config.distro {
            build_context.set_distro(distro.clone());
        }
        for layer_conf in self.find_layer_confs(&layer_paths)? {
            build_context.add_layer_from_conf(&layer_conf)?;
        }

        // Step 3: Build recipe graph (without task specs)
        let stage_start = Instant::now();
        info!("Building recipe dependency graph");

        let class_search_paths: Vec<std::path::PathBuf> = layer_paths
            .values()
            .flatten()
            .map(|layer_path| layer_path.join("classes"))
            .collect();

        let extractor = RecipeExtractor::new(ExtractionConfig {
            extract_tasks: false, // Don't extract tasks - we only need dependencies
            resolve_providers: true,
            resolve_includes: true,
            resolve_inherit: false, // Don't need inheritance for simple queries
            class_search_paths,
            ..Default::default()
        });
        let (recipe_graph, _) = pipeline.build_recipe_graph(&parsed_recipes, &extractor)?;
        info!("✓ Step 3 completed in {:?}", stage_start.elapsed());

        info!("✓ Recipe graph built in {:?} ({} recipes)", build_start.elapsed(), recipe_graph.recipe_count());

        Ok((recipe_graph, build_context))
    }

    /// Find all layer.conf files in layer paths
    fn find_layer_confs(
        &self,
        layer_paths: &HashMap<String, Vec<PathBuf>>,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error + Send + Sync>> {
        let mut confs = Vec::new();

        for paths in layer_paths.values() {
            for layer_dir in paths {
                let layer_conf = layer_dir.join("conf/layer.conf");
                if layer_conf.exists() {
                    confs.push(layer_conf);
                }
            }
        }

        Ok(confs)
    }

    /// Analyze incremental build requirements
    fn analyze_incremental_build(
        &self,
        task_graph: &TaskGraph,
        sig_cache: &SignatureCache,
        previous_signatures: &HashMap<String, crate::EnhancedTaskSignature>,
    ) -> IncrementalStats {
        let mut unchanged = 0;
        let mut need_rebuild = 0;
        let mut new_tasks = 0;

        for task in task_graph.tasks.values() {
            let task_key = format!("{}:{}", task.recipe_name, task.task_name);

            if let Some(current_sig) = sig_cache.get_signature(&task.recipe_name, &task.task_name) {
                if let Some(prev_sig) = previous_signatures.get(&task_key) {
                    if let Some(prev_sig_str) = &prev_sig.signature {
                        if current_sig == prev_sig_str {
                            unchanged += 1;
                        } else {
                            need_rebuild += 1;
                        }
                    } else {
                        need_rebuild += 1;
                    }
                } else {
                    new_tasks += 1;
                }
            }
        }

        IncrementalStats {
            total_tasks: task_graph.tasks.len(),
            unchanged,
            need_rebuild,
            new_tasks,
        }
    }

    /// Create task specifications for execution
    fn create_task_specs(
        &self,
        task_graph: &TaskGraph,
        task_implementations: &HashMap<String, HashMap<String, TaskImplementation>>,
        helper_implementations: &HashMap<String, HashMap<String, TaskImplementation>>,
        recipe_variables: &HashMap<String, HashMap<String, String>>,
        build_context: &BuildContext,
        build_dir: &Path,
    ) -> Result<HashMap<String, TaskSpec>, Box<dyn std::error::Error + Send + Sync>> {
        let tmp_dir = build_dir.join("tmp");
        fs::create_dir_all(&tmp_dir)?;

        let total_tasks = task_graph.tasks.len();
        info!("  Processing {} tasks in parallel (CPU-bound)...", total_tasks);

        // Create a custom rayon thread pool with larger stacks (16MB instead of default 2MB)
        // This prevents stack overflow from deep recursion in SimplePythonEvaluator
        // and file:// URI resolution
        let num_threads = std::env::var("HITZELEITER_THREADS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| num_cpus::get());
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .stack_size(16 * 1024 * 1024)  // 16MB stack per thread
            .thread_name(|idx| format!("task-spec-{}", idx))
            .build()
            .map_err(|e| format!("Failed to create thread pool: {}", e))?;

        info!("  Using {} threads with 16MB stacks for task spec generation", num_threads);

        // Thread-safe progress tracking
        let processed = AtomicUsize::new(0);
        let preprocess_total_time = AtomicUsize::new(0);
        let last_report = Mutex::new(Instant::now());

        // Parallel processing with custom thread pool
        let specs: Result<HashMap<String, TaskSpec>, Box<dyn std::error::Error + Send + Sync>> = pool.install(|| {
            task_graph.tasks.values()
            .collect::<Vec<_>>()
            .par_iter()
            .map(|task| {
                let current = processed.fetch_add(1, Ordering::Relaxed) + 1;

                // Report progress every 3000 tasks or every 2 seconds (thread-safe)
                if current % 3000 == 0 {
                    let pct = (current as f64 / total_tasks as f64) * 100.0;
                    info!("  Progress: {}/{} tasks ({:.1}%)", current, total_tasks, pct);
                } else if let Ok(mut last) = last_report.try_lock() {
                    if last.elapsed().as_secs() >= 2 {
                        let pct = (current as f64 / total_tasks as f64) * 100.0;
                        info!("  Progress: {}/{} tasks ({:.1}%)", current, total_tasks, pct);
                        *last = Instant::now();
                    }
                }

            let task_key = format!("{}:{}", task.recipe_name, task.task_name);

            // Get helper functions for this recipe (both explicit helpers and other task functions)
            let mut all_helpers = helper_implementations
                .get(&task.recipe_name)
                .cloned()
                .unwrap_or_default();

            // Also include all other task functions from this recipe as potential helpers
            // (e.g., do_prepare_config can be called by do_configure)
            // Only include shell functions for shell tasks (don't mix Python and shell)
            if let Some(recipe_tasks) = task_implementations.get(&task.recipe_name) {
                for (task_fn_name, task_fn_impl) in recipe_tasks {
                    // Don't include the current task itself
                    // Don't include Python functions in shell scripts
                    if task_fn_name != &task.task_name
                        && task_fn_impl.impl_type == crate::task_extractor::TaskImplementationType::Shell {
                        all_helpers.insert(format!("do_{task_fn_name}"), task_fn_impl.clone());
                    }
                }
            }

            // Try to find real task implementation
            let raw_script = if let Some(recipe_impls) = task_implementations.get(&task.recipe_name) {
                if let Some(task_impl) = recipe_impls.get(&task.task_name) {
                    self.create_task_script(&task.recipe_name, &task.task_name, &task_impl.code, &all_helpers)
                } else {
                    self.create_placeholder_script(&task.recipe_name, &task.task_name)
                }
            } else {
                self.create_placeholder_script(&task.recipe_name, &task.task_name)
            };

            // NEW: Preprocess script to handle BitBake syntax (${@python_expr}, ${VAR[flag]}, etc.)
            // Also prepare environment variables for task execution
            let result_tuple = {
                let preprocess_start = Instant::now();

                // Get recipe variables from parsed recipes, or use defaults
                let mut recipe_vars = recipe_variables
                    .get(&task.recipe_name)
                    .cloned()
                    .unwrap_or_else(HashMap::new);

                // Merge global variables from build context (machine.conf, distro.conf, etc.)
                // These should be added BEFORE recipe-specific variables and hardcoded defaults
                for (key, value) in &build_context.global_variables {
                    recipe_vars.entry(key.clone()).or_insert_with(|| value.clone());
                }

                // Add runtime variables that may not be in recipe
                recipe_vars.entry("PN".to_string()).or_insert_with(|| task.recipe_name.clone());
                let workdir = build_dir.join("tmp").join(&task.recipe_name);
                recipe_vars.entry("WORKDIR".to_string()).or_insert_with(|| workdir.to_string_lossy().to_string());

                // Add common missing variables with sensible defaults (only if not already set)
                recipe_vars.entry("LLVMVERSION".to_string()).or_insert_with(|| "15".to_string());
                recipe_vars.entry("TARGET_ARCH".to_string()).or_insert_with(|| "aarch64".to_string());
                recipe_vars.entry("HOST_ARCH".to_string()).or_insert_with(|| "aarch64".to_string());
                recipe_vars.entry("BUILD_ARCH".to_string()).or_insert_with(|| "x86_64".to_string());
                recipe_vars.entry("TARGET_OS".to_string()).or_insert_with(|| "linux".to_string());
                recipe_vars.entry("HOST_OS".to_string()).or_insert_with(|| "linux".to_string());
                recipe_vars.entry("BUILD_OS".to_string()).or_insert_with(|| "linux".to_string());
                recipe_vars.entry("TARGET_SYS".to_string()).or_insert_with(|| "aarch64-poky-linux".to_string());
                recipe_vars.entry("HOST_SYS".to_string()).or_insert_with(|| "aarch64-poky-linux".to_string());
                recipe_vars.entry("BUILD_SYS".to_string()).or_insert_with(|| "x86_64-linux".to_string());
                recipe_vars.entry("AUTOTOOLS_AUXDIR".to_string()).or_insert_with(|| "${S}/build-aux".to_string());
                // Note: SRCREV should come from recipe parsing, not set to a dummy value
                // If missing, git fetcher will use HEAD/default branch
                recipe_vars.entry("baselib".to_string()).or_insert_with(|| "lib".to_string());
                recipe_vars.entry("prefix".to_string()).or_insert_with(|| "/usr".to_string());
                recipe_vars.entry("bindir".to_string()).or_insert_with(|| "/usr/bin".to_string());
                recipe_vars.entry("sbindir".to_string()).or_insert_with(|| "/usr/sbin".to_string());
                recipe_vars.entry("libdir".to_string()).or_insert_with(|| "/usr/lib".to_string());
                recipe_vars.entry("libexecdir".to_string()).or_insert_with(|| "/usr/libexec".to_string());
                recipe_vars.entry("includedir".to_string()).or_insert_with(|| "/usr/include".to_string());
                recipe_vars.entry("datadir".to_string()).or_insert_with(|| "/usr/share".to_string());
                recipe_vars.entry("sharedstatedir".to_string()).or_insert_with(|| "/var/lib".to_string());
                recipe_vars.entry("INIT_SYSTEM".to_string()).or_insert_with(|| "sysvinit".to_string());
                recipe_vars.entry("DISTRO_FEATURES".to_string()).or_insert_with(|| "sysvinit".to_string());
                recipe_vars.entry("KERNEL_VERSION".to_string()).or_insert_with(|| "5.15".to_string());
                recipe_vars.entry("KERNEL_IMAGETYPE".to_string()).or_insert_with(|| "Image".to_string());

                // Clone recipe_vars for task environment, then expand Python expressions in values
                let mut env_vars = recipe_vars.clone();

                // Expand Python expressions in environment variables (especially SRC_URI)
                // This uses the same SimplePythonEvaluator as script preprocessing
                let evaluator = crate::simple_python_eval::SimplePythonEvaluator::new(env_vars.clone());
                for (key, value) in env_vars.iter_mut() {
                    if value.contains("${@") {
                        // Expand Python expressions in this value
                        let expanded = evaluator.expand_all_expressions(value);
                        if expanded != *value {
                            trace!("Expanded env var {}: {} -> {}", key, value, expanded);
                            *value = expanded;
                        }
                    }
                }

                let preprocess_start = Instant::now();
                let preprocessor = ScriptPreprocessor::new(recipe_vars.clone());

                let result = match preprocessor.preprocess(&raw_script) {
                    Ok(processed) => {
                        // Successfully preprocessed
                        processed
                    }
                    Err(e) => {
                        // Preprocessing failed - log warning and use original
                        warn!(
                            "Script preprocessing failed for {}:{}: {}",
                            task.recipe_name, task.task_name, e
                        );
                        warn!("Falling back to unprocessed script");
                        raw_script
                    }
                };

                let elapsed = preprocess_start.elapsed();
                preprocess_total_time.fetch_add(elapsed.as_micros() as usize, Ordering::Relaxed);

                // Resolve file:// URIs for fetch/unpack tasks BEFORE returning from block
                // This makes file:// resources available without runtime filesystem access
                let file_resources = if task.task_name == "do_fetch" || task.task_name == "do_unpack" {
                    self.resolve_file_uris(&task.recipe_name, &recipe_vars, build_context)
                } else {
                    HashMap::new()
                };

                (result, env_vars, file_resources)
            };

            let (script, task_env, file_resources) = result_tuple;

            // Work directory already created before parallel loop
            let task_workdir = tmp_dir.join(&task.recipe_name).join(&task.task_name);
            // Use std::fs::create_dir_all which handles concurrent creation safely
            if let Err(e) = fs::create_dir_all(&task_workdir) {
                // Only fail if directory doesn't exist after the call
                if !task_workdir.exists() {
                    return Err(format!("Failed to create workdir {}: {}", task_workdir.display(), e).into());
                }
            }

            // Output file - executor will prepend /work/outputs/ for relative paths
            let output_file = format!("{}.done", task.task_name);

            // Determine network policy based on task type
            let network_policy = if task.task_name == "do_fetch" || task.task_name.contains("fetch") {
                NetworkPolicy::FullNetwork  // Fetch tasks need real internet access
            } else {
                NetworkPolicy::Isolated  // Build tasks should be hermetic
            };

            // Auto-detect execution mode from script (using preprocessed script)
            let execution_mode = crate::executor::determine_execution_mode(&script);

            // Resolve file:// URIs for fetch/unpack tasks
            // This makes file:// resources available without runtime filesystem access
            let file_resources = if task.task_name == "do_fetch" || task.task_name == "do_unpack" {
                self.resolve_file_uris(&task.recipe_name, &task_env, build_context)
            } else {
                HashMap::new()
            };

            let spec = TaskSpec {
                name: task.task_name.clone(),
                recipe: task.recipe_name.clone(),
                script,
                workdir: task_workdir,
                env: task_env,  // Use recipe variables as task environment
                outputs: vec![PathBuf::from(&output_file)],
                file_resources,  // Resolved file:// paths from SRC_URI
                timeout: Some(Duration::from_secs(300)),
                execution_mode,
                network_policy,
                resource_limits: ResourceLimits::default(),
            };

            Ok((task_key, spec))
        })
        .collect::<Result<HashMap<_, _>, _>>()
        });  // Close parallel block

        let specs = specs?;

        let total_preprocess_micros = preprocess_total_time.load(Ordering::Relaxed) as u64;
        if total_preprocess_micros > 0 && total_tasks > 0 {
            let total_time = Duration::from_micros(total_preprocess_micros);
            let avg_micros = total_preprocess_micros / total_tasks as u64;
            info!(
                "  ✓ Preprocessing: {} tasks in {:?} (avg {:?}/task)",
                total_tasks, total_time, Duration::from_micros(avg_micros)
            );
        }

        Ok(specs)
    }

    /// Create a task script from implementation code with helper functions
    fn create_task_script(
        &self,
        recipe_name: &str,
        task_name: &str,
        code: &str,
        helpers: &HashMap<String, TaskImplementation>,
    ) -> String {
        let mut script = String::new();

        // Source shared prelude for common environment and functions
        // HITZELEITER_ROOT is set by the executor to the prelude location
        script.push_str("#!/bin/bash\n");
        script.push_str(". \"${HITZELEITER_ROOT}/prelude.sh\"\n\n");

        // Set recipe-specific variables
        script.push_str(&format!("export PN=\"{recipe_name}\"\n"));

        // Set up work directories - use variables that will be set by executor
        script.push_str("# Set up work directories (paths will be set by executor)\n");
        script.push_str("export WORKDIR=\"${WORKDIR:-/work}\"\n");
        script.push_str("export S=\"${S:-${WORKDIR}/src}\"\n");
        script.push_str("export B=\"${B:-${WORKDIR}/build}\"\n");
        script.push_str("export D=\"${D:-${WORKDIR}/image}\"\n");
        script.push_str("bbdirs \"${WORKDIR}\" \"${S}\" \"${B}\" \"${D}\"\n");
        script.push_str("cd \"${WORKDIR}\"\n\n");

        // Create minimal stub files for known recipes
        if recipe_name == "busybox" && (task_name == "configure" || task_name == "compile") {
            script.push_str("# Create stub defconfig for busybox (minimal working config)\n");
            script.push_str("cat > ${WORKDIR}/defconfig <<'DEFCONFIG_EOF'\n");
            script.push_str("# Minimal busybox configuration\n");
            script.push_str("CONFIG_DESKTOP=y\n");
            script.push_str("CONFIG_EXTRA_COMPAT=y\n");
            script.push_str("CONFIG_FEATURE_DEVPTS=y\n");
            script.push_str("CONFIG_LFS=y\n");
            script.push_str("DEFCONFIG_EOF\n\n");

            // Also need a minimal Makefile in ${S}
            script.push_str("# Create stub Makefile for busybox (for make oldconfig)\n");
            script.push_str("cat > ${S}/Makefile <<'MAKEFILE_EOF'\n");
            script.push_str(".PHONY: oldconfig\n");
            script.push_str("oldconfig:\n");
            script.push_str("\t@echo \"[STUB] make oldconfig completed\"\n");
            script.push_str("\t@touch .config\n");
            script.push_str("MAKEFILE_EOF\n\n");
        }

        // Add helper functions before the task implementation
        if !helpers.is_empty() {
            script.push_str("# Helper functions from recipe\n");
            for (helper_name, helper_impl) in helpers {
                script.push_str(&format!("{helper_name}() {{\n"));
                script.push_str(&helper_impl.code);
                script.push_str("\n}\n\n");
            }
        }

        // Task code
        script.push_str("# Task implementation\n");
        script.push_str(code);
        script.push_str("\n\n");

        // Explicitly create completion marker (don't rely solely on trap)
        // Output will be collected from work/outputs/<task>.done by the executor
        let output_filename = format!("{task_name}.done");
        script.push_str("# Mark task as complete\n");
        script.push_str("mkdir -p outputs\n");
        script.push_str(&format!("touch \"outputs/{output_filename}\"\n"));

        script
    }

    /// Create a placeholder script for tasks without implementation
    fn create_placeholder_script(&self, recipe_name: &str, task_name: &str) -> String {
        let output_filename = format!("{task_name}.done");
        format!(
            "#!/bin/bash\n\
. \"${{HITZELEITER_ROOT}}/prelude.sh\"\n\
\n\
export PN=\"{recipe_name}\"\n\
export WORKDIR=\"${{WORKDIR:-/work}}\"\n\
# Note: The executor already changes to WORKDIR before executing the script,\n\
# so we don't need to cd here. This avoids path duplication issues.\n\
\n\
bb_note '[PLACEHOLDER] {task_name}'\n\
mkdir -p outputs\n\
touch \"outputs/{output_filename}\"\n"
        )
    }

    /// Resolve file:// URIs from SRC_URI using FILESPATH search
    ///
    /// This searches recipe directories for local files referenced in SRC_URI.
    /// Search order follows BitBake's FILESPATH:
    /// 1. ${FILE_DIRNAME}/${BP}/ (recipe_dir/busybox-1.35.0/)
    /// 2. ${FILE_DIRNAME}/${BPN}/ (recipe_dir/busybox/)
    /// 3. ${FILE_DIRNAME}/files/ (recipe_dir/files/)
    /// 4. ${FILE_DIRNAME}/${BPN}-${MACHINE}/ (recipe_dir/busybox-qemuarm64/)
    fn resolve_file_uris(
        &self,
        recipe_name: &str,
        recipe_vars: &HashMap<String, String>,
        build_context: &BuildContext,
    ) -> HashMap<String, PathBuf> {
        let mut file_resources = HashMap::new();

        // Get SRC_URI - if not present, nothing to do
        let src_uri = match recipe_vars.get("SRC_URI") {
            Some(uri) => uri,
            None => return file_resources,
        };

        // Get variables needed for FILESPATH
        let bpn = recipe_vars.get("BPN")
            .or_else(|| recipe_vars.get("PN"))
            .cloned()
            .unwrap_or_else(|| recipe_name.to_string());
        let pv = recipe_vars.get("PV").cloned().unwrap_or_else(|| "unknown".to_string());
        let bp = format!("{}-{}", bpn, pv);
        let machine = build_context.machine.as_deref().unwrap_or("unknown");

        // Find recipe file in layers to get FILE_DIRNAME
        let recipe_dir = self.find_recipe_directory(&bpn, build_context);
        if recipe_dir.is_none() {
            debug!("Could not find recipe directory for {}", recipe_name);
            return file_resources;
        }
        let recipe_dir = recipe_dir.unwrap();

        // Parse file:// URIs from SRC_URI
        for part in src_uri.split_whitespace() {
            if let Some(file_uri) = part.strip_prefix("file://") {
                // Remove any parameters (;param=value)
                let filename = file_uri.split(';').next().unwrap_or(file_uri);
                if filename.is_empty() {
                    continue;
                }

                // Search FILESPATH for the file
                let search_paths = vec![
                    recipe_dir.join(&bp),           // ${FILE_DIRNAME}/${BP}/
                    recipe_dir.join(&bpn),          // ${FILE_DIRNAME}/${BPN}/
                    recipe_dir.join("files"),       // ${FILE_DIRNAME}/files/
                    recipe_dir.join(format!("{}-{}", bpn, machine)), // ${FILE_DIRNAME}/${BPN}-${MACHINE}/
                ];

                for search_dir in &search_paths {
                    let candidate = search_dir.join(filename);
                    if candidate.exists() && candidate.is_file() {
                        debug!("Resolved file://{} → {}", filename, candidate.display());
                        file_resources.insert(filename.to_string(), candidate);
                        break;
                    }
                }

                if !file_resources.contains_key(filename) {
                    debug!("Could not resolve file://{} in FILESPATH for {}", filename, recipe_name);
                }
            }
        }

        if !file_resources.is_empty() {
            info!("Resolved {} file:// URIs for {}", file_resources.len(), recipe_name);
        }

        file_resources
    }

    /// Find recipe directory by searching all layers
    fn find_recipe_directory(&self, recipe_name: &str, build_context: &BuildContext) -> Option<PathBuf> {
        // Search layers in priority order
        for layer in &build_context.layers {
            // Try recipes-*/<category>/<recipe_name>/
            let layer_path = &layer.layer_dir;

            // Use read_dir instead of glob for thread safety
            if let Ok(entries) = std::fs::read_dir(layer_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    // Check if directory name starts with "recipes-"
                    if path.is_dir() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.starts_with("recipes-") {
                                // Check if recipe_name directory exists in this category
                                let candidate = path.join(recipe_name);
                                if candidate.exists() && candidate.is_dir() {
                                    return Some(candidate);
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }
}
