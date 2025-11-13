//! Interactive task executor with pause/resume/debug capabilities
//!
//! Provides real-time control over task execution with the ability to:
//! - Pause execution at any point
//! - Resume execution
//! - Debug individual tasks
//! - Break on task failure for inspection
//! - Query task state during execution

use super::executor::TaskExecutor;
use super::monitor::{TaskMonitor, TaskState};
use super::types::{ExecutionResult, TaskOutput, TaskSpec};
use crate::task_graph::TaskGraph;
use crate::recipe_graph::TaskId;
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

/// Execution control commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionControl {
    /// Continue normal execution
    Continue,
    /// Pause execution after current wave
    Pause,
    /// Stop execution immediately
    Stop,
    /// Break into debugger
    Debug,
}

/// Interactive executor options
#[derive(Debug, Clone)]
pub struct InteractiveOptions {
    /// Break on first failure
    pub break_on_failure: bool,
    /// Enable interactive mode (pause before each wave)
    pub interactive_mode: bool,
    /// Show task status after each wave
    pub show_progress: bool,
    /// Export JSON after completion
    pub export_json: Option<std::path::PathBuf>,
}

impl Default for InteractiveOptions {
    fn default() -> Self {
        Self {
            break_on_failure: true,
            interactive_mode: false,
            show_progress: true,
            export_json: None,
        }
    }
}

/// Interactive task executor
pub struct InteractiveExecutor {
    executor: TaskExecutor,
    monitor: TaskMonitor,
    options: InteractiveOptions,
    control: Arc<Mutex<ExecutionControl>>,
    paused: Arc<AtomicBool>,
}

impl InteractiveExecutor {
    /// Create a new interactive executor
    pub fn new(
        cache_dir: impl AsRef<std::path::Path>,
        options: InteractiveOptions,
    ) -> ExecutionResult<Self> {
        Ok(Self {
            executor: TaskExecutor::new(cache_dir)?,
            monitor: TaskMonitor::new(),
            options,
            control: Arc::new(Mutex::new(ExecutionControl::Continue)),
            paused: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Get monitor for status queries
    pub fn monitor(&self) -> &TaskMonitor {
        &self.monitor
    }

    /// Get control handle for external control
    pub fn control_handle(&self) -> ExecutionControlHandle {
        ExecutionControlHandle {
            control: Arc::clone(&self.control),
            paused: Arc::clone(&self.paused),
        }
    }

    /// Execute task graph with interactive control
    pub fn execute_graph(
        &mut self,
        task_graph: &TaskGraph,
        task_specs: HashMap<String, TaskSpec>,
    ) -> ExecutionResult<HashMap<String, TaskOutput>> {
        let mut completed: HashSet<TaskId> = HashSet::new();
        let mut results = HashMap::new();
        let mut wave_number = 0;

        // Register all tasks
        for task in task_graph.tasks.values() {
            let task_key = format!("{}:{}", task.recipe_name, task.task_name);
            self.monitor.register_task(
                task_key.clone(),
                task.recipe_name.clone(),
                task.task_name.clone(),
            );
        }

        println!("\n🚀 Starting interactive execution");
        println!("   Total tasks: {}", task_graph.tasks.len());
        println!("   Break on failure: {}", self.options.break_on_failure);
        println!("   Interactive mode: {}\n", self.options.interactive_mode);

        // Execute in waves
        while completed.len() < task_graph.tasks.len() {
            // Check control state
            let control = *self.control.lock().unwrap();
            match control {
                ExecutionControl::Stop => {
                    println!("\n⚠️  Execution stopped by user");
                    break;
                }
                ExecutionControl::Pause => {
                    self.handle_pause();
                    continue;
                }
                ExecutionControl::Debug => {
                    self.enter_debugger(task_graph, &completed);
                    *self.control.lock().unwrap() = ExecutionControl::Continue;
                    continue;
                }
                ExecutionControl::Continue => {}
            }

            // Get ready tasks
            let ready_tasks: Vec<_> = task_graph
                .get_ready_tasks(&completed)
                .into_iter()
                .filter_map(|task_id| task_graph.get_task(task_id))
                .collect();

            if ready_tasks.is_empty() {
                if completed.len() < task_graph.tasks.len() {
                    println!("❌ Deadlock detected: no tasks ready but graph incomplete");
                    break;
                }
                break;
            }

            wave_number += 1;
            println!("\n🌊 Wave {} - {} tasks", wave_number, ready_tasks.len());

            // Interactive pause
            if self.options.interactive_mode {
                self.interactive_prompt(&ready_tasks);
            }

            // Execute tasks in current wave
            for task in ready_tasks {
                let task_key = format!("{}:{}", task.recipe_name, task.task_name);

                if let Some(spec) = task_specs.get(&task_key) {
                    self.monitor.task_started(&task_key);

                    // Execute task
                    match self.executor.execute_task(spec.clone()) {
                        Ok(output) => {
                            let cached = self.executor.stats().cache_hits > 0;
                            self.monitor.task_completed(&task_key, &output, cached);
                            results.insert(task_key.clone(), output);
                            completed.insert(task.task_id);

                            println!(
                                "  ✅ {}:{} {}",
                                task.recipe_name,
                                task.task_name,
                                if cached { "💾" } else { "" }
                            );
                        }
                        Err(e) => {
                            self.monitor.task_failed(&task_key, &e.to_string());
                            println!("  ❌ {}:{} - {}", task.recipe_name, task.task_name, e);

                            if self.options.break_on_failure {
                                println!("\n⚠️  Task failed - entering debug mode");
                                self.debug_task_failure(&task_key, &e.to_string());
                                return Err(e);
                            }
                        }
                    }
                }
            }

            // Show progress
            if self.options.show_progress {
                self.show_progress(&completed, task_graph.tasks.len());
            }
        }

        // Final statistics
        println!("\n");
        println!("{}", self.monitor.get_stats());

        // Export JSON if requested
        if let Some(ref path) = self.options.export_json {
            println!("\n📄 Exporting execution report to {}", path.display());
            self.monitor.export_json(path)?;
        }

        Ok(results)
    }

    fn handle_pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        println!("\n⏸️  Execution paused");
        println!("   Commands: [c]ontinue, [s]tatus, [t]asks, [q]uit");

        // Wait for resume
        loop {
            if !self.paused.load(Ordering::SeqCst) {
                println!("▶️  Resuming execution\n");
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn interactive_prompt(&self, ready_tasks: &[&crate::task_graph::ExecutableTask]) {
        println!("\n  Ready tasks:");
        for task in ready_tasks {
            println!("    - {}:{}", task.recipe_name, task.task_name);
        }

        println!("\n  Continue? [y/n/s(status)] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();

        match input.trim() {
            "n" => {
                *self.control.lock().unwrap() = ExecutionControl::Stop;
            }
            "s" => {
                self.monitor.print_tasks();
            }
            _ => {}
        }
    }

    fn enter_debugger(&self, _task_graph: &TaskGraph, _completed: &HashSet<TaskId>) {
        println!("\n🐛 Debug Mode");
        println!("════════════════════════════════════════");

        loop {
            println!("\nCommands:");
            println!("  [s]tatus  - Show task status");
            println!("  [t]asks   - List all tasks");
            println!("  [p]ending - Show pending tasks");
            println!("  [r]unning - Show running tasks");
            println!("  [f]ailed  - Show failed tasks");
            println!("  [c]ontinue - Exit debugger");
            println!("  [q]uit    - Stop execution");

            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();

            match input.trim() {
                "s" => {
                    println!("{}", self.monitor.get_stats());
                }
                "t" => {
                    self.monitor.print_tasks();
                }
                "p" => {
                    self.show_tasks_by_state(TaskState::Pending);
                }
                "r" => {
                    self.show_tasks_by_state(TaskState::Running);
                }
                "f" => {
                    self.show_tasks_by_state(TaskState::Failed);
                }
                "c" => {
                    break;
                }
                "q" => {
                    *self.control.lock().unwrap() = ExecutionControl::Stop;
                    break;
                }
                _ => {
                    println!("Unknown command");
                }
            }
        }
    }

    fn debug_task_failure(&self, task_key: &str, error: &str) {
        println!("\n🐛 Task Failure Debugger");
        println!("════════════════════════════════════════");
        println!("Task: {}", task_key);
        println!("Error: {}", error);

        if let Some(task) = self.monitor.get_task(task_key) {
            println!("\nTask Details:");
            println!("  Recipe: {}", task.recipe);
            println!("  Task: {}", task.task_name);
            println!("  State: {}", task.state);
            if let Some(duration) = task.duration_ms {
                println!("  Duration: {:.2}s", duration as f64 / 1000.0);
            }
            if let Some(ref msg) = task.error_message {
                println!("  Error: {}", msg);
            }
        }

        println!("\nCommands:");
        println!("  [c]ontinue - Continue execution (will fail)");
        println!("  [r]etry    - Retry task (not implemented)");
        println!("  [s]kip     - Skip task (not implemented)");
        println!("  [q]uit     - Stop execution");

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();

        if input.trim() == "q" {
            *self.control.lock().unwrap() = ExecutionControl::Stop;
        }
    }

    fn show_progress(&self, completed: &HashSet<TaskId>, total: usize) {
        let percentage = (completed.len() as f64 / total as f64) * 100.0;
        let bar_width = 40;
        let filled = (percentage / 100.0 * bar_width as f64) as usize;

        print!("\r  Progress: [");
        print!("{}", "█".repeat(filled));
        print!("{}", "░".repeat(bar_width - filled));
        print!("] {:.1}% ({}/{})", percentage, completed.len(), total);
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    fn show_tasks_by_state(&self, state: TaskState) {
        let tasks: Vec<_> = self
            .monitor
            .get_all_tasks()
            .into_iter()
            .filter(|t| t.state == state)
            .collect();

        println!("\n{} Tasks:", state);
        if tasks.is_empty() {
            println!("  (none)");
        } else {
            for task in tasks {
                println!("  - {}:{}", task.recipe, task.task_name);
            }
        }
    }
}

/// Handle for controlling execution from another thread
pub struct ExecutionControlHandle {
    control: Arc<Mutex<ExecutionControl>>,
    paused: Arc<AtomicBool>,
}

impl ExecutionControlHandle {
    /// Pause execution
    pub fn pause(&self) {
        *self.control.lock().unwrap() = ExecutionControl::Pause;
    }

    /// Resume execution
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        *self.control.lock().unwrap() = ExecutionControl::Continue;
    }

    /// Stop execution
    pub fn stop(&self) {
        *self.control.lock().unwrap() = ExecutionControl::Stop;
    }

    /// Enter debugger
    pub fn debug(&self) {
        *self.control.lock().unwrap() = ExecutionControl::Debug;
    }

    /// Check if paused
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}
