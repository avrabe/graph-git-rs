//! Enhanced caching system with proper task signature computation
//!
//! Implements Bazel-style content-addressable caching where task signatures
//! are computed from:
//! - Recipe file content hash
//! - Dependency task signatures (transitive)
//! - Environment variables
//! - Task implementation code
//!
//! This ensures that changes propagate correctly through the dependency graph.

use crate::{TaskGraph, TaskImplementation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

/// Task signature with all inputs that affect the build
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedTaskSignature {
    /// Recipe name
    pub recipe: String,

    /// Task name
    pub task: String,

    /// Recipe file content hash
    pub recipe_hash: String,

    /// Task implementation code hash
    pub task_code_hash: String,

    /// Dependency task signatures (sorted)
    pub dep_signatures: Vec<String>,

    /// Environment variables that affect build (sorted)
    pub env_vars: HashMap<String, String>,

    /// MACHINE override
    pub machine: Option<String>,

    /// DISTRO override
    pub distro: Option<String>,

    /// Final combined signature
    pub signature: Option<String>,
}

impl EnhancedTaskSignature {
    /// Create a new task signature
    pub fn new(
        recipe: String,
        task: String,
        recipe_hash: String,
        task_code: &str,
        dep_signatures: Vec<String>,
        env_vars: HashMap<String, String>,
        machine: Option<String>,
        distro: Option<String>,
    ) -> Self {
        // Hash the task code
        let mut hasher = Sha256::new();
        hasher.update(task_code.as_bytes());
        let task_code_hash = format!("{:x}", hasher.finalize());

        Self {
            recipe,
            task,
            recipe_hash,
            task_code_hash,
            dep_signatures,
            env_vars,
            machine,
            distro,
            signature: None,
        }
    }

    /// Compute the final signature from all inputs
    pub fn compute(&mut self) -> String {
        let mut hasher = Sha256::new();

        // Add recipe and task name
        hasher.update(self.recipe.as_bytes());
        hasher.update(b"|");
        hasher.update(self.task.as_bytes());
        hasher.update(b"|");

        // Add recipe content hash
        hasher.update(self.recipe_hash.as_bytes());
        hasher.update(b"|");

        // Add task code hash
        hasher.update(self.task_code_hash.as_bytes());
        hasher.update(b"|");

        // Add dependency signatures (sorted)
        let mut deps = self.dep_signatures.clone();
        deps.sort();
        for dep in deps {
            hasher.update(dep.as_bytes());
            hasher.update(b"|");
        }

        // Add environment variables (sorted by key)
        let mut env_keys: Vec<_> = self.env_vars.keys().collect();
        env_keys.sort();
        for key in env_keys {
            hasher.update(key.as_bytes());
            hasher.update(b"=");
            hasher.update(self.env_vars[key].as_bytes());
            hasher.update(b"|");
        }

        // Add overrides
        if let Some(machine) = &self.machine {
            hasher.update(b"MACHINE=");
            hasher.update(machine.as_bytes());
            hasher.update(b"|");
        }
        if let Some(distro) = &self.distro {
            hasher.update(b"DISTRO=");
            hasher.update(distro.as_bytes());
            hasher.update(b"|");
        }

        let signature = format!("{:x}", hasher.finalize());
        self.signature = Some(signature.clone());
        signature
    }

    /// Get or compute signature
    pub fn get_signature(&mut self) -> &str {
        if self.signature.is_none() {
            self.compute();
        }
        self.signature.as_ref().unwrap()
    }
}

/// Cache for task signatures and outputs
#[derive(Debug)]
pub struct SignatureCache {
    /// Task signatures by task key (recipe:task)
    signatures: HashMap<String, EnhancedTaskSignature>,

    /// Output cache directory
    cache_dir: PathBuf,
}

impl SignatureCache {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            signatures: HashMap::new(),
            cache_dir,
        }
    }

    /// Compute signatures for all tasks in the graph
    pub async fn compute_signatures(
        &mut self,
        task_graph: &TaskGraph,
        recipe_hashes: &HashMap<String, String>,
        task_impls: &HashMap<String, HashMap<String, TaskImplementation>>,
        machine: Option<&str>,
        distro: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Computing task signatures...");

        let mut computed_signatures: HashMap<String, String> = HashMap::new();

        // Process tasks in execution order (dependencies first)
        for task_id in &task_graph.execution_order {
            if let Some(task) = task_graph.tasks.get(task_id) {
                let task_key = format!("{}:{}", task.recipe_name, task.task_name);

                // Get recipe hash
                let recipe_hash = recipe_hashes
                    .get(&task.recipe_name)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());

                // Get task implementation
                let task_code = task_impls
                    .get(&task.recipe_name)
                    .and_then(|impls| impls.get(&task.task_name))
                    .map(|impl_| impl_.code.as_str())
                    .unwrap_or("");

                // Collect dependency signatures
                let mut dep_sigs = Vec::new();
                for dep_id in &task.depends_on {
                    if let Some(dep_task) = task_graph.tasks.get(dep_id) {
                        let dep_key = format!("{}:{}", dep_task.recipe_name, dep_task.task_name);
                        if let Some(dep_sig) = computed_signatures.get(&dep_key) {
                            dep_sigs.push(dep_sig.clone());
                        }
                    }
                }

                // Build environment variables (basic set)
                let mut env = HashMap::new();
                env.insert("PN".to_string(), task.recipe_name.clone());
                env.insert("TASK".to_string(), task.task_name.clone());

                // Create and compute signature
                let mut sig = EnhancedTaskSignature::new(
                    task.recipe_name.clone(),
                    task.task_name.clone(),
                    recipe_hash,
                    task_code,
                    dep_sigs,
                    env,
                    machine.map(String::from),
                    distro.map(String::from),
                );

                let signature = sig.compute();
                computed_signatures.insert(task_key.clone(), signature.clone());
                self.signatures.insert(task_key, sig);

                debug!("  Task {}: {}",
                    format!("{}:{}", task.recipe_name, task.task_name),
                    &signature[..8]
                );
            }
        }

        info!("  Computed {} task signatures", self.signatures.len());
        Ok(())
    }

    /// Get signature for a task
    pub fn get_signature(&self, recipe: &str, task: &str) -> Option<&str> {
        let key = format!("{}:{}", recipe, task);
        self.signatures.get(&key).and_then(|sig| sig.signature.as_deref())
    }

    /// Save signatures to cache
    pub async fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        let cache_path = self.cache_dir.join("signatures.json");

        let json = serde_json::to_string_pretty(&self.signatures)?;
        tokio::fs::write(&cache_path, json).await?;

        info!("Saved {} task signatures to cache", self.signatures.len());
        Ok(())
    }

    /// Load signatures from cache
    pub async fn load(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let cache_path = self.cache_dir.join("signatures.json");

        if !cache_path.exists() {
            debug!("No signature cache found");
            return Ok(());
        }

        let json = tokio::fs::read_to_string(&cache_path).await?;
        self.signatures = serde_json::from_str(&json)?;

        info!("Loaded {} task signatures from cache", self.signatures.len());
        Ok(())
    }

    /// Check if a task needs to be rebuilt
    pub fn needs_rebuild(
        &self,
        recipe: &str,
        task: &str,
        new_signature: &EnhancedTaskSignature,
    ) -> bool {
        let key = format!("{}:{}", recipe, task);

        match self.signatures.get(&key) {
            Some(cached_sig) => {
                // Compare signatures
                let cached = cached_sig.signature.as_ref().map(|s| s.as_str());
                let new = new_signature.signature.as_ref().map(|s| s.as_str());

                match (cached, new) {
                    (Some(c), Some(n)) if c == n => {
                        debug!("  ✓ Task {}:{} unchanged", recipe, task);
                        false
                    }
                    _ => {
                        info!("  ↻ Task {}:{} needs rebuild", recipe, task);
                        true
                    }
                }
            }
            None => {
                info!("  ↻ Task {}:{} not in cache", recipe, task);
                true
            }
        }
    }
}

/// Statistics about signature changes
#[derive(Debug, Default)]
pub struct SignatureStats {
    /// Total tasks
    pub total: usize,

    /// Tasks with unchanged signatures
    pub unchanged: usize,

    /// Tasks that need rebuild
    pub changed: usize,

    /// Tasks not in cache
    pub new_tasks: usize,
}

impl SignatureStats {
    pub fn print(&self) {
        println!("\n📊 Task Signature Analysis:");
        println!("  Total tasks:    {}", self.total);
        println!("  Unchanged:      {} ({:.1}%)",
                 self.unchanged,
                 (self.unchanged as f64 / self.total as f64) * 100.0);
        println!("  Changed:        {} ({:.1}%)",
                 self.changed,
                 (self.changed as f64 / self.total as f64) * 100.0);
        println!("  New tasks:      {} ({:.1}%)",
                 self.new_tasks,
                 (self.new_tasks as f64 / self.total as f64) * 100.0);
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_stability() {
        let mut sig1 = EnhancedTaskSignature::new(
            "busybox".to_string(),
            "do_compile".to_string(),
            "abc123".to_string(),
            "oe_runmake",
            vec!["dep1".to_string()],
            HashMap::new(),
            Some("qemux86-64".to_string()),
            Some("poky".to_string()),
        );

        let sig1_hash = sig1.compute();

        let mut sig2 = EnhancedTaskSignature::new(
            "busybox".to_string(),
            "do_compile".to_string(),
            "abc123".to_string(),
            "oe_runmake",
            vec!["dep1".to_string()],
            HashMap::new(),
            Some("qemux86-64".to_string()),
            Some("poky".to_string()),
        );

        let sig2_hash = sig2.compute();

        assert_eq!(sig1_hash, sig2_hash, "Identical inputs should produce identical signatures");
    }

    #[test]
    fn test_signature_change_detection() {
        let mut sig1 = EnhancedTaskSignature::new(
            "busybox".to_string(),
            "do_compile".to_string(),
            "abc123".to_string(),
            "oe_runmake",
            vec!["dep1".to_string()],
            HashMap::new(),
            Some("qemux86-64".to_string()),
            None,
        );

        let sig1_hash = sig1.compute();

        // Change task code
        let mut sig2 = EnhancedTaskSignature::new(
            "busybox".to_string(),
            "do_compile".to_string(),
            "abc123".to_string(),
            "oe_runmake --modified",  // Changed!
            vec!["dep1".to_string()],
            HashMap::new(),
            Some("qemux86-64".to_string()),
            None,
        );

        let sig2_hash = sig2.compute();

        assert_ne!(sig1_hash, sig2_hash, "Code change should change signature");
    }

    #[test]
    fn test_dependency_propagation() {
        let mut sig1 = EnhancedTaskSignature::new(
            "busybox".to_string(),
            "do_compile".to_string(),
            "abc123".to_string(),
            "oe_runmake",
            vec!["dep1".to_string()],
            HashMap::new(),
            None,
            None,
        );

        let sig1_hash = sig1.compute();

        // Change dependency signature
        let mut sig2 = EnhancedTaskSignature::new(
            "busybox".to_string(),
            "do_compile".to_string(),
            "abc123".to_string(),
            "oe_runmake",
            vec!["dep2".to_string()],  // Different dep!
            HashMap::new(),
            None,
            None,
        );

        let sig2_hash = sig2.compute();

        assert_ne!(sig1_hash, sig2_hash, "Dependency change should propagate");
    }
}
