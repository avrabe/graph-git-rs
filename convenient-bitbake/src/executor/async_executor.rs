//! Async task executor for parallel execution with priority-based scheduling
//! WASM-compatible using platform-agnostic async

use super::executor::TaskExecutor;
use super::types::{ExecutionMode, ExecutionResult, TaskOutput, TaskSpec, NetworkPolicy, ResourceLimits};
use crate::task_graph::TaskGraph;
use crate::scheduler::{TaskScheduler, SchedulerStats};
use crate::recipe_graph::TaskId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "async-executor")]
use futures::future::join_all;

#[cfg(feature = "async-executor")]
use tokio::sync::RwLock;

#[cfg(not(feature = "async-executor"))]
use std::sync::RwLock;

/// Async task executor that runs tasks in parallel with priority-based scheduling
pub struct AsyncTaskExecutor {
    executor: Arc<RwLock<TaskExecutor>>,
    /// Maximum number of parallel tasks
    max_parallel: usize,
}

impl AsyncTaskExecutor {
    /// Create new async executor
    pub fn new(executor: TaskExecutor) -> Self {
        Self::with_parallelism(executor, num_cpus::get())
    }

    /// Create new async executor with custom parallelism
    pub fn with_parallelism(executor: TaskExecutor, max_parallel: usize) -> Self {
        Self {
            executor: Arc::new(RwLock::new(executor)),
            max_parallel,
        }
    }

    /// Execute a task graph with priority-based scheduling
    #[cfg(feature = "async-executor")]
    pub async fn execute_graph_with_scheduler(
        &self,
        task_graph: &TaskGraph,
        task_specs: HashMap<String, TaskSpec>,
        scheduler: &mut TaskScheduler,
        progress_callback: Option<Box<dyn Fn(&ExecutionProgress) + Send + Sync>>,
    ) -> ExecutionResult<ExecutionSummary> {
        let start_time = Instant::now();
        let mut results = HashMap::new();
        let total_tasks = task_graph.tasks.len();

        // Initialize scheduler
        scheduler.initialize();

        while scheduler.get_stats().completed < total_tasks {
            // Get ready tasks from scheduler (up to max_parallel)
            let ready_tasks = scheduler.get_ready_tasks(self.max_parallel);

            if ready_tasks.is_empty() {
                let stats = scheduler.get_stats();
                if stats.completed + stats.running < total_tasks {
                    return Err(super::types::ExecutionError::SandboxError(
                        "Deadlock detected: no tasks ready but graph incomplete".to_string(),
                    ));
                }
                break;
            }

            // Report progress
            if let Some(ref callback) = progress_callback {
                let progress = self.compute_progress(scheduler, &start_time, total_tasks);
                callback(&progress);
            }

            // Execute ready tasks in parallel (limited batch)
            let futures: Vec<_> = ready_tasks
                .iter()
                .filter_map(|scheduled_task| {
                    task_graph.get_task(scheduled_task.task_id).and_then(|task| {
                        let task_key = format!("{}:{}", task.recipe_name, task.task_name);
                        task_specs.get(&task_key).map(|spec| {
                            let executor = Arc::clone(&self.executor);
                            let spec = spec.clone();
                            let task_id = task.task_id;
                            let task_key = task_key.clone();
                            let task_name = task.task_name.clone();
                            let recipe_name = task.recipe_name.clone();

                            async move {
                                let task_start = Instant::now();
                                let output = {
                                    let mut exec = executor.write().await;
                                    exec.execute_task(spec)
                                };
                                let duration = task_start.elapsed();

                                match output {
                                    Ok(output) => Ok((task_id, task_key, recipe_name, task_name, output, duration)),
                                    Err(e) => Err((task_id, task_key, recipe_name, task_name, e, duration)),
                                }
                            }
                        })
                    })
                })
                .collect();

            // Wait for all tasks in this wave to complete
            let wave_results = join_all(futures).await;

            // Process results and update scheduler
            for result in wave_results {
                match result {
                    Ok((task_id, task_key, _recipe, _task, output, _duration)) => {
                        scheduler.mark_completed(task_id);
                        results.insert(task_key, output);
                    }
                    Err((task_id, _task_key, recipe, task, error, _duration)) => {
                        scheduler.mark_failed(task_id);
                        eprintln!("Task failed: {}:{} - {:?}", recipe, task, error);
                        return Err(error);
                    }
                }
            }
        }

        let total_duration = start_time.elapsed();
        let stats = scheduler.get_stats();

        Ok(ExecutionSummary {
            total_tasks,
            completed: stats.completed,
            failed: total_tasks - stats.completed,
            total_duration,
            results,
        })
    }

    /// Compute current execution progress
    fn compute_progress(&self, scheduler: &TaskScheduler, start_time: &Instant, total_tasks: usize) -> ExecutionProgress {
        let stats = scheduler.get_stats();
        let elapsed = start_time.elapsed();
        let completion_rate = if elapsed.as_secs() > 0 {
            stats.completed as f64 / elapsed.as_secs() as f64
        } else {
            0.0
        };

        let remaining_tasks = total_tasks - stats.completed;
        let estimated_remaining = if completion_rate > 0.0 {
            Duration::from_secs_f64(remaining_tasks as f64 / completion_rate)
        } else {
            Duration::from_secs(0)
        };

        ExecutionProgress {
            completed: stats.completed,
            running: stats.running,
            pending: stats.pending,
            total: total_tasks,
            elapsed,
            estimated_remaining,
            completion_percent: (stats.completed as f64 / total_tasks as f64) * 100.0,
            parallelism_utilization: stats.parallelism_utilization(self.max_parallel),
        }
    }

    /// Execute a task graph with maximum parallelism (legacy method)
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
            execution_mode: ExecutionMode::Shell,
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
        };

        let result = async_executor.execute_task(spec).await;
        assert!(result.is_ok());
    }
}

/// Real-time execution progress information
#[derive(Debug, Clone)]
pub struct ExecutionProgress {
    /// Number of completed tasks
    pub completed: usize,
    /// Number of currently running tasks
    pub running: usize,
    /// Number of pending tasks (not yet ready)
    pub pending: usize,
    /// Total number of tasks
    pub total: usize,
    /// Time elapsed since execution started
    pub elapsed: Duration,
    /// Estimated time remaining (based on completion rate)
    pub estimated_remaining: Duration,
    /// Completion percentage (0-100)
    pub completion_percent: f64,
    /// Parallelism utilization percentage (0-100)
    pub parallelism_utilization: f64,
}

impl ExecutionProgress {
    /// Format progress as a human-readable string
    pub fn format(&self) -> String {
        format!(
            "Progress: {}/{} ({:.1}%) | Running: {} | Elapsed: {:?} | ETA: {:?} | Parallelism: {:.1}%",
            self.completed,
            self.total,
            self.completion_percent,
            self.running,
            self.elapsed,
            self.estimated_remaining,
            self.parallelism_utilization
        )
    }

    /// Get completion rate (tasks per second)
    pub fn completion_rate(&self) -> f64 {
        if self.elapsed.as_secs() > 0 {
            self.completed as f64 / self.elapsed.as_secs() as f64
        } else {
            0.0
        }
    }
}

/// Summary of execution results
#[derive(Debug, Clone)]
pub struct ExecutionSummary {
    /// Total number of tasks
    pub total_tasks: usize,
    /// Number of completed tasks
    pub completed: usize,
    /// Number of failed tasks
    pub failed: usize,
    /// Total execution duration
    pub total_duration: Duration,
    /// Task execution results
    pub results: HashMap<String, TaskOutput>,
}

impl ExecutionSummary {
    /// Calculate success rate as percentage
    pub fn success_rate(&self) -> f64 {
        if self.total_tasks == 0 {
            100.0
        } else {
            (self.completed as f64 / self.total_tasks as f64) * 100.0
        }
    }

    /// Calculate average task duration
    pub fn average_task_duration(&self) -> Duration {
        if self.completed == 0 {
            Duration::from_secs(0)
        } else {
            self.total_duration / self.completed as u32
        }
    }

    /// Format summary as human-readable string
    pub fn format(&self) -> String {
        format!(
            "Execution Summary:\n  Total: {} tasks\n  Completed: {} ({:.1}%)\n  Failed: {}\n  Duration: {:?}\n  Avg per task: {:?}",
            self.total_tasks,
            self.completed,
            self.success_rate(),
            self.failed,
            self.total_duration,
            self.average_task_duration()
        )
    }
}
