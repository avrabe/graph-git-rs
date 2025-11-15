//! Async task executor for parallel execution
//! WASM-compatible using platform-agnostic async

use super::executor::TaskExecutor;
use super::types::{ExecutionResult, TaskOutput, TaskSpec, NetworkPolicy, ResourceLimits};
use crate::task_graph::TaskGraph;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[cfg(feature = "async-executor")]
use futures::future::join_all;

#[cfg(feature = "async-executor")]
use tokio::sync::RwLock;

#[cfg(not(feature = "async-executor"))]
use std::sync::RwLock;

/// Async task executor that runs tasks in parallel
pub struct AsyncTaskExecutor {
    executor: Arc<RwLock<TaskExecutor>>,
}

impl AsyncTaskExecutor {
    /// Create new async executor
    pub fn new(executor: TaskExecutor) -> Self {
        Self {
            executor: Arc::new(RwLock::new(executor)),
        }
    }

    /// Execute a task graph with maximum parallelism
    #[cfg(feature = "async-executor")]
    pub async fn execute_graph(
        &self,
        task_graph: &TaskGraph,
        task_specs: HashMap<String, TaskSpec>,
    ) -> ExecutionResult<HashMap<String, TaskOutput>> {
        let mut completed = HashSet::new();
        let mut results = HashMap::new();

        // Execute in waves (by dependency level)
        while completed.len() < task_graph.tasks.len() {
            // Get all ready tasks (dependencies satisfied)
            let ready_tasks: Vec<_> = task_graph
                .get_ready_tasks(&completed)
                .into_iter()
                .filter_map(|task_id| task_graph.get_task(task_id))
                .collect();

            if ready_tasks.is_empty() && completed.len() < task_graph.tasks.len() {
                return Err(super::types::ExecutionError::SandboxError(
                    "Deadlock detected: no tasks ready but graph incomplete".to_string(),
                ));
            }

            // Execute ready tasks in parallel
            let futures: Vec<_> = ready_tasks
                .iter()
                .filter_map(|task| {
                    let task_key = format!("{}:{}", task.recipe_name, task.task_name);
                    task_specs.get(&task_key).map(|spec| {
                        let executor = Arc::clone(&self.executor);
                        let spec = spec.clone();
                        let task_id = task.task_id;
                        let task_key = task_key.clone();

                        async move {
                            let output = {
                                let mut exec = executor.write().await;
                                exec.execute_task(spec)?
                            };
                            Ok::<_, super::types::ExecutionError>((task_id, task_key, output))
                        }
                    })
                })
                .collect();

            // Wait for all tasks in this wave to complete
            let wave_results = join_all(futures).await;

            for result in wave_results {
                let (task_id, task_key, output) = result?;
                completed.insert(task_id);
                results.insert(task_key, output);
            }
        }

        Ok(results)
    }

    /// Execute a single task asynchronously
    #[cfg(feature = "async-executor")]
    pub async fn execute_task(&self, spec: TaskSpec) -> ExecutionResult<TaskOutput> {
        let mut executor = self.executor.write().await;
        executor.execute_task(spec)
    }

    /// Get executor statistics
    #[cfg(feature = "async-executor")]
    pub async fn stats(&self) -> super::executor::ExecutionStats {
        let executor = self.executor.read().await;
        executor.stats().clone()
    }
}

// For non-async environments, provide a blocking wrapper
#[cfg(not(feature = "async-executor"))]
impl AsyncTaskExecutor {
    /// Execute graph in blocking mode (for WASM/non-tokio)
    pub fn execute_graph_blocking(
        &self,
        task_graph: &TaskGraph,
        task_specs: HashMap<String, TaskSpec>,
    ) -> ExecutionResult<HashMap<String, TaskOutput>> {
        // Fallback to sequential execution
        let mut completed = HashSet::new();
        let mut results = HashMap::new();

        for &task_id in &task_graph.execution_order {
            if let Some(task) = task_graph.get_task(task_id) {
                let task_key = format!("{}:{}", task.recipe_name, task.task_name);
                if let Some(spec) = task_specs.get(&task_key) {
                    let mut executor = self.executor.write().unwrap();
                    let output = executor.execute_task(spec.clone())?;
                    results.insert(task_key, output);
                    completed.insert(task_id);
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe_graph::RecipeGraph;
    use crate::task_graph::TaskGraphBuilder;
    use tempfile::TempDir;

    #[tokio::test]
    #[cfg(feature = "async-executor")]
    async fn test_async_execution() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");

        let executor = TaskExecutor::new(&cache_dir).unwrap();
        let async_executor = AsyncTaskExecutor::new(executor);

        let spec = TaskSpec {
            name: "test".to_string(),
            recipe: "test-recipe".to_string(),
            script: "echo 'test'".to_string(),
            workdir: tmp.path().to_path_buf(),
            env: HashMap::new(),
            outputs: vec![],
            timeout: Some(std::time::Duration::from_secs(30)),
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
        };

        let result = async_executor.execute_task(spec).await;
        assert!(result.is_ok());
    }
}
