//! Main task executor - brings together caching, sandboxing, and execution

use super::cache::{ActionCache, ContentAddressableStore};
use super::sandbox::SandboxManager;
use super::types::{
    ContentHash, ExecutionError, ExecutionResult, SandboxSpec,
    TaskOutput, TaskSignature, TaskSpec,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{info, debug, warn};

/// Main task executor with caching and sandboxing
pub struct TaskExecutor {
    /// Content-addressable store for caching files
    cas: ContentAddressableStore,
    /// Action cache for task results
    action_cache: ActionCache,
    /// Sandbox manager
    sandbox_manager: SandboxManager,
    /// Statistics
    stats: ExecutionStats,
}

impl TaskExecutor {
    /// Create a new task executor
    pub fn new(cache_dir: impl AsRef<Path>) -> ExecutionResult<Self> {
        let cache_dir = cache_dir.as_ref();
        let cas_dir = cache_dir.join("cas");
        let action_cache_dir = cache_dir.join("action-cache");
        let sandbox_dir = cache_dir.join("sandboxes");

        info!("Initializing task executor");
        debug!("CAS directory: {}", cas_dir.display());
        debug!("Action cache directory: {}", action_cache_dir.display());
        debug!("Sandbox directory: {}", sandbox_dir.display());

        Ok(Self {
            cas: ContentAddressableStore::new(cas_dir)?,
            action_cache: ActionCache::new(action_cache_dir)?,
            sandbox_manager: SandboxManager::new(sandbox_dir)?,
            stats: ExecutionStats::default(),
        })
    }

    /// Execute a task with caching
    pub fn execute_task(&mut self, spec: TaskSpec) -> ExecutionResult<TaskOutput> {
        info!("Executing task: {}:{}", spec.recipe, spec.name);

        // 1. Compute signature
        let mut signature = self.compute_signature(&spec)?;
        let sig_hash = signature.compute();

        debug!("Task signature: {}", sig_hash);

        // 2. Check cache
        if let Some(cached) = self.action_cache.get(&sig_hash) {
            info!("Cache HIT for {}:{}", spec.recipe, spec.name);
            self.stats.cache_hits += 1;
            return Ok(cached.clone());
        }

        info!("Cache MISS for {}:{}", spec.recipe, spec.name);
        self.stats.cache_misses += 1;

        // 3. Execute in sandbox
        let start = Instant::now();
        let mut sandbox_spec = self.prepare_sandbox(&spec)?;
        let mut sandbox = self.sandbox_manager.create_sandbox(sandbox_spec.clone())?;

        // Update environment variables with actual sandbox paths
        let sandbox_root = sandbox.root().to_path_buf();
        sandbox_spec.env.insert("WORKDIR".to_string(), sandbox_root.join("work").to_string_lossy().to_string());
        sandbox_spec.env.insert("S".to_string(), sandbox_root.join("work/src").to_string_lossy().to_string());
        sandbox_spec.env.insert("B".to_string(), sandbox_root.join("work/build").to_string_lossy().to_string());
        sandbox_spec.env.insert("D".to_string(), sandbox_root.join("work/outputs").to_string_lossy().to_string());
        sandbox.update_env(sandbox_spec.env);

        info!("Executing in sandbox: {}", sandbox_root.display());
        let result = sandbox.execute()?;

        if !result.success() {
            warn!("Task failed with exit code: {}", result.exit_code);
            warn!("Stdout: {}", result.stdout);
            warn!("Stderr: {}", result.stderr);
            return Err(ExecutionError::TaskFailed(result.exit_code));
        }

        // 4. Collect and hash outputs
        let output_map = sandbox.collect_outputs()?;
        let mut output_files = HashMap::new();

        for (path, content) in output_map {
            let hash = self.cas.put(&content)?;
            output_files.insert(path, hash);
        }

        let duration = start.elapsed();

        let task_output = TaskOutput {
            signature: sig_hash.clone(),
            output_files,
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
            duration_ms: duration.as_millis() as u64,
        };

        // 5. Store in cache
        self.action_cache.put(sig_hash, task_output.clone())?;

        // 6. Cleanup sandbox
        sandbox.cleanup()?;

        info!("Task completed in {}ms", task_output.duration_ms);
        self.stats.tasks_executed += 1;

        Ok(task_output)
    }

    /// Compute task signature from spec
    fn compute_signature(&mut self, spec: &TaskSpec) -> ExecutionResult<TaskSignature> {
        let mut sig = TaskSignature {
            recipe: spec.recipe.clone(),
            task: spec.name.clone(),
            input_files: HashMap::new(),
            dep_signatures: Vec::new(),
            env_vars: spec.env.clone(),
            task_code_hash: ContentHash::from_bytes(spec.script.as_bytes()),
            signature: None,
        };

        // Hash input files in workdir
        if spec.workdir.exists() {
            self.hash_directory(&spec.workdir, &mut sig.input_files)?;
        }

        Ok(sig)
    }

    /// Recursively hash all files in a directory
    fn hash_directory(
        &mut self,
        dir: &Path,
        hashes: &mut HashMap<PathBuf, ContentHash>,
    ) -> ExecutionResult<()> {
        if !dir.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let path = entry.path();
                let hash = ContentHash::from_file(path)?;
                let rel_path = path.strip_prefix(dir)
                    .unwrap_or(path)
                    .to_path_buf();
                hashes.insert(rel_path, hash);
            }
        }

        Ok(())
    }

    /// Prepare sandbox spec from task spec
    fn prepare_sandbox(&self, spec: &TaskSpec) -> ExecutionResult<SandboxSpec> {
        let mut sandbox_spec = SandboxSpec::new(vec![spec.script.clone()]);

        // Mount workdir as read-write
        sandbox_spec.rw_dirs.push(PathBuf::from("/work"));
        sandbox_spec.rw_dirs.push(PathBuf::from("/work/outputs"));
        sandbox_spec.rw_dirs.push(PathBuf::from("/work/build"));
        sandbox_spec.rw_dirs.push(PathBuf::from("/work/temp"));
        sandbox_spec.cwd = PathBuf::from("/work");

        // If workdir exists, mount it
        if spec.workdir.exists() {
            sandbox_spec.ro_inputs.push((
                spec.workdir.clone(),
                PathBuf::from("/work/src"),
            ));
        }

        // Add declared outputs
        for output in &spec.outputs {
            sandbox_spec.outputs.push(PathBuf::from("/work/outputs").join(output));
        }

        // Copy environment
        sandbox_spec.env = spec.env.clone();

        // BitBake environment variables will be set after sandbox creation with actual paths

        Ok(sandbox_spec)
    }

    /// Restore task outputs to a directory
    pub fn restore_outputs(
        &self,
        output: &TaskOutput,
        dest_dir: &Path,
    ) -> ExecutionResult<()> {
        std::fs::create_dir_all(dest_dir)?;

        for (path, hash) in &output.output_files {
            let dest_path = dest_dir.join(path);
            self.cas.get_file(hash, &dest_path)?;
        }

        Ok(())
    }

    /// Get executor statistics
    pub fn stats(&self) -> &ExecutionStats {
        &self.stats
    }

    /// Clean up old sandboxes
    pub fn cleanup_sandboxes(&self) -> ExecutionResult<usize> {
        self.sandbox_manager.cleanup()
    }

    /// Get CAS statistics
    pub fn cas_stats(&self) -> super::cache::CacheStats {
        self.cas.stats()
    }

    /// Get action cache statistics
    pub fn action_cache_stats(&self) -> super::cache::ActionCacheStats {
        self.action_cache.stats()
    }
}

/// Execution statistics
#[derive(Debug, Default, Clone)]
pub struct ExecutionStats {
    pub tasks_executed: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

impl ExecutionStats {
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            (self.cache_hits as f64) / (total as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_simple_task_execution() {
        let tmp = TempDir::new().unwrap();
        let mut executor = TaskExecutor::new(tmp.path()).unwrap();

        let spec = TaskSpec {
            name: "do_test".to_string(),
            recipe: "test-recipe".to_string(),
            script: "echo 'Hello from task' > /work/outputs/result.txt".to_string(),
            workdir: tmp.path().join("workdir"),
            env: HashMap::new(),
            outputs: vec![PathBuf::from("result.txt")],
            timeout: None,
        };

        std::fs::create_dir_all(&spec.workdir).unwrap();

        let output = executor.execute_task(spec).unwrap();

        assert_eq!(output.exit_code, 0);
        assert_eq!(output.output_files.len(), 1);
        assert_eq!(executor.stats().cache_misses, 1);
    }

    #[test]
    fn test_task_caching() {
        let tmp = TempDir::new().unwrap();
        let mut executor = TaskExecutor::new(tmp.path()).unwrap();

        let workdir = tmp.path().join("workdir");
        std::fs::create_dir_all(&workdir).unwrap();

        let spec = TaskSpec {
            name: "do_cached".to_string(),
            recipe: "cache-test".to_string(),
            script: "echo 'cached output' > /work/outputs/out.txt".to_string(),
            workdir: workdir.clone(),
            env: HashMap::new(),
            outputs: vec![PathBuf::from("out.txt")],
            timeout: None,
        };

        // First execution - cache miss
        let output1 = executor.execute_task(spec.clone()).unwrap();
        assert_eq!(executor.stats().cache_misses, 1);
        assert_eq!(executor.stats().cache_hits, 0);

        // Second execution - cache hit
        let output2 = executor.execute_task(spec.clone()).unwrap();
        assert_eq!(executor.stats().cache_hits, 1);
        assert_eq!(executor.stats().cache_misses, 1);

        // Outputs should be identical
        assert_eq!(output1.signature, output2.signature);
    }

    #[test]
    fn test_task_failure() {
        let tmp = TempDir::new().unwrap();
        let mut executor = TaskExecutor::new(tmp.path()).unwrap();

        let spec = TaskSpec {
            name: "do_fail".to_string(),
            recipe: "fail-recipe".to_string(),
            script: "exit 1".to_string(),
            workdir: tmp.path().join("workdir"),
            env: HashMap::new(),
            outputs: vec![],
            timeout: None,
        };

        let result = executor.execute_task(spec);
        assert!(result.is_err());

        match result {
            Err(ExecutionError::TaskFailed(code)) => assert_eq!(code, 1),
            _ => panic!("Expected TaskFailed error"),
        }
    }
}
