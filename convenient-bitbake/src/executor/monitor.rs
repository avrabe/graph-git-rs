//! Task execution monitoring and statistics
//!
//! Provides real-time task state tracking, performance metrics, and
//! both human-readable and machine-readable (JSON) output formats.

use super::types::{ExecutionResult, TaskOutput};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Task execution state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    /// Task is waiting for dependencies
    Pending,
    /// Task is currently executing
    Running,
    /// Task completed successfully
    Completed,
    /// Task failed with error
    Failed,
    /// Task was skipped (e.g., cached)
    Cached,
    /// Task execution was cancelled
    Cancelled,
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskState::Pending => write!(f, "⏳ Pending"),
            TaskState::Running => write!(f, "▶️  Running"),
            TaskState::Completed => write!(f, "✅ Completed"),
            TaskState::Failed => write!(f, "❌ Failed"),
            TaskState::Cached => write!(f, "💾 Cached"),
            TaskState::Cancelled => write!(f, "⚠️  Cancelled"),
        }
    }
}

/// Task execution information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub task_id: String,
    pub recipe: String,
    pub task_name: String,
    pub state: TaskState,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub duration_ms: Option<u64>,
    pub cache_hit: bool,
    pub error_message: Option<String>,
    pub output_size_bytes: Option<u64>,
}

impl TaskInfo {
    pub fn new(task_id: String, recipe: String, task_name: String) -> Self {
        Self {
            task_id,
            recipe,
            task_name,
            state: TaskState::Pending,
            start_time: None,
            end_time: None,
            duration_ms: None,
            cache_hit: false,
            error_message: None,
            output_size_bytes: None,
        }
    }

    /// Get human-readable status string
    pub fn status_line(&self) -> String {
        let duration_str = if let Some(ms) = self.duration_ms {
            format!(" ({:.2}s)", ms as f64 / 1000.0)
        } else {
            String::new()
        };

        format!(
            "{:12} {:20} {:30} {}{}",
            self.state.to_string(),
            self.recipe,
            self.task_name,
            if self.cache_hit { "💾 " } else { "" },
            duration_str
        )
    }
}

/// Build execution statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStats {
    pub total_tasks: usize,
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub cached: usize,
    pub cancelled: usize,
    pub total_duration_ms: u64,
    pub cache_hit_rate: f64,
    pub avg_task_duration_ms: f64,
    pub slowest_task: Option<String>,
    pub slowest_task_duration_ms: Option<u64>,
}

impl Default for BuildStats {
    fn default() -> Self {
        Self {
            total_tasks: 0,
            pending: 0,
            running: 0,
            completed: 0,
            failed: 0,
            cached: 0,
            cancelled: 0,
            total_duration_ms: 0,
            cache_hit_rate: 0.0,
            avg_task_duration_ms: 0.0,
            slowest_task: None,
            slowest_task_duration_ms: None,
        }
    }
}

impl std::fmt::Display for BuildStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Build Statistics")?;
        writeln!(f, "───────────────────────────────────")?;
        writeln!(f, "Total tasks:     {}", self.total_tasks)?;
        writeln!(f, "  ⏳ Pending:    {}", self.pending)?;
        writeln!(f, "  ▶️  Running:    {}", self.running)?;
        writeln!(f, "  ✅ Completed:  {}", self.completed)?;
        writeln!(f, "  ❌ Failed:     {}", self.failed)?;
        writeln!(f, "  💾 Cached:     {}", self.cached)?;
        writeln!(f, "  ⚠️  Cancelled:  {}", self.cancelled)?;
        writeln!(f)?;
        writeln!(
            f,
            "Total time:      {:.2}s",
            self.total_duration_ms as f64 / 1000.0
        )?;
        writeln!(f, "Cache hit rate:  {:.1}%", self.cache_hit_rate * 100.0)?;
        writeln!(
            f,
            "Avg task time:   {:.2}s",
            self.avg_task_duration_ms / 1000.0
        )?;
        if let Some(ref slowest) = self.slowest_task {
            writeln!(f, "Slowest task:    {}", slowest)?;
            if let Some(ms) = self.slowest_task_duration_ms {
                writeln!(f, "  Duration:      {:.2}s", ms as f64 / 1000.0)?;
            }
        }
        Ok(())
    }
}

/// Task execution monitor
#[derive(Clone)]
pub struct TaskMonitor {
    inner: Arc<Mutex<MonitorInner>>,
}

struct MonitorInner {
    tasks: HashMap<String, TaskInfo>,
    start_time: Instant,
    timing_stack: Vec<(String, Instant)>,
}

impl TaskMonitor {
    /// Create a new task monitor
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MonitorInner {
                tasks: HashMap::new(),
                start_time: Instant::now(),
                timing_stack: Vec::new(),
            })),
        }
    }

    /// Register a new task
    pub fn register_task(&self, task_id: String, recipe: String, task_name: String) {
        let mut inner = self.inner.lock().unwrap();
        let info = TaskInfo::new(task_id.clone(), recipe, task_name);
        inner.tasks.insert(task_id, info);
    }

    /// Mark task as running
    pub fn task_started(&self, task_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        let elapsed = inner.start_time.elapsed().as_millis() as u64;

        if let Some(task) = inner.tasks.get_mut(task_id) {
            task.state = TaskState::Running;
            task.start_time = Some(elapsed);

            // Copy for logging
            let recipe = task.recipe.clone();
            let task_name = task.task_name.clone();

            // Push onto timing stack for tracing
            inner.timing_stack.push((task_id.to_string(), Instant::now()));

            tracing::info!(
                task_id = %task_id,
                recipe = %recipe,
                task_name = %task_name,
                "Task started"
            );
        }
    }

    /// Mark task as completed
    pub fn task_completed(&self, task_id: &str, output: &TaskOutput, cached: bool) {
        let mut inner = self.inner.lock().unwrap();
        let elapsed = inner.start_time.elapsed().as_millis() as u64;

        if let Some(task) = inner.tasks.get_mut(task_id) {
            task.state = if cached {
                TaskState::Cached
            } else {
                TaskState::Completed
            };

            task.end_time = Some(elapsed);

            // Calculate duration
            if let Some(start) = task.start_time {
                task.duration_ms = Some(elapsed - start);
            }

            task.cache_hit = cached;

            // Store number of output files
            task.output_size_bytes = Some(output.output_files.len() as u64);

            // Copy for logging
            let recipe = task.recipe.clone();
            let task_name = task.task_name.clone();

            // Pop from timing stack
            if let Some((tid, start)) = inner.timing_stack.pop()
                && tid == task_id {
                    let duration = start.elapsed();
                    tracing::info!(
                        task_id = %task_id,
                        recipe = %recipe,
                        task_name = %task_name,
                        duration_ms = %duration.as_millis(),
                        cached = %cached,
                        "Task completed"
                    );
                }
        }
    }

    /// Mark task as failed
    pub fn task_failed(&self, task_id: &str, error: &str) {
        let mut inner = self.inner.lock().unwrap();
        let elapsed = inner.start_time.elapsed().as_millis() as u64;

        if let Some(task) = inner.tasks.get_mut(task_id) {
            task.state = TaskState::Failed;
            task.error_message = Some(error.to_string());

            task.end_time = Some(elapsed);

            if let Some(start) = task.start_time {
                task.duration_ms = Some(elapsed - start);
            }

            // Copy for logging
            let recipe = task.recipe.clone();
            let task_name = task.task_name.clone();

            // Pop from timing stack
            inner.timing_stack.retain(|(tid, _)| tid != task_id);

            tracing::error!(
                task_id = %task_id,
                recipe = %recipe,
                task_name = %task_name,
                error = %error,
                "Task failed"
            );
        }
    }

    /// Get current statistics
    pub fn get_stats(&self) -> BuildStats {
        let inner = self.inner.lock().unwrap();
        let mut stats = BuildStats::default();

        stats.total_tasks = inner.tasks.len();

        let mut total_duration = 0u64;
        let mut completed_count = 0usize;
        let mut cached_count = 0usize;
        let mut slowest_duration = 0u64;
        let mut slowest_task = None;

        for task in inner.tasks.values() {
            match task.state {
                TaskState::Pending => stats.pending += 1,
                TaskState::Running => stats.running += 1,
                TaskState::Completed => stats.completed += 1,
                TaskState::Failed => stats.failed += 1,
                TaskState::Cached => {
                    stats.cached += 1;
                    cached_count += 1;
                }
                TaskState::Cancelled => stats.cancelled += 1,
            }

            if let Some(duration) = task.duration_ms {
                total_duration += duration;
                completed_count += 1;

                if duration > slowest_duration {
                    slowest_duration = duration;
                    slowest_task = Some(format!("{}:{}", task.recipe, task.task_name));
                }
            }
        }

        stats.total_duration_ms = total_duration;

        if stats.total_tasks > 0 {
            stats.cache_hit_rate = cached_count as f64 / stats.total_tasks as f64;
        }

        if completed_count > 0 {
            stats.avg_task_duration_ms = total_duration as f64 / completed_count as f64;
        }

        stats.slowest_task = slowest_task;
        stats.slowest_task_duration_ms = if slowest_duration > 0 {
            Some(slowest_duration)
        } else {
            None
        };

        stats
    }

    /// Get all task info
    pub fn get_all_tasks(&self) -> Vec<TaskInfo> {
        let inner = self.inner.lock().unwrap();
        inner.tasks.values().cloned().collect()
    }

    /// Get task info by ID
    pub fn get_task(&self, task_id: &str) -> Option<TaskInfo> {
        let inner = self.inner.lock().unwrap();
        inner.tasks.get(task_id).cloned()
    }

    /// Print human-readable task list
    pub fn print_tasks(&self) {
        let tasks = self.get_all_tasks();

        println!("\n📋 Task Execution Status");
        println!("═══════════════════════════════════════════════════════════════════════════");
        println!(
            "{:12} {:20} {:30} {:10}",
            "State", "Recipe", "Task", "Duration"
        );
        println!("───────────────────────────────────────────────────────────────────────────");

        for task in tasks {
            println!("{}", task.status_line());
        }

        println!("═══════════════════════════════════════════════════════════════════════════\n");
    }

    /// Export to JSON format (machine-readable)
    pub fn to_json(&self) -> ExecutionResult<String> {
        let tasks = self.get_all_tasks();
        let stats = self.get_stats();

        let output = serde_json::json!({
            "version": "1.0",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "statistics": stats,
            "tasks": tasks,
        });

        serde_json::to_string_pretty(&output)
            .map_err(|e| super::types::ExecutionError::CacheError(e.to_string()))
    }

    /// Export to JSON file
    pub fn export_json(&self, path: &std::path::Path) -> ExecutionResult<()> {
        let json = self.to_json()?;
        std::fs::write(path, json)
            .map_err(|e| super::types::ExecutionError::SandboxError(e.to_string()))
    }
}

impl Default for TaskMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_monitor() {
        let monitor = TaskMonitor::new();

        // Register task
        monitor.register_task(
            "task1".to_string(),
            "recipe1".to_string(),
            "do_compile".to_string(),
        );

        // Start task
        monitor.task_started("task1");

        let task = monitor.get_task("task1").unwrap();
        assert_eq!(task.state, TaskState::Running);

        // Complete task
        let output = TaskOutput {
            signature: super::super::types::ContentHash::from_bytes(b"test"),
            output_files: HashMap::new(),
            stdout: String::new(),
            stderr: String::new(),
        };

        monitor.task_completed("task1", &output, false);

        let task = monitor.get_task("task1").unwrap();
        assert_eq!(task.state, TaskState::Completed);
        assert!(task.duration_ms.is_some());

        // Get stats
        let stats = monitor.get_stats();
        assert_eq!(stats.total_tasks, 1);
        assert_eq!(stats.completed, 1);
    }
}
