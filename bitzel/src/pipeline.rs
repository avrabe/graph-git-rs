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

use convenient_bitbake::{
    BuildContext, ContentHash, ExtractionConfig, RecipeExtractor, RecipeGraph,
    TaskExtractor, TaskImplementation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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
            cache_dir: PathBuf::from(".bitzel-cache/pipeline"),
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
                Err(e) => return Err(format!("Task join error: {}", e).into()),
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
            .filter_map(|e| e.ok())
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
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

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
        info!("Stage 2: Parsing {} recipes in parallel", recipes.len());

        let task_extractor = Arc::new(TaskExtractor::new());

        // Process recipes in batches to limit parallelism
        let chunk_size = self.config.max_io_parallelism;
        let mut all_parsed = Vec::new();

        for chunk in recipes.chunks(chunk_size) {
            let mut tasks = Vec::new();

            for recipe_file in chunk {
                let recipe_file = recipe_file.clone();
                let task_extractor = Arc::clone(&task_extractor);

                let task = tokio::spawn(async move {
                    Self::parse_single_recipe(recipe_file, task_extractor).await
                });

                tasks.push(task);
            }

            // Wait for this batch
            for task in tasks {
                if let Ok(Ok(Some(parsed))) = task.await {
                    all_parsed.push(parsed);
                }
            }
        }

        // Sort for deterministic hashing
        all_parsed.sort_by(|a, b| a.file.path.cmp(&b.file.path));

        // Compute stage hash
        let mut hash_input = String::new();
        for parsed in &all_parsed {
            hash_input.push_str(&format!("{}:{}\n", parsed.file.path.display(), parsed.hash));
        }
        let stage_hash = StageHash::from_string("parse", &hash_input);

        info!("  Parsed {} recipes successfully", all_parsed.len());
        debug!("  Stage hash: {}", stage_hash.hash);

        Ok((all_parsed, stage_hash))
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

        // Extract task implementations (CPU-bound work)
        let task_impls = tokio::task::spawn_blocking({
            let content = content.clone();
            move || task_extractor.extract_from_content(&content)
        })
        .await?;

        Ok(Some(ParsedRecipe {
            file: recipe_file,
            content,
            task_impls,
            hash,
        }))
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
        for parsed in parsed_recipes {
            match extractor.extract_from_content(&mut graph, &parsed.file.name, &parsed.content) {
                Ok(extraction) => {
                    extractions.push(extraction);
                }
                Err(e) => {
                    debug!("Skipping {}: {}", parsed.file.name, e);
                }
            }
        }

        // Populate dependencies
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
        self.config.cache_dir.join(format!("{}.cache", stage))
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
        match self.load_stage_hash(stage).await {
            Some(cached) => {
                if cached.hash == current_hash {
                    info!("  ✓ Stage '{}' unchanged (cache hit)", stage);
                    false
                } else {
                    info!("  ↻ Stage '{}' changed (cache miss)", stage);
                    true
                }
            }
            None => {
                info!("  ↻ Stage '{}' not cached", stage);
                true
            }
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
