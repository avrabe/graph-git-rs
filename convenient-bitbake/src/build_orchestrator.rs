//! High-level build orchestration
//!
//! Provides a clean API for orchestrating the full build pipeline from
//! layer discovery through task graph generation.

use crate::{
    BuildContext, ExtractionConfig, LayerConfig, Pipeline, PipelineConfig,
    RecipeExtractor, RecipeGraph, SignatureCache, TaskExtractor, TaskGraph,
    TaskGraphBuilder, TaskImplementation, TaskSpec,
};
use crate::executor::types::{NetworkPolicy, ResourceLimits};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::info;

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

        // Step 1: Build layer context
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

        // Step 2: Parse recipes with parallel pipeline
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

        // Extract task implementations
        let mut task_implementations = HashMap::new();
        for parsed in &parsed_recipes {
            if !parsed.task_impls.is_empty() {
                task_implementations.insert(parsed.file.name.clone(), parsed.task_impls.clone());
            }
        }

        // Step 3: Build recipe graph
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

        // Step 4: Build task execution graph
        info!("Building task execution graph");
        let task_builder = TaskGraphBuilder::new(recipe_graph.clone());
        let task_graph = task_builder.build_full_graph()?;

        // Step 5: Compute task signatures
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

        // Step 6: Analyze incremental build requirements
        info!("Analyzing incremental build requirements");
        let incremental_stats = self.analyze_incremental_build(
            &task_graph,
            &sig_cache,
            &previous_signatures,
        );

        // Save updated signatures
        sig_cache.save().await?;

        // Step 7: Create task specifications
        info!("Creating task specifications");
        let task_specs = self.create_task_specs(
            &task_graph,
            &task_implementations,
            &self.config.build_dir,
        )?;

        Ok(BuildPlan {
            build_context,
            recipe_graph,
            task_graph,
            task_specs,
            task_implementations,
            signature_cache: sig_cache,
            incremental_stats,
        })
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

        for (_task_id, task) in &task_graph.tasks {
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
        build_dir: &Path,
    ) -> Result<HashMap<String, TaskSpec>, Box<dyn std::error::Error + Send + Sync>> {
        let mut specs = HashMap::new();
        let tmp_dir = build_dir.join("tmp");
        fs::create_dir_all(&tmp_dir)?;

        for (_task_id, task) in &task_graph.tasks {
            let task_key = format!("{}:{}", task.recipe_name, task.task_name);

            // Try to find real task implementation
            let script = if let Some(recipe_impls) = task_implementations.get(&task.recipe_name) {
                if let Some(task_impl) = recipe_impls.get(&task.task_name) {
                    self.create_task_script(&task.recipe_name, &task.task_name, &task_impl.code)
                } else {
                    self.create_placeholder_script(&task.recipe_name, &task.task_name)
                }
            } else {
                self.create_placeholder_script(&task.recipe_name, &task.task_name)
            };

            let task_workdir = tmp_dir.join(&task.recipe_name).join(&task.task_name);
            fs::create_dir_all(&task_workdir)?;

            let output_file = format!("{}.done", task.task_name);

            // Determine network policy based on task type
            let network_policy = if task.task_name == "do_fetch" || task.task_name.contains("fetch") {
                NetworkPolicy::FullNetwork  // Fetch tasks need real internet access
            } else {
                NetworkPolicy::Isolated  // Build tasks should be hermetic
            };

            // Auto-detect execution mode from script
            let execution_mode = crate::executor::determine_execution_mode(&script);

            let spec = TaskSpec {
                name: task.task_name.clone(),
                recipe: task.recipe_name.clone(),
                script,
                workdir: task_workdir,
                env: HashMap::new(),
                outputs: vec![PathBuf::from(&output_file)],
                timeout: Some(Duration::from_secs(300)),
                execution_mode,
                network_policy,
                resource_limits: ResourceLimits::default(),
            };

            specs.insert(task_key, spec);
        }

        Ok(specs)
    }

    /// Create a task script from implementation code
    fn create_task_script(&self, recipe_name: &str, task_name: &str, code: &str) -> String {
        let mut script = String::new();

        // Source shared prelude for common environment and functions
        script.push_str("#!/bin/bash\n");
        script.push_str(". /hitzeleiter/prelude.sh\n\n");

        // Set recipe-specific variables
        script.push_str(&format!("export PN=\"{}\"\n\n", recipe_name));

        // Task code
        script.push_str(code);
        script.push_str("\n\n");

        // Mark task complete
        let output_file = format!("{}.done", task_name);
        script.push_str(&format!(
            "# Mark task complete\ntouch \"$D/{}\"\n",
            output_file
        ));

        script
    }

    /// Create a placeholder script for tasks without implementation
    fn create_placeholder_script(&self, recipe_name: &str, task_name: &str) -> String {
        let output_file = format!("{}.done", task_name);
        format!(
            "#!/bin/bash\n. /hitzeleiter/prelude.sh\nexport PN=\"{}\"\nbb_note '[PLACEHOLDER] {}'\ntouch \"$D/{}\"",
            recipe_name, task_name, output_file
        )
    }
}
