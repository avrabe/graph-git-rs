//! Parallel build pipeline with incremental build support
//!
//! Implements a multi-stage pipeline for processing BitBake recipes:
//! 1. File discovery (parallel)
//! 2. Recipe parsing (parallel)
//! 3. Task extraction (parallel)
//! 4. Dependency resolution (sequential - needs full graph)
//! 5. Task graph building (sequential - needs dependencies)
//!
//! Each stage computes content hashes to enable incremental builds.

use crate::{
    BuildContext, RecipeExtractor, RecipeGraph,
    TaskExtractor, TaskImplementation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, info};

/// Content hash for a pipeline stage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageHash {
    /// Stage name
    pub stage: String,
    /// SHA-256 hash of stage inputs
    pub hash: String,
    /// Timestamp when computed
    pub timestamp: u64,
}

impl StageHash {
    pub fn new(stage: impl Into<String>, data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());

        Self {
            stage: stage.into(),
            hash,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    pub fn from_string(stage: impl Into<String>, data: &str) -> Self {
        Self::new(stage, data.as_bytes())
    }
}

/// A recipe file with metadata
#[derive(Debug, Clone)]
pub struct RecipeFile {
    /// Full path to recipe file
    pub path: PathBuf,
    /// Recipe name (extracted from filename)
    pub name: String,
    /// Layer this recipe belongs to
    pub layer: String,
    /// File modification time
    pub mtime: u64,
    /// File size
    pub size: u64,
}

/// Result of parsing a recipe
#[derive(Debug, Clone)]
pub struct ParsedRecipe {
    /// Recipe file info
    pub file: RecipeFile,
    /// Recipe content
    pub content: String,
    /// Extracted task implementations
    pub task_impls: HashMap<String, TaskImplementation>,
    /// Extracted helper functions
    pub helper_funcs: HashMap<String, TaskImplementation>,
    /// Content hash
    pub hash: String,
}

/// Pipeline configuration
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Maximum parallel tasks for file I/O
    pub max_io_parallelism: usize,
    /// Maximum parallel tasks for CPU-bound work
    pub max_cpu_parallelism: usize,
    /// Enable incremental build cache
    pub enable_cache: bool,
    /// Cache directory
    pub cache_dir: PathBuf,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_io_parallelism: 32,
            max_cpu_parallelism: num_cpus::get(),
            enable_cache: true,
            cache_dir: PathBuf::from(".hitzeleiter-cache/pipeline"),
        }
    }
}

/// Parallel build pipeline
pub struct Pipeline {
    config: PipelineConfig,
    build_context: BuildContext,
}

impl Pipeline {
    pub fn new(config: PipelineConfig, build_context: BuildContext) -> Self {
        Self {
            config,
            build_context,
        }
    }

    /// Stage 1: Discover all recipe files in parallel
    pub async fn discover_recipes(
        &self,
        layer_paths: &HashMap<String, Vec<PathBuf>>,
    ) -> Result<(Vec<RecipeFile>, StageHash), Box<dyn std::error::Error + Send + Sync>> {
        info!("Stage 1: Discovering recipe files in parallel");

        let mut tasks: Vec<JoinHandle<Result<Vec<RecipeFile>, Box<dyn std::error::Error + Send + Sync>>>> = Vec::new();

        // Spawn parallel tasks for each layer
        for (repo_name, layers) in layer_paths {
            for layer_path in layers {
                let layer_path = layer_path.clone();
                let repo_name = repo_name.clone();

                let task = tokio::spawn(async move {
                    Self::discover_recipes_in_layer(&layer_path, &repo_name).await
                });

                tasks.push(task);
            }
        }

        // Collect all results
        let mut all_recipes = Vec::new();
        for task in tasks {
            match task.await {
                Ok(Ok(recipes)) => all_recipes.extend(recipes),
                Ok(Err(e)) => return Err(e),
                Err(e) => return Err(format!("Task join error: {e}").into()),
            }
        }

        // Sort for deterministic hashing
        all_recipes.sort_by(|a, b| a.path.cmp(&b.path));

        // Compute stage hash
        let mut hash_input = String::new();
        for recipe in &all_recipes {
            hash_input.push_str(&format!("{}:{}:{}\n", recipe.path.display(), recipe.mtime, recipe.size));
        }
        let stage_hash = StageHash::from_string("discover", &hash_input);

        info!("  Discovered {} recipes", all_recipes.len());
        debug!("  Stage hash: {}", stage_hash.hash);

        Ok((all_recipes, stage_hash))
    }

    /// Discover recipes in a single layer
    async fn discover_recipes_in_layer(
        layer_path: &Path,
        layer_name: &str,
    ) -> Result<Vec<RecipeFile>, Box<dyn std::error::Error + Send + Sync>> {
        let mut recipes = Vec::new();

        let entries = walkdir::WalkDir::new(layer_path)
            .max_depth(10)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("bb"));

        for entry in entries {
            let path = entry.path().to_path_buf();

            // Extract recipe name from filename
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.split('_').next())
                .unwrap_or("unknown")
                .to_string();

            // Get file metadata
            if let Ok(metadata) = tokio::fs::metadata(&path).await {
                let mtime = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());

                recipes.push(RecipeFile {
                    path,
                    name,
                    layer: layer_name.to_string(),
                    mtime,
                    size: metadata.len(),
                });
            }
        }

        Ok(recipes)
    }

    /// Stage 2: Parse recipes in parallel
    pub async fn parse_recipes(
        &self,
        recipes: Vec<RecipeFile>,
    ) -> Result<(Vec<ParsedRecipe>, StageHash), Box<dyn std::error::Error + Send + Sync>> {
        info!("Stage 2: Parsing {} recipes in parallel (max {} concurrent)",
              recipes.len(), self.config.max_io_parallelism);

        let task_extractor = Arc::new(TaskExtractor::new());

        // Use futures stream with buffer_unordered for true parallel processing
        // This allows tasks to complete independently without batch blocking
        use futures::stream::{self, StreamExt};

        let max_concurrent = self.config.max_io_parallelism;

        let total_recipes = recipes.len();
        let progress_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let last_report = Arc::new(std::sync::Mutex::new(std::time::Instant::now()));

        let all_parsed: Vec<ParsedRecipe> = stream::iter(recipes)
            .map(|recipe_file| {
                let task_extractor = Arc::clone(&task_extractor);
                async move {
                    Self::parse_single_recipe(recipe_file, task_extractor).await
                }
            })
            .buffer_unordered(max_concurrent)  // Process up to N recipes concurrently
            .filter_map(|result| async move {
                match result {
                    Ok(Some(parsed)) => Some(parsed),
                    Ok(None) => None,
                    Err(e) => {
                        debug!("Failed to parse recipe: {}", e);
                        None
                    }
                }
            })
            .inspect({
                let progress_counter = Arc::clone(&progress_counter);
                let last_report = Arc::clone(&last_report);
                move |_| {
                    let count = progress_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    // Report progress every 100 recipes or every 2 seconds
                    let mut last = last_report.lock().unwrap();
                    if count % 100 == 0 || last.elapsed().as_secs() >= 2 {
                        let pct = (count as f64 / total_recipes as f64) * 100.0;
                        info!("  Progress: {}/{} recipes parsed ({:.1}%)", count, total_recipes, pct);
                        *last = std::time::Instant::now();
                    }
                }
            })
            .collect()
            .await;

        // Sort for deterministic hashing
        let mut sorted_parsed = all_parsed;
        sorted_parsed.sort_by(|a, b| a.file.path.cmp(&b.file.path));

        // Compute stage hash
        let mut hash_input = String::new();
        for parsed in &sorted_parsed {
            hash_input.push_str(&format!("{}:{}\n", parsed.file.path.display(), parsed.hash));
        }
        let stage_hash = StageHash::from_string("parse", &hash_input);

        info!("  ✓ Parsed {} recipes successfully", sorted_parsed.len());
        debug!("  Stage hash: {}", stage_hash.hash);

        Ok((sorted_parsed, stage_hash))
    }

    /// Parse a single recipe file
    async fn parse_single_recipe(
        recipe_file: RecipeFile,
        task_extractor: Arc<TaskExtractor>,
    ) -> Result<Option<ParsedRecipe>, Box<dyn std::error::Error + Send + Sync>> {
        // Read file content
        let content = match tokio::fs::read_to_string(&recipe_file.path).await {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read {}: {}", recipe_file.path.display(), e);
                return Ok(None);
            }
        };

        // Compute content hash
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        // Extract task implementations from main file
        let recipe_path = recipe_file.path.clone();
        let (task_impls, helper_funcs) = tokio::task::spawn_blocking({
            let content = content.clone();
            let task_extractor = Arc::clone(&task_extractor);
            let recipe_path = recipe_path.clone();
            move || {
                Self::extract_tasks_with_includes(&content, &recipe_path, &task_extractor)
            }
        })
        .await?;

        Ok(Some(ParsedRecipe {
            file: recipe_file,
            content,
            task_impls,
            helper_funcs,
            hash,
        }))
    }

    /// Extract task implementations and helper functions from recipe and all its included files
    fn extract_tasks_with_includes(
        content: &str,
        recipe_path: &Path,
        task_extractor: &TaskExtractor,
    ) -> (HashMap<String, TaskImplementation>, HashMap<String, TaskImplementation>) {
        

        // Extract tasks and helpers from main recipe
        let main_impls = task_extractor.extract_all_from_content(content);
        let mut all_tasks = main_impls.tasks.clone();
        let mut all_helpers = main_impls.helpers.clone();
        let main_task_count = all_tasks.len();
        let main_helper_count = all_helpers.len();

        // Find all require and include directives
        let includes = Self::find_include_directives(content);

        // Get the directory containing the recipe
        let recipe_dir = recipe_path.parent().unwrap_or_else(|| Path::new("."));

        // Process each included file
        for include_path in &includes {
            // Try to find the include file
            if let Some(resolved_path) = Self::resolve_include_path(include_path, recipe_dir) {
                // Read and parse the include file
                if let Ok(include_content) = std::fs::read_to_string(&resolved_path) {
                    let include_impls = task_extractor.extract_all_from_content(&include_content);

                    if !include_impls.tasks.is_empty() || !include_impls.helpers.is_empty() {
                        info!("✓ Extracted {} tasks from {:?} for recipe {:?}",
                              include_impls.tasks.len(),
                              resolved_path.file_name().unwrap_or(std::ffi::OsStr::new("unknown")),
                              recipe_path.file_name().unwrap_or(std::ffi::OsStr::new("unknown")));
                    }

                    // Merge tasks from include file
                    // Include files are processed first, so recipe can override
                    all_tasks = task_extractor.merge_implementations(&include_impls.tasks, &all_tasks);

                    // Merge helpers from include file (recipe helpers take precedence)
                    for (name, helper) in include_impls.helpers {
                        all_helpers.entry(name).or_insert(helper);
                    }
                } else {
                    info!("✗ Failed to read include file: {:?}", resolved_path);
                }
            } else if include_path.ends_with(".inc") {
                info!("✗ Include file not found: {} (recipe: {:?})",
                      include_path,
                      recipe_path.file_name().unwrap_or(std::ffi::OsStr::new("unknown")));
            }
        }

        let total_tasks = all_tasks.len();
        let total_helpers = all_helpers.len();
        if total_tasks > main_task_count || total_helpers > main_helper_count {
            info!("Recipe {:?}: {} tasks total ({} from main, {} from includes), {} helpers total ({} from main, {} from includes)",
                  recipe_path.file_name().unwrap_or(std::ffi::OsStr::new("unknown")),
                  total_tasks, main_task_count, total_tasks.saturating_sub(main_task_count),
                  total_helpers, main_helper_count, total_helpers.saturating_sub(main_helper_count));
        }

        (all_tasks, all_helpers)
    }

    /// Find all require, include, and inherit directives in recipe content
    fn find_include_directives(content: &str) -> Vec<String> {
        let mut includes = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Match: require filename.inc
            if trimmed.starts_with("require ")
                && let Some(path) = trimmed.strip_prefix("require ") {
                    let path = path.trim();
                    if !path.is_empty() {
                        includes.push(path.to_string());
                    }
                }

            // Match: include filename.inc
            if trimmed.starts_with("include ")
                && let Some(path) = trimmed.strip_prefix("include ") {
                    let path = path.trim();
                    if !path.is_empty() {
                        includes.push(path.to_string());
                    }
                }

            // Match: inherit class1 class2 class3
            if trimmed.starts_with("inherit ")
                && let Some(classes) = trimmed.strip_prefix("inherit ") {
                    // Split by whitespace and add .bbclass extension
                    for class_name in classes.split_whitespace() {
                        if !class_name.is_empty() {
                            includes.push(format!("{class_name}.bbclass"));
                        }
                    }
                }
        }

        includes
    }

    /// Resolve an include path relative to recipe directory and layer classes
    fn resolve_include_path(include_path: &str, recipe_dir: &Path) -> Option<PathBuf> {
        // First try relative to recipe's directory
        let relative_path = recipe_dir.join(include_path);
        if relative_path.exists() {
            return Some(relative_path);
        }

        // Try in the same directory (common for .inc files)
        if let Some(filename) = Path::new(include_path).file_name() {
            let same_dir = recipe_dir.join(filename);
            if same_dir.exists() {
                return Some(same_dir);
            }
        }

        // For .bbclass files, search in classes directory relative to layer root
        if include_path.ends_with(".bbclass") {
            // Try to find the layer root (go up until we find conf/layer.conf)
            let mut current = recipe_dir;
            while let Some(parent) = current.parent() {
                let layer_conf = parent.join("conf/layer.conf");
                if layer_conf.exists() {
                    // Found layer root, check classes directory
                    let class_path = parent.join("classes").join(include_path);
                    if class_path.exists() {
                        return Some(class_path);
                    }
                    break;
                }
                current = parent;
            }
        }

        // Could expand to search in BBPATH, classes directory, etc.
        // For now, just these locations
        None
    }

    /// Stage 3: Build recipe graph (sequential - needs all recipes)
    pub fn build_recipe_graph(
        &self,
        parsed_recipes: &[ParsedRecipe],
        extractor: &RecipeExtractor,
    ) -> Result<(RecipeGraph, StageHash), Box<dyn std::error::Error + Send + Sync>> {
        info!("Stage 3: Building recipe dependency graph");

        let mut graph = RecipeGraph::new();
        let mut extractions = Vec::new();

        // Extract recipe metadata (still fast, but sequential)
        // Use extract_from_file to properly process includes for DEPENDS
        for parsed in parsed_recipes {
            match extractor.extract_from_file(&mut graph, &parsed.file.path) {
                Ok(extraction) => {
                    extractions.push(extraction);
                }
                Err(e) => {
                    debug!("Skipping {}: {}", parsed.file.name, e);
                }
            }
        }

        // Debug: Check busybox and libxcrypt task counts after extraction
        if let Some(busybox_id) = graph.find_recipe("busybox") {
            let busybox_tasks = graph.get_recipe_tasks(busybox_id);
            info!("  After extract_from_file: busybox has {} tasks", busybox_tasks.len());
            if busybox_tasks.len() < 15 {
                for task in &busybox_tasks {
                    info!("    - {}", task.name);
                }
            }
        }
        if let Some(libxcrypt_id) = graph.find_recipe("libxcrypt") {
            let libxcrypt_tasks = graph.get_recipe_tasks(libxcrypt_id);
            info!("  After extract_from_file: libxcrypt has {} tasks", libxcrypt_tasks.len());
        }

        // Add tasks from extracted task implementations
        info!("Adding tasks from extracted implementations");

        // Collect tasks to add (to avoid borrow checker issues)
        let mut tasks_to_add = Vec::new();
        for parsed in parsed_recipes {
            if let Some(recipe_id) = graph.find_recipe(&parsed.file.name) {
                // Get existing tasks for this recipe
                let existing_tasks = graph.get_recipe_tasks(recipe_id);
                let existing_names: HashSet<&str> = existing_tasks.iter()
                    .map(|t| t.name.as_str())
                    .collect();

                for task_name in parsed.task_impls.keys() {
                    // Only add if task doesn't already exist for this recipe
                    if !existing_names.contains(task_name.as_str()) {
                        tasks_to_add.push((recipe_id, task_name.clone()));
                    }
                }
            }
        }

        // Add all tasks
        for (recipe_id, task_name) in &tasks_to_add {
            graph.add_task(*recipe_id, task_name);
        }

        info!("  Added {} tasks from implementations", tasks_to_add.len());

        // Debug: Log extraction depends
        for extraction in &extractions {
            if !extraction.depends.is_empty() {
                info!("  Recipe '{}' DEPENDS: {:?}", extraction.name, extraction.depends);
            }
        }

        // Populate dependencies
        info!("  Populating {} recipe dependencies...", extractions.len());
        extractor.populate_dependencies(&mut graph, &extractions)?;

        // Compute stage hash
        let stats = graph.statistics();
        let hash_input = format!(
            "recipes:{},tasks:{},deps:{}",
            stats.recipe_count, stats.task_count, stats.total_dependencies
        );
        let stage_hash = StageHash::from_string("graph", &hash_input);

        info!("  Built graph: {} recipes, {} tasks", stats.recipe_count, stats.task_count);
        debug!("  Stage hash: {}", stage_hash.hash);

        Ok((graph, stage_hash))
    }

    /// Get cache path for a stage
    fn cache_path(&self, stage: &str) -> PathBuf {
        self.config.cache_dir.join(format!("{stage}.cache"))
    }

    /// Save stage hash to cache
    pub async fn save_stage_hash(&self, stage_hash: &StageHash) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.config.enable_cache {
            return Ok(());
        }

        tokio::fs::create_dir_all(&self.config.cache_dir).await?;
        let cache_path = self.cache_path(&stage_hash.stage);

        let json = serde_json::to_string(stage_hash)?;
        tokio::fs::write(&cache_path, json).await?;

        debug!("Saved {} stage hash to cache", stage_hash.stage);
        Ok(())
    }

    /// Load stage hash from cache
    pub async fn load_stage_hash(&self, stage: &str) -> Option<StageHash> {
        if !self.config.enable_cache {
            return None;
        }

        let cache_path = self.cache_path(stage);
        let json = tokio::fs::read_to_string(&cache_path).await.ok()?;
        serde_json::from_str(&json).ok()
    }

    /// Check if stage needs to be recomputed
    pub async fn needs_recompute(&self, stage: &str, current_hash: &str) -> bool {
        if let Some(cached) = self.load_stage_hash(stage).await {
            if cached.hash == current_hash {
                info!("  ✓ Stage '{}' unchanged (cache hit)", stage);
                false
            } else {
                info!("  ↻ Stage '{}' changed (cache miss)", stage);
                true
            }
        } else {
            info!("  ↻ Stage '{}' not cached", stage);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_hash() {
        let hash1 = StageHash::from_string("test", "data1");
        let hash2 = StageHash::from_string("test", "data1");
        let hash3 = StageHash::from_string("test", "data2");

        assert_eq!(hash1.hash, hash2.hash);
        assert_ne!(hash1.hash, hash3.hash);
    }

    #[test]
    fn test_recipe_file_ordering() {
        let r1 = RecipeFile {
            path: PathBuf::from("/a/recipe1.bb"),
            name: "recipe1".to_string(),
            layer: "meta".to_string(),
            mtime: 100,
            size: 1000,
        };

        let r2 = RecipeFile {
            path: PathBuf::from("/b/recipe2.bb"),
            name: "recipe2".to_string(),
            layer: "meta".to_string(),
            mtime: 200,
            size: 2000,
        };

        assert!(r1.path < r2.path);
    }
}
