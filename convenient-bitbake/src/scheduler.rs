// ! Task scheduler with priority queue and critical path analysis

use crate::recipe_graph::{RecipeGraph, RecipeId, TaskId};
use std::collections::{HashMap, HashSet, BinaryHeap};
use std::cmp::Ordering;

/// Task priority for scheduling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskPriority {
    /// Critical path length (higher = more critical)
    pub critical_path_length: u32,

    /// Number of dependent tasks (higher = more blockers)
    pub dependent_count: u32,

    /// Estimated execution time (ms)
    pub estimated_time_ms: u64,
}

impl Ord for TaskPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority = should run first
        // 1. Critical path length (longest first)
        // 2. Number of dependents (most blockers first)
        // 3. Estimated time (longest first - start heavy tasks early)
        self.critical_path_length
            .cmp(&other.critical_path_length)
            .then(self.dependent_count.cmp(&other.dependent_count))
            .then(self.estimated_time_ms.cmp(&other.estimated_time_ms))
    }
}

impl PartialOrd for TaskPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Scheduled task with priority
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTask {
    pub task_id: TaskId,
    pub recipe_id: RecipeId,
    pub priority: TaskPriority,
}

impl Ord for ScheduledTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl PartialOrd for ScheduledTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Task scheduler with critical path analysis
pub struct TaskScheduler {
    /// Recipe graph
    graph: RecipeGraph,

    /// Task priority cache
    priorities: HashMap<TaskId, TaskPriority>,

    /// Completed tasks
    completed: HashSet<TaskId>,

    /// Running tasks
    running: HashSet<TaskId>,

    /// Ready queue (priority queue)
    ready_queue: BinaryHeap<ScheduledTask>,
}

impl TaskScheduler {
    /// Create a new task scheduler
    pub fn new(graph: RecipeGraph) -> Self {
        Self {
            graph,
            priorities: HashMap::new(),
            completed: HashSet::new(),
            running: HashSet::new(),
            ready_queue: BinaryHeap::new(),
        }
    }

    /// Initialize the scheduler with the task graph
    pub fn initialize(&mut self) {
        // First analyze critical paths to compute priorities
        self.analyze_critical_paths();

        // Then populate the ready queue with initial tasks (no dependencies)
        self.update_ready_queue();
    }

    /// Analyze critical paths for all tasks
    pub fn analyze_critical_paths(&mut self) {
        // Build task dependency graph
        let task_deps = self.build_task_dependencies();

        // Compute critical path lengths using topological sort + dynamic programming
        let mut critical_lengths: HashMap<TaskId, u32> = HashMap::new();
        let visited: HashSet<TaskId> = HashSet::new();

        // Process tasks in reverse topological order (leaves first)
        let sorted_tasks = self.topological_sort(&task_deps);

        for task_id in sorted_tasks.iter().rev() {
            let deps = task_deps.get(task_id).cloned().unwrap_or_default();

            if deps.is_empty() {
                // Leaf task - critical path length is 1
                critical_lengths.insert(*task_id, 1);
            } else {
                // Critical path = 1 + max(critical_paths of dependencies)
                let max_dep_length = deps
                    .iter()
                    .map(|dep| critical_lengths.get(dep).copied().unwrap_or(0))
                    .max()
                    .unwrap_or(0);
                critical_lengths.insert(*task_id, max_dep_length + 1);
            }
        }

        // Count dependents for each task
        let mut dependent_counts: HashMap<TaskId, u32> = HashMap::new();
        for (task_id, deps) in &task_deps {
            for dep in deps {
                *dependent_counts.entry(*dep).or_insert(0) += 1;
            }
        }

        // Cache priorities
        for task_id in sorted_tasks {
            let critical_path_length = critical_lengths.get(&task_id).copied().unwrap_or(0);
            let dependent_count = dependent_counts.get(&task_id).copied().unwrap_or(0);

            self.priorities.insert(
                task_id,
                TaskPriority {
                    critical_path_length,
                    dependent_count,
                    estimated_time_ms: 1000,  // TODO: Get from task metadata
                },
            );
        }
    }

    /// Build task dependency map from the recipe graph
    fn build_task_dependencies(&self) -> HashMap<TaskId, Vec<TaskId>> {
        let mut deps_map = HashMap::new();

        // Extract dependencies from all tasks in the graph
        for recipe in self.graph.recipes() {
            let tasks = self.graph.get_recipe_tasks(recipe.id);

            for task in tasks {
                // Collect all dependencies for this task
                let mut task_deps = Vec::new();

                // Add intra-recipe dependencies (after relationships)
                task_deps.extend(task.after.iter().copied());

                // Add inter-recipe task dependencies
                for task_dep in &task.task_depends {
                    if let Some(dep_id) = task_dep.task_id {
                        task_deps.push(dep_id);
                    } else {
                        // Resolve by name
                        if let Some(resolved_id) = self.graph.find_task(
                            task_dep.recipe_id,
                            &task_dep.task_name,
                        ) {
                            task_deps.push(resolved_id);
                        }
                    }
                }

                // Add recipe-level dependencies (e.g., DEPENDS)
                for &dep_recipe_id in &recipe.depends {
                    // Add dependency on do_populate_sysroot of dependent recipes
                    if let Some(sysroot_task) = self.graph.find_task(dep_recipe_id, "do_populate_sysroot") {
                        // Only add if this task needs it (compile/install tasks)
                        if task.name.contains("compile")
                            || task.name.contains("install")
                            || task.name.contains("configure")
                        {
                            task_deps.push(sysroot_task);
                        }
                    }
                }

                deps_map.insert(task.id, task_deps);
            }
        }

        deps_map
    }

    /// Topological sort of tasks
    fn topological_sort(&self, task_deps: &HashMap<TaskId, Vec<TaskId>>) -> Vec<TaskId> {
        let mut sorted = Vec::new();
        let mut visited = HashSet::new();
        let mut temp_mark = HashSet::new();

        fn visit(
            task: TaskId,
            deps: &HashMap<TaskId, Vec<TaskId>>,
            visited: &mut HashSet<TaskId>,
            temp_mark: &mut HashSet<TaskId>,
            sorted: &mut Vec<TaskId>,
        ) {
            if visited.contains(&task) {
                return;
            }
            if temp_mark.contains(&task) {
                // Cycle detected
                return;
            }

            temp_mark.insert(task);

            if let Some(task_deps) = deps.get(&task) {
                for dep in task_deps {
                    visit(*dep, deps, visited, temp_mark, sorted);
                }
            }

            temp_mark.remove(&task);
            visited.insert(task);
            sorted.push(task);
        }

        for task_id in task_deps.keys() {
            visit(*task_id, task_deps, &mut visited, &mut temp_mark, &mut sorted);
        }

        sorted
    }

    /// Get next ready tasks to execute (up to limit)
    pub fn get_ready_tasks(&mut self, limit: usize) -> Vec<ScheduledTask> {
        let mut ready = Vec::new();

        while ready.len() < limit {
            if let Some(task) = self.ready_queue.pop() {
                // Check if task is still eligible
                if !self.completed.contains(&task.task_id)
                    && !self.running.contains(&task.task_id) {
                    ready.push(task);
                }
            } else {
                break;
            }
        }

        // Mark as running
        for task in &ready {
            self.running.insert(task.task_id);
        }

        ready
    }

    /// Mark task as started
    pub fn mark_running(&mut self, task_id: TaskId) {
        self.running.insert(task_id);
    }

    /// Mark task as completed
    pub fn mark_completed(&mut self, task_id: TaskId) {
        self.running.remove(&task_id);
        self.completed.insert(task_id);

        // Update ready queue - tasks that were blocked by this one may now be ready
        self.update_ready_queue();
    }

    /// Mark task as failed
    pub fn mark_failed(&mut self, task_id: TaskId) {
        self.running.remove(&task_id);
        // Don't add to completed - failed tasks don't unblock dependents
    }

    /// Update ready queue with newly available tasks
    fn update_ready_queue(&mut self) {
        // Build task dependency map if not cached
        let task_deps = self.build_task_dependencies();

        // Find all tasks that are ready (all dependencies completed)
        for recipe in self.graph.recipes() {
            let tasks = self.graph.get_recipe_tasks(recipe.id);

            for task in tasks {
                let task_id = task.id;

                // Skip if already completed, running, or in queue
                if self.completed.contains(&task_id) || self.running.contains(&task_id) {
                    continue;
                }

                // Check if already in queue
                let in_queue = self.ready_queue.iter().any(|st| st.task_id == task_id);
                if in_queue {
                    continue;
                }

                // Check if all dependencies are completed
                let deps = task_deps.get(&task_id).cloned().unwrap_or_default();
                let all_deps_complete = deps.iter().all(|dep| self.completed.contains(dep));

                if all_deps_complete {
                    // Get priority for this task
                    if let Some(&priority) = self.priorities.get(&task_id) {
                        let scheduled_task = ScheduledTask {
                            task_id,
                            recipe_id: task.recipe_id,
                            priority,
                        };
                        self.ready_queue.push(scheduled_task);
                    }
                }
            }
        }
    }

    /// Get scheduling statistics
    pub fn get_stats(&self) -> SchedulerStats {
        SchedulerStats {
            total_tasks: self.priorities.len(),
            completed: self.completed.len(),
            running: self.running.len(),
            ready: self.ready_queue.len(),
            pending: self.priorities.len() - self.completed.len() - self.running.len(),
        }
    }

    /// Get critical path tasks
    pub fn get_critical_path(&self) -> Vec<TaskId> {
        let mut tasks: Vec<_> = self.priorities
            .iter()
            .map(|(id, priority)| (*id, priority.critical_path_length))
            .collect();

        tasks.sort_by(|a, b| b.1.cmp(&a.1));

        tasks.into_iter()
            .take(10)  // Top 10 critical tasks
            .map(|(id, _)| id)
            .collect()
    }

    /// Estimate total build time on the critical path
    pub fn estimate_critical_path_time(&self) -> u64 {
        self.get_critical_path()
            .iter()
            .filter_map(|id| self.priorities.get(id))
            .map(|p| p.estimated_time_ms)
            .sum()
    }

    /// Get parallelism opportunity (tasks that can run in parallel)
    pub fn get_parallelism_level(&self) -> usize {
        // Count tasks with no incomplete dependencies
        // This is the maximum parallelism available
        self.ready_queue.len()
    }
}

/// Scheduler statistics
#[derive(Debug, Clone)]
pub struct SchedulerStats {
    pub total_tasks: usize,
    pub completed: usize,
    pub running: usize,
    pub ready: usize,
    pub pending: usize,
}

impl SchedulerStats {
    pub fn completion_percent(&self) -> f64 {
        if self.total_tasks == 0 {
            0.0
        } else {
            (self.completed as f64 / self.total_tasks as f64) * 100.0
        }
    }

    pub fn parallelism_utilization(&self, max_parallel: usize) -> f64 {
        if max_parallel == 0 {
            0.0
        } else {
            (self.running as f64 / max_parallel as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_priority_ordering() {
        let p1 = TaskPriority {
            critical_path_length: 10,
            dependent_count: 5,
            estimated_time_ms: 1000,
        };

        let p2 = TaskPriority {
            critical_path_length: 5,
            dependent_count: 5,
            estimated_time_ms: 1000,
        };

        assert!(p1 > p2);  // Longer critical path = higher priority
    }

    #[test]
    fn test_scheduler_stats() {
        let stats = SchedulerStats {
            total_tasks: 100,
            completed: 50,
            running: 10,
            ready: 20,
            pending: 20,
        };

        assert_eq!(stats.completion_percent(), 50.0);
        assert_eq!(stats.parallelism_utilization(20), 50.0);
    }

    #[test]
    fn test_priority_queue_ordering() {
        let mut queue = BinaryHeap::new();

        let task1 = ScheduledTask {
            task_id: TaskId(1),
            recipe_id: RecipeId(1),
            priority: TaskPriority {
                critical_path_length: 5,
                dependent_count: 2,
                estimated_time_ms: 1000,
            },
        };

        let task2 = ScheduledTask {
            task_id: TaskId(2),
            recipe_id: RecipeId(1),
            priority: TaskPriority {
                critical_path_length: 10,
                dependent_count: 5,
                estimated_time_ms: 2000,
            },
        };

        queue.push(task1.clone());
        queue.push(task2.clone());

        // Higher priority (task2) should come first
        assert_eq!(queue.pop().unwrap().task_id, TaskId(2));
        assert_eq!(queue.pop().unwrap().task_id, TaskId(1));
    }
}
