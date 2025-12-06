//! SHA256-based build graph caching
//!
//! Caches parsed recipes, recipe graph, and task graph based on content hashes
//! of all input files. This dramatically speeds up repeated builds when nothing
//! has changed.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;
use tracing::{debug, info, warn};

use crate::{RecipeGraph, TaskGraph};
use crate::pipeline::ParsedRecipe;

/// Cache metadata including content hashes of all input files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetadata {
    /// SHA256 hash of all input files combined
    pub content_hash: String,

    /// Individual file hashes for debugging
    pub file_hashes: HashMap<PathBuf, String>,

    /// Timestamp when cache was created
    pub created_at: SystemTime,

    /// MACHINE setting
    pub machine: Option<String>,

    /// DISTRO setting
    pub distro: Option<String>,
}

/// Cached build artifacts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedBuildArtifacts {
    /// Metadata about the cache
    pub metadata: CacheMetadata,

    /// Parsed recipes (serialized)
    pub parsed_recipes: Vec<ParsedRecipe>,

    /// Recipe dependency graph (serialized)
    pub recipe_graph_json: String,

    /// Task execution graph (serialized)
    pub task_graph_json: String,

    /// Task specifications (serialized) - NEW: Cache expensive task specs!
    pub task_specs_json: String,

    /// Task implementations (serialized)
    pub task_implementations_json: String,

    /// Helper implementations (serialized)
    pub helper_implementations_json: String,
}

/// Build cache manager
pub struct BuildCache {
    cache_dir: PathBuf,
}

impl BuildCache {
    /// Create a new build cache manager
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Compute SHA256 hash of a file
    async fn hash_file(path: &Path) -> Result<String, std::io::Error> {
        let content = fs::read(path).await?;
        let hash = Sha256::digest(&content);
        Ok(format!("{:x}", hash))
    }

    /// Compute content hash of all input files
    pub async fn compute_content_hash(
        &self,
        layer_paths: &HashMap<String, Vec<PathBuf>>,
        machine: Option<&str>,
        distro: Option<&str>,
    ) -> Result<CacheMetadata, Box<dyn std::error::Error + Send + Sync>> {
        info!("Computing content hash of all input files for cache validation");

        let mut file_hashes = HashMap::new();
        let mut all_hashes = Vec::new();

        // Hash all recipe files (.bb, .bbappend)
        for layer_path_list in layer_paths.values() {
            for layer_path in layer_path_list {
                // Hash recipes directory
                let recipes_dir = layer_path.join("recipes-*");
                if let Ok(entries) = glob::glob(&recipes_dir.to_string_lossy()) {
                    for entry in entries.flatten() {
                        if let Ok(recipe_entries) = glob::glob(&format!("{}/**/*.bb", entry.display())) {
                            for recipe_file in recipe_entries.flatten() {
                                if let Ok(hash) = Self::hash_file(&recipe_file).await {
                                    file_hashes.insert(recipe_file.clone(), hash.clone());
                                    all_hashes.push(hash);
                                }
                            }
                        }

                        // Also hash .inc files
                        if let Ok(inc_entries) = glob::glob(&format!("{}/**/*.inc", entry.display())) {
                            for inc_file in inc_entries.flatten() {
                                if let Ok(hash) = Self::hash_file(&inc_file).await {
                                    file_hashes.insert(inc_file.clone(), hash.clone());
                                    all_hashes.push(hash);
                                }
                            }
                        }

                        // Also hash .bbappend files
                        if let Ok(append_entries) = glob::glob(&format!("{}/**/*.bbappend", entry.display())) {
                            for append_file in append_entries.flatten() {
                                if let Ok(hash) = Self::hash_file(&append_file).await {
                                    file_hashes.insert(append_file.clone(), hash.clone());
                                    all_hashes.push(hash);
                                }
                            }
                        }
                    }
                }

                // Hash .bbclass files
                let classes_dir = layer_path.join("classes");
                if classes_dir.exists() {
                    if let Ok(class_entries) = glob::glob(&format!("{}/*.bbclass", classes_dir.display())) {
                        for class_file in class_entries.flatten() {
                            if let Ok(hash) = Self::hash_file(&class_file).await {
                                file_hashes.insert(class_file.clone(), hash.clone());
                                all_hashes.push(hash);
                            }
                        }
                    }
                }

                // Hash conf files
                let conf_dir = layer_path.join("conf");
                if conf_dir.exists() {
                    if let Ok(conf_entries) = glob::glob(&format!("{}/**/*.conf", conf_dir.display())) {
                        for conf_file in conf_entries.flatten() {
                            if let Ok(hash) = Self::hash_file(&conf_file).await {
                                file_hashes.insert(conf_file.clone(), hash.clone());
                                all_hashes.push(hash);
                            }
                        }
                    }
                }
            }
        }

        // Sort hashes for deterministic combined hash
        all_hashes.sort();

        // Compute combined hash
        let mut hasher = Sha256::new();
        for hash in &all_hashes {
            hasher.update(hash.as_bytes());
        }

        // Include machine and distro in hash
        if let Some(m) = machine {
            hasher.update(b"MACHINE:");
            hasher.update(m.as_bytes());
        }
        if let Some(d) = distro {
            hasher.update(b"DISTRO:");
            hasher.update(d.as_bytes());
        }

        let content_hash = format!("{:x}", hasher.finalize());

        info!("Computed content hash: {} (from {} files)", content_hash, file_hashes.len());

        Ok(CacheMetadata {
            content_hash,
            file_hashes,
            created_at: SystemTime::now(),
            machine: machine.map(|s| s.to_string()),
            distro: distro.map(|s| s.to_string()),
        })
    }

    /// Load cached build artifacts if available and valid
    pub async fn load(
        &self,
        expected_metadata: &CacheMetadata,
    ) -> Result<Option<CachedBuildArtifacts>, Box<dyn std::error::Error + Send + Sync>> {
        let cache_file = self.cache_dir.join("build_graph.cache.json");

        if !cache_file.exists() {
            info!("No cache file found at {:?}", cache_file);
            return Ok(None);
        }

        info!("Loading build graph cache from {:?}", cache_file);

        let json = fs::read_to_string(&cache_file).await?;
        let cached: CachedBuildArtifacts = serde_json::from_str(&json)?;

        // Validate content hash
        if cached.metadata.content_hash != expected_metadata.content_hash {
            warn!("Cache invalidated: content hash mismatch");
            warn!("  Expected: {}", expected_metadata.content_hash);
            warn!("  Cached:   {}", cached.metadata.content_hash);
            return Ok(None);
        }

        // Validate machine/distro
        if cached.metadata.machine != expected_metadata.machine {
            warn!("Cache invalidated: MACHINE mismatch");
            return Ok(None);
        }

        if cached.metadata.distro != expected_metadata.distro {
            warn!("Cache invalidated: DISTRO mismatch");
            return Ok(None);
        }

        info!("✓ Cache validated and loaded successfully");
        info!("  Created: {:?}", cached.metadata.created_at);
        info!("  Recipes: {}", cached.parsed_recipes.len());

        Ok(Some(cached))
    }

    /// Save build artifacts to cache
    pub async fn save(
        &self,
        metadata: CacheMetadata,
        parsed_recipes: Vec<ParsedRecipe>,
        recipe_graph: &RecipeGraph,
        task_graph: &TaskGraph,
        task_specs: &HashMap<String, crate::TaskSpec>,
        task_implementations: &HashMap<String, HashMap<String, crate::TaskImplementation>>,
        helper_implementations: &HashMap<String, HashMap<String, crate::TaskImplementation>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Create cache directory if it doesn't exist
        fs::create_dir_all(&self.cache_dir).await?;

        let cache_file = self.cache_dir.join("build_graph.cache.json");

        info!("Saving build graph cache to {:?}", cache_file);
        info!("  Caching {} task specs (EXPENSIVE - saves 20-30 minutes next run!)", task_specs.len());

        // Serialize graphs and task specs
        let recipe_graph_json = serde_json::to_string(recipe_graph)?;
        let task_graph_json = serde_json::to_string(task_graph)?;
        let task_specs_json = serde_json::to_string(task_specs)?;
        let task_implementations_json = serde_json::to_string(task_implementations)?;
        let helper_implementations_json = serde_json::to_string(helper_implementations)?;

        let artifacts = CachedBuildArtifacts {
            metadata,
            parsed_recipes,
            recipe_graph_json,
            task_graph_json,
            task_specs_json,
            task_implementations_json,
            helper_implementations_json,
        };

        let json = serde_json::to_string_pretty(&artifacts)?;
        fs::write(&cache_file, json).await?;

        info!("✓ Build graph cache saved successfully (includes task specs)");

        Ok(())
    }
}
