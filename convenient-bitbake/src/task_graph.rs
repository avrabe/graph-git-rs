//! Task graph builder - builds execution DAG from RecipeGraph
//!
//! Converts BitBake recipe dependencies into a concrete task execution graph

use crate::recipe_graph::{RecipeGraph, RecipeId, TaskId};
use std::collections::{HashMap, HashSet, VecDeque};

/// A concrete task that can be executed
#[derive(Debug, Clone)]
pub struct ExecutableTask {
    /// Unique task ID
    pub task_id: TaskId,
    /// Recipe this task belongs to
    pub recipe_id: RecipeId,
    /// Task name (e.g., "do_compile")
    pub task_name: String,
    /// Recipe name (e.g., "glibc")
    pub recipe_name: String,
    /// Direct dependencies (must complete before this task)
    pub depends_on: Vec<TaskId>,
    /// Tasks that depend on this one
    pub dependents: Vec<TaskId>,
}

/// Complete task execution graph
#[derive(Debug, Clone)]
pub struct TaskGraph {
    /// All tasks indexed by TaskId
    pub tasks: HashMap<TaskId, ExecutableTask>,
    /// Topological order (dependencies first)
    pub execution_order: Vec<TaskId>,
    /// Tasks with no dependencies (can start immediately)
    pub root_tasks: Vec<TaskId>,
    /// Tasks with no dependents (final outputs)
    pub leaf_tasks: Vec<TaskId>,
}

impl TaskGraph {
    /// Get a task by ID
    pub fn get_task(&self, task_id: TaskId) -> Option<&ExecutableTask> {
        self.tasks.get(&task_id)
    }

    /// Get all tasks that are ready to execute (dependencies satisfied)
    pub fn get_ready_tasks(&self, completed: &HashSet<TaskId>) -> Vec<TaskId> {
        self.tasks
            .iter()
            .filter(|(task_id, task)| {
                !completed.contains(task_id)
                    && task.depends_on.iter().all(|dep| completed.contains(dep))
            })
            .map(|(task_id, _)| *task_id)
            .collect()
    }

    /// Get tasks by recipe
    pub fn get_recipe_tasks(&self, recipe_id: RecipeId) -> Vec<&ExecutableTask> {
        self.tasks
            .values()
            .filter(|task| task.recipe_id == recipe_id)
            .collect()
    }

    /// Statistics
    pub fn stats(&self) -> TaskGraphStats {
        TaskGraphStats {
            total_tasks: self.tasks.len(),
            root_tasks: self.root_tasks.len(),
            leaf_tasks: self.leaf_tasks.len(),
            max_depth: self.compute_max_depth(),
        }
    }

    fn compute_max_depth(&self) -> usize {
        let mut depths = HashMap::new();

        for &task_id in &self.execution_order {
            let task = &self.tasks[&task_id];
            let max_dep_depth = task
                .depends_on
                .iter()
                .filter_map(|dep| depths.get(dep))
                .max()
                .unwrap_or(&0);
            depths.insert(task_id, max_dep_depth + 1);
        }

        depths.values().max().copied().unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub struct TaskGraphStats {
    pub total_tasks: usize,
    pub root_tasks: usize,
    pub leaf_tasks: usize,
    pub max_depth: usize,
}

/// Builds task execution graph from recipe graph
pub struct TaskGraphBuilder {
    recipe_graph: RecipeGraph,
}

impl TaskGraphBuilder {
    /// Create a new builder
    pub fn new(recipe_graph: RecipeGraph) -> Self {
        Self { recipe_graph }
    }

    /// Build complete task graph for all recipes
    pub fn build_full_graph(&self) -> Result<TaskGraph, String> {
        let mut tasks = HashMap::new();
        let mut task_dependencies: HashMap<TaskId, Vec<TaskId>> = HashMap::new();

        // Phase 1: Create all tasks
        for recipe in self.recipe_graph.recipes() {
            let recipe_tasks = self.recipe_graph.get_recipe_tasks(recipe.id);

            for task_node in recipe_tasks {
                let executable = ExecutableTask {
                    task_id: task_node.id,
                    recipe_id: recipe.id,
                    task_name: task_node.name.clone(),
                    recipe_name: recipe.name.clone(),
                    depends_on: Vec::new(),
                    dependents: Vec::new(),
                };
                tasks.insert(task_node.id, executable);
            }
        }

        // Phase 2: Resolve dependencies
        for recipe in self.recipe_graph.recipes() {
            let recipe_tasks = self.recipe_graph.get_recipe_tasks(recipe.id);

            for task_node in recipe_tasks {
                let mut deps = Vec::new();

                // Intra-recipe dependencies (after/before)
                deps.extend(task_node.after.iter().copied());

                // Inter-recipe task dependencies
                for task_dep in &task_node.task_depends {
                    if let Some(dep_task_id) = task_dep.task_id {
                        deps.push(dep_task_id);
                    } else {
                        // Resolve by name
                        if let Some(resolved_id) = self.recipe_graph.find_task(
                            task_dep.recipe_id,
                            &task_dep.task_name,
                        ) {
                            deps.push(resolved_id);
                        }
                    }
                }

                // Recipe-level dependencies (e.g., DEPENDS)
                // For each dependent recipe, add dependency on do_populate_sysroot
                for &dep_recipe_id in &recipe.depends {
                    if let Some(sysroot_task) =
                        self.recipe_graph.find_task(dep_recipe_id, "do_populate_sysroot")
                    {
                        // Only add if this task needs it (compile/install tasks)
                        if task_node.name.contains("compile")
                            || task_node.name.contains("install")
                            || task_node.name.contains("configure")
                        {
                            deps.push(sysroot_task);
                        }
                    }
                }

                task_dependencies.insert(task_node.id, deps);
            }
        }

        // Phase 3: Apply dependencies and build reverse dependencies
        for (task_id, deps) in task_dependencies {
            if let Some(task) = tasks.get_mut(&task_id) {
                task.depends_on = deps.clone();
            }

            // Add reverse dependencies
            for dep_id in deps {
                if let Some(dep_task) = tasks.get_mut(&dep_id) {
                    dep_task.dependents.push(task_id);
                }
            }
        }

        // Phase 4: Compute topological order
        let execution_order = self.topological_sort(&tasks)?;

        // Phase 5: Find root and leaf tasks
        let root_tasks: Vec<_> = tasks
            .iter()
            .filter(|(_, task)| task.depends_on.is_empty())
            .map(|(id, _)| *id)
            .collect();

        let leaf_tasks: Vec<_> = tasks
            .iter()
            .filter(|(_, task)| task.dependents.is_empty())
            .map(|(id, _)| *id)
            .collect();

        Ok(TaskGraph {
            tasks,
            execution_order,
            root_tasks,
            leaf_tasks,
        })
    }

    /// Build task graph for specific target task
    pub fn build_for_target(
        &self,
        recipe_name: &str,
        task_name: &str,
    ) -> Result<TaskGraph, String> {
        let recipe_id = self
            .recipe_graph
            .find_recipe(recipe_name)
            .ok_or_else(|| format!("Recipe not found: {recipe_name}"))?;

        let task_id = self
            .recipe_graph
            .find_task(recipe_id, task_name)
            .ok_or_else(|| format!("Task not found: {recipe_name}:{task_name}"))?;

        self.build_for_task(task_id)
    }

    /// Build minimal task graph needed to execute a specific task
    pub fn build_for_task(&self, target_task: TaskId) -> Result<TaskGraph, String> {
        let mut required_tasks = HashSet::new();
        self.collect_dependencies(target_task, &mut required_tasks);

        // Build graph with only required tasks
        let full_graph = self.build_full_graph()?;

        let tasks: HashMap<_, _> = full_graph
            .tasks
            .into_iter()
            .filter(|(id, _)| required_tasks.contains(id))
            .collect();

        let execution_order: Vec<_> = full_graph
            .execution_order
            .into_iter()
            .filter(|id| required_tasks.contains(id))
            .collect();

        let root_tasks: Vec<_> = tasks
            .iter()
            .filter(|(_, task)| task.depends_on.iter().all(|d| !required_tasks.contains(d)))
            .map(|(id, _)| *id)
            .collect();

        let leaf_tasks = vec![target_task];

        Ok(TaskGraph {
            tasks,
            execution_order,
            root_tasks,
            leaf_tasks,
        })
    }

    /// Recursively collect all dependencies of a task
    fn collect_dependencies(&self, task_id: TaskId, collected: &mut HashSet<TaskId>) {
        if collected.contains(&task_id) {
            return;
        }

        collected.insert(task_id);

        if let Some(task_node) = self.recipe_graph.get_task(task_id) {
            // Collect intra-recipe dependencies
            for &dep_id in &task_node.after {
                self.collect_dependencies(dep_id, collected);
            }

            // Collect inter-recipe dependencies
            for task_dep in &task_node.task_depends {
                if let Some(dep_id) = task_dep.task_id {
                    self.collect_dependencies(dep_id, collected);
                } else if let Some(resolved_id) =
                    self.recipe_graph
                        .find_task(task_dep.recipe_id, &task_dep.task_name)
                {
                    self.collect_dependencies(resolved_id, collected);
                }
            }

            // Collect recipe-level dependencies
            if let Some(recipe) = self.recipe_graph.get_recipe(task_node.recipe_id) {
                for &dep_recipe_id in &recipe.depends {
                    // Try do_populate_sysroot first (preferred for build deps)
                    if let Some(sysroot_task) =
                        self.recipe_graph.find_task(dep_recipe_id, "do_populate_sysroot")
                    {
                        self.collect_dependencies(sysroot_task, collected);
                    } else if let Some(install_task) =
                        self.recipe_graph.find_task(dep_recipe_id, "do_install")
                    {
                        // Fallback to do_install if do_populate_sysroot doesn't exist
                        self.collect_dependencies(install_task, collected);
                    } else {
                        // Fallback: collect ALL tasks from the dependency recipe
                        if let Some(dep_recipe) = self.recipe_graph.get_recipe(dep_recipe_id) {
                            for &dep_task_id in &dep_recipe.tasks {
                                self.collect_dependencies(dep_task_id, collected);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Topological sort using Kahn's algorithm
    fn topological_sort(
        &self,
        tasks: &HashMap<TaskId, ExecutableTask>,
    ) -> Result<Vec<TaskId>, String> {
        let mut in_degree: HashMap<TaskId, usize> = HashMap::new();
        let mut result = Vec::new();

        // Calculate in-degrees
        for task in tasks.values() {
            in_degree.entry(task.task_id).or_insert(0);
            for &dep in &task.depends_on {
                if tasks.contains_key(&dep) {
                    *in_degree.entry(task.task_id).or_insert(0) += 1;
                }
            }
        }

        // Queue tasks with no dependencies
        let mut queue: VecDeque<_> = in_degree
            .iter()
            .filter(|&(_, &degree)| degree == 0)
            .map(|(id, _)| *id)
            .collect();

        while let Some(task_id) = queue.pop_front() {
            result.push(task_id);

            if let Some(task) = tasks.get(&task_id) {
                for &dependent in &task.dependents {
                    if let Some(degree) = in_degree.get_mut(&dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dependent);
                        }
                    }
                }
            }
        }

        if result.len() != tasks.len() {
            return Err("Circular dependency detected in task graph".to_string());
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe_graph::{Recipe, TaskNode};

    #[test]
    fn test_simple_task_graph() {
        let mut graph = RecipeGraph::new();

        // Create a simple recipe with tasks
        let recipe_id = graph.add_recipe("test-recipe");
        let fetch_id = graph.add_task(recipe_id, "do_fetch");
        let compile_id = graph.add_task(recipe_id, "do_compile");
        let install_id = graph.add_task(recipe_id, "do_install");

        // Set up dependencies: fetch -> compile -> install
        if let Some(compile_task) = graph.get_task_mut(compile_id) {
            compile_task.after.push(fetch_id);
        }
        if let Some(install_task) = graph.get_task_mut(install_id) {
            install_task.after.push(compile_id);
        }

        let builder = TaskGraphBuilder::new(graph);
        let task_graph = builder.build_full_graph().unwrap();

        assert_eq!(task_graph.tasks.len(), 3);
        assert!(task_graph.root_tasks.contains(&fetch_id));
        assert!(task_graph.leaf_tasks.contains(&install_id));

        // Verify execution order
        let fetch_pos = task_graph.execution_order.iter().position(|&id| id == fetch_id);
        let compile_pos = task_graph.execution_order.iter().position(|&id| id == compile_id);
        let install_pos = task_graph.execution_order.iter().position(|&id| id == install_id);

        assert!(fetch_pos < compile_pos);
        assert!(compile_pos < install_pos);
    }

    #[test]
    fn test_target_task_graph() {
        let mut graph = RecipeGraph::new();

        let recipe_id = graph.add_recipe("test");
        let fetch_id = graph.add_task(recipe_id, "do_fetch");
        let compile_id = graph.add_task(recipe_id, "do_compile");
        let install_id = graph.add_task(recipe_id, "do_install");

        if let Some(compile_task) = graph.get_task_mut(compile_id) {
            compile_task.after.push(fetch_id);
        }
        if let Some(install_task) = graph.get_task_mut(install_id) {
            install_task.after.push(compile_id);
        }

        let builder = TaskGraphBuilder::new(graph);
        let task_graph = builder.build_for_task(compile_id).unwrap();

        // Should only include fetch and compile, not install
        assert_eq!(task_graph.tasks.len(), 2);
        assert!(task_graph.tasks.contains_key(&fetch_id));
        assert!(task_graph.tasks.contains_key(&compile_id));
        assert!(!task_graph.tasks.contains_key(&install_id));
    }
}
