//! Main task executor - brings together caching, sandboxing, and execution

use super::cache::{ActionCache, ContentAddressableStore};
use super::direct_executor;
use super::sandbox::SandboxManager;
use super::script_analyzer;
use super::types::{
    ContentHash, ExecutionError, ExecutionMode, ExecutionResult, NetworkPolicy, ResourceLimits,
    SandboxSpec, TaskOutput, TaskSignature, TaskSpec,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, warn};

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
        info!(
            "Executing task: {}:{} (mode: {:?})",
            spec.recipe, spec.name, spec.execution_mode
        );

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

        // 3. Execute based on execution mode
        let (result_stdout, result_stderr, result_exit_code, output_files, duration) =
            match spec.execution_mode {
                ExecutionMode::DirectRust => {
                    // Direct Rust execution - no sandbox, no host contamination
                    info!("Using DirectRust execution (sandbox-free, hermetic)");
                    self.execute_direct_rust(&spec)?
                }
                ExecutionMode::RustShell => {
                    // Rust-based shell execution - in-process bash interpreter
                    info!("Using RustShell execution (in-process, variable tracking)");
                    self.execute_rust_shell(&spec)?
                }
                ExecutionMode::Shell | ExecutionMode::Python => {
                    // Shell/Python execution - requires full sandboxing
                    info!("Using sandboxed execution");
                    self.execute_sandboxed(&spec)?
                }
            };

        let task_output = TaskOutput {
            signature: sig_hash.clone(),
            output_files,
            stdout: result_stdout,
            stderr: result_stderr,
            exit_code: result_exit_code,
            duration_ms: duration,
        };

        // 4. Store in cache
        self.action_cache.put(sig_hash, task_output.clone())?;

        info!("Task completed in {}ms", task_output.duration_ms);
        self.stats.tasks_executed += 1;

        Ok(task_output)
    }

    /// Execute task using direct Rust calls (no sandbox)
    fn execute_direct_rust(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        let start = Instant::now();

        // Analyze script to determine if it can be executed directly
        let analysis = script_analyzer::analyze_script(&spec.script);

        if !analysis.is_simple {
            return Err(ExecutionError::SandboxError(format!(
                "Script is too complex for DirectRust execution: {}",
                analysis.complexity_reason.unwrap_or_default()
            )));
        }

        // Create work directory structure
        let work_dir = spec.workdir.join("direct-rust");
        std::fs::create_dir_all(&work_dir)?;

        let outputs_dir = work_dir.join("outputs");
        std::fs::create_dir_all(&outputs_dir)?;

        // Prepare environment with BitBake-style paths
        let mut env = spec.env.clone();
        env.insert("WORKDIR".to_string(), work_dir.to_string_lossy().to_string());
        env.insert(
            "S".to_string(),
            work_dir.join("src").to_string_lossy().to_string(),
        );
        env.insert(
            "B".to_string(),
            work_dir.join("build").to_string_lossy().to_string(),
        );
        env.insert(
            "D".to_string(),
            outputs_dir.to_string_lossy().to_string(),
        );

        // Execute directly without sandbox
        let result = direct_executor::execute_direct(&analysis, &work_dir, &env)?;

        if result.exit_code != 0 {
            warn!("Direct execution failed with exit code: {}", result.exit_code);
            warn!("Stderr: {}", result.stderr);
            return Err(ExecutionError::TaskFailed(result.exit_code));
        }

        // Collect and hash outputs
        let mut output_files = HashMap::new();
        if outputs_dir.exists() {
            for entry in walkdir::WalkDir::new(&outputs_dir)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() {
                    let path = entry.path();
                    let content = std::fs::read(path)?;
                    let hash = self.cas.put(&content)?;
                    let rel_path = path
                        .strip_prefix(&outputs_dir)
                        .unwrap_or(path)
                        .to_path_buf();
                    output_files.insert(rel_path, hash);
                }
            }
        }

        let duration = start.elapsed().as_millis() as u64;

        info!("DirectRust execution completed successfully (no sandbox used)");

        Ok((
            result.stdout,
            result.stderr,
            result.exit_code,
            output_files,
            duration,
        ))
    }

    /// Execute task using RustShell (brush-shell in-process interpreter)
    fn execute_rust_shell(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        let start = Instant::now();

        info!("Executing with RustShell (in-process bash interpreter)");

        // Execute using RustShell with BitBake environment
        let result = super::rust_shell_executor::execute_with_bitbake_env(
            &spec.script,
            &spec.recipe,
            None, // TODO: Extract version from spec
            &spec.workdir,
            &spec.env,
        )?;

        if result.exit_code != 0 {
            warn!("RustShell execution failed with exit code: {}", result.exit_code);
            warn!("Stderr: {}", result.stderr);
            return Err(ExecutionError::TaskFailed(result.exit_code));
        }

        // Collect and hash outputs
        let outputs_dir = spec.workdir.join("image");
        let mut output_files = HashMap::new();

        if outputs_dir.exists() {
            for entry in walkdir::WalkDir::new(&outputs_dir)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() {
                    let path = entry.path();
                    let content = std::fs::read(path)?;
                    let hash = self.cas.put(&content)?;
                    let rel_path = path
                        .strip_prefix(&outputs_dir)
                        .unwrap_or(path)
                        .to_path_buf();
                    output_files.insert(rel_path, hash);
                }
            }
        }

        let duration = start.elapsed().as_millis() as u64;

        info!(
            "RustShell execution completed successfully ({} vars read, {} vars written)",
            result.vars_read.len(),
            result.vars_written.len()
        );
        debug!("Variables read: {:?}", result.vars_read);
        debug!("Variables written: {:?}", result.vars_written.keys().collect::<Vec<_>>());

        Ok((
            result.stdout,
            result.stderr,
            result.exit_code,
            output_files,
            duration,
        ))
    }

    /// Execute task in sandbox (for Shell/Python modes)
    fn execute_sandboxed(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        let start = Instant::now();
        let mut sandbox_spec = self.prepare_sandbox(spec)?;
        let mut sandbox = self.sandbox_manager.create_sandbox(sandbox_spec.clone())?;

        // Update environment variables with actual sandbox paths
        let sandbox_root = sandbox.root().to_path_buf();
        sandbox_spec.env.insert(
            "WORKDIR".to_string(),
            sandbox_root.join("work").to_string_lossy().to_string(),
        );
        sandbox_spec.env.insert(
            "S".to_string(),
            sandbox_root.join("work/src").to_string_lossy().to_string(),
        );
        sandbox_spec.env.insert(
            "B".to_string(),
            sandbox_root.join("work/build").to_string_lossy().to_string(),
        );
        sandbox_spec.env.insert(
            "D".to_string(),
            sandbox_root
                .join("work/outputs")
                .to_string_lossy()
                .to_string(),
        );
        sandbox.update_env(sandbox_spec.env);

        info!("Executing in sandbox: {}", sandbox_root.display());
        let result = sandbox.execute()?;

        if !result.success() {
            warn!("Task failed with exit code: {}", result.exit_code);
            warn!("Stdout: {}", result.stdout);
            warn!("Stderr: {}", result.stderr);
            return Err(ExecutionError::TaskFailed(result.exit_code));
        }

        // Collect and hash outputs
        let output_map = sandbox.collect_outputs()?;
        let mut output_files = HashMap::new();

        for (path, content) in output_map {
            let hash = self.cas.put(&content)?;
            output_files.insert(path, hash);
        }

        // Cleanup sandbox
        sandbox.cleanup()?;

        let duration = start.elapsed().as_millis() as u64;

        Ok((
            result.stdout,
            result.stderr,
            result.exit_code,
            output_files,
            duration,
        ))
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
            execution_mode: ExecutionMode::Shell,
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
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
            execution_mode: ExecutionMode::Shell,
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
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
            execution_mode: ExecutionMode::Shell,
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
        };

        let result = executor.execute_task(spec);
        assert!(result.is_err());

        match result {
            Err(ExecutionError::TaskFailed(code)) => assert_eq!(code, 1),
            _ => panic!("Expected TaskFailed error"),
        }
    }

    #[test]
    fn test_direct_rust_execution_no_sandbox() {
        let tmp = TempDir::new().unwrap();
        let mut executor = TaskExecutor::new(tmp.path()).unwrap();

        // Simple script that can be executed directly in Rust
        let script = r#"#!/bin/bash
. /hitzeleiter/prelude.sh
export PN="test-recipe"
bb_note "Starting DirectRust execution"
mkdir -p "$D/usr/bin"
touch "$D/usr/bin/myapp"
echo "Hello from Rust!" > "$D/output.txt"
"#;

        let spec = TaskSpec {
            name: "do_install".to_string(),
            recipe: "direct-test".to_string(),
            script: script.to_string(),
            workdir: tmp.path().join("workdir"),
            env: HashMap::new(),
            outputs: vec![PathBuf::from("usr/bin/myapp"), PathBuf::from("output.txt")],
            timeout: None,
            execution_mode: ExecutionMode::DirectRust,
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
        };

        std::fs::create_dir_all(&spec.workdir).unwrap();

        let output = executor.execute_task(spec).unwrap();

        // Verify execution succeeded
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("NOTE: Starting DirectRust execution"));

        // Verify outputs were collected
        assert!(output.output_files.len() >= 1, "Expected at least 1 output file");

        // Verify cache hit on second execution
        assert_eq!(executor.stats().cache_misses, 1);
    }

    #[test]
    fn test_auto_detect_execution_mode() {
        // Simple script should be DirectRust
        let simple_script = r#"#!/bin/bash
. /hitzeleiter/prelude.sh
bb_note "Hello"
touch "$D/file.txt"
"#;
        assert_eq!(
            script_analyzer::determine_execution_mode(simple_script),
            ExecutionMode::DirectRust
        );

        // Complex script should be Shell
        let complex_script = r#"#!/bin/bash
for file in *.txt; do
    echo "Processing $file"
done
"#;
        assert_eq!(
            script_analyzer::determine_execution_mode(complex_script),
            ExecutionMode::Shell
        );

        // Python script should be Python
        let python_script = r#"#!/usr/bin/env python3
print("Hello")
"#;
        assert_eq!(
            script_analyzer::determine_execution_mode(python_script),
            ExecutionMode::Python
        );
    }
}
