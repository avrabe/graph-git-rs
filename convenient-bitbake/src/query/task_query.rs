//! Task graph query engine
//!
//! Executes query expressions against a TaskGraph (Bazel cquery equivalent).

use super::expr::{QueryExpr, TargetPattern};
use crate::task_graph::TaskGraph;
use crate::recipe_graph::TaskId;
use std::collections::{HashMap, HashSet, VecDeque};
use serde::{Serialize, Deserialize};

/// A task target in query results
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TaskTarget {
    /// Layer name (may be unknown)
    pub layer: String,
    /// Recipe name
    pub recipe: String,
    /// Task name
    pub task: String,
    /// Task ID in the graph
    pub task_id: TaskId,
}

/// Query engine for task graphs
pub struct TaskQueryEngine<'a> {
    graph: &'a TaskGraph,
    /// Task specifications (for script/env/outputs queries)
    task_specs: &'a HashMap<String, crate::executor::TaskSpec>,
}

impl<'a> TaskQueryEngine<'a> {
    /// Create a new task query engine
    pub fn new(
        graph: &'a TaskGraph,
        task_specs: &'a HashMap<String, crate::executor::TaskSpec>,
    ) -> Self {
        Self { graph, task_specs }
    }

    /// Execute a query and return matching task targets
    pub fn execute(&self, expr: &QueryExpr) -> Result<Vec<TaskTarget>, String> {
        let mut results = self.execute_expr(expr)?;

        // Sort results for deterministic output
        results.sort();
        results.dedup();

        Ok(results)
    }

    fn execute_expr(&self, expr: &QueryExpr) -> Result<Vec<TaskTarget>, String> {
        match expr {
            QueryExpr::Target(pattern) => self.match_pattern(pattern),

            QueryExpr::Deps { expr, max_depth } => {
                let targets = self.execute_expr(expr)?;
                self.get_dependencies(&targets, *max_depth)
            }

            QueryExpr::ReverseDeps { universe, target } => {
                let universe_targets = self.execute_expr(universe)?;
                let target_set: HashSet<TaskTarget> =
                    self.execute_expr(target)?.into_iter().collect();
                self.get_reverse_dependencies(&universe_targets, &target_set)
            }

            QueryExpr::SomePath { from, to } => {
                let from_targets = self.execute_expr(from)?;
                let to_targets = self.execute_expr(to)?;
                self.find_some_path(&from_targets, &to_targets)
            }

            QueryExpr::AllPaths { from, to } => {
                let from_targets = self.execute_expr(from)?;
                let to_targets = self.execute_expr(to)?;
                self.find_all_paths(&from_targets, &to_targets)
            }

            QueryExpr::Kind { pattern, expr } => {
                let targets = self.execute_expr(expr)?;
                Ok(targets
                    .into_iter()
                    .filter(|t| self.matches_kind(t, pattern))
                    .collect())
            }

            QueryExpr::Filter { pattern, expr } => {
                let targets = self.execute_expr(expr)?;
                Ok(targets
                    .into_iter()
                    .filter(|t| self.matches_filter(t, pattern))
                    .collect())
            }

            QueryExpr::Attr { name, value, expr } => {
                let targets = self.execute_expr(expr)?;
                Ok(targets
                    .into_iter()
                    .filter(|t| self.matches_attr(t, name, value))
                    .collect())
            }

            QueryExpr::Intersect(a, b) => {
                let a_results: HashSet<TaskTarget> =
                    self.execute_expr(a)?.into_iter().collect();
                let b_results: HashSet<TaskTarget> =
                    self.execute_expr(b)?.into_iter().collect();
                Ok(a_results.intersection(&b_results).cloned().collect())
            }

            QueryExpr::Union(a, b) => {
                let mut results = self.execute_expr(a)?;
                results.extend(self.execute_expr(b)?);
                Ok(results)
            }

            QueryExpr::Except(a, b) => {
                let a_results: HashSet<TaskTarget> =
                    self.execute_expr(a)?.into_iter().collect();
                let b_results: HashSet<TaskTarget> =
                    self.execute_expr(b)?.into_iter().collect();
                Ok(a_results.difference(&b_results).cloned().collect())
            }

            // Task-specific queries
            QueryExpr::Script(expr) => self.execute_expr(expr),
            QueryExpr::Inputs(expr) => self.execute_expr(expr),
            QueryExpr::Outputs(expr) => self.execute_expr(expr),
            QueryExpr::Env(expr) => self.execute_expr(expr),
            QueryExpr::CriticalPath(expr) => {
                let targets = self.execute_expr(expr)?;
                self.compute_critical_path(&targets)
            }
        }
    }

    fn match_pattern(&self, pattern: &TargetPattern) -> Result<Vec<TaskTarget>, String> {
        let mut results = Vec::new();

        for (task_id, task) in &self.graph.tasks {
            // Parse recipe name to extract layer (if present)
            let (layer, recipe_name) = if task.recipe_name.contains(':') {
                let parts: Vec<&str> = task.recipe_name.split(':').collect();
                (parts[0], parts.get(1).copied().unwrap_or(&task.recipe_name))
            } else {
                ("unknown", task.recipe_name.as_str())
            };

            if pattern.matches_task(layer, recipe_name, &task.task_name) {
                results.push(TaskTarget {
                    layer: layer.to_string(),
                    recipe: recipe_name.to_string(),
                    task: task.task_name.clone(),
                    task_id: *task_id,
                });
            }
        }

        Ok(results)
    }

    fn get_dependencies(
        &self,
        targets: &[TaskTarget],
        max_depth: Option<usize>,
    ) -> Result<Vec<TaskTarget>, String> {
        let mut results = HashSet::new();
        let mut queue = VecDeque::new();

        // Initialize queue with starting targets
        for target in targets {
            queue.push_back((target.task_id, 0));
            results.insert(target.clone());
        }

        while let Some((task_id, depth)) = queue.pop_front() {
            if let Some(max) = max_depth
                && depth >= max {
                    continue;
                }

            if let Some(task) = self.graph.tasks.get(&task_id) {
                for &dep_id in &task.depends_on {
                    if let Some(dep_task) = self.graph.tasks.get(&dep_id) {
                        let (layer, recipe_name) = if dep_task.recipe_name.contains(':') {
                            let parts: Vec<&str> = dep_task.recipe_name.split(':').collect();
                            (parts[0], parts.get(1).copied().unwrap_or(&dep_task.recipe_name))
                        } else {
                            ("unknown", dep_task.recipe_name.as_str())
                        };

                        let dep_target = TaskTarget {
                            layer: layer.to_string(),
                            recipe: recipe_name.to_string(),
                            task: dep_task.task_name.clone(),
                            task_id: dep_id,
                        };

                        if results.insert(dep_target) {
                            queue.push_back((dep_id, depth + 1));
                        }
                    }
                }
            }
        }

        Ok(results.into_iter().collect())
    }

    fn get_reverse_dependencies(
        &self,
        universe: &[TaskTarget],
        targets: &HashSet<TaskTarget>,
    ) -> Result<Vec<TaskTarget>, String> {
        let target_ids: HashSet<TaskId> = targets.iter().map(|t| t.task_id).collect();
        let mut results = HashSet::new();

        for universe_target in universe {
            if self.depends_on_any(&universe_target.task_id, &target_ids) {
                results.insert(universe_target.clone());
            }
        }

        Ok(results.into_iter().collect())
    }

    fn depends_on_any(&self, task_id: &TaskId, targets: &HashSet<TaskId>) -> bool {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(*task_id);

        while let Some(current_id) = queue.pop_front() {
            if !visited.insert(current_id) {
                continue;
            }

            if targets.contains(&current_id) {
                return true;
            }

            if let Some(task) = self.graph.tasks.get(&current_id) {
                for &dep_id in &task.depends_on {
                    queue.push_back(dep_id);
                }
            }
        }

        false
    }

    fn find_some_path(
        &self,
        from_targets: &[TaskTarget],
        to_targets: &[TaskTarget],
    ) -> Result<Vec<TaskTarget>, String> {
        let to_ids: HashSet<TaskId> = to_targets.iter().map(|t| t.task_id).collect();

        for from_target in from_targets {
            if let Some(path) = self.find_path_to_any(&from_target.task_id, &to_ids) {
                return Ok(path);
            }
        }

        Ok(Vec::new())
    }

    fn find_path_to_any(
        &self,
        start: &TaskId,
        targets: &HashSet<TaskId>,
    ) -> Option<Vec<TaskTarget>> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut parent: HashMap<TaskId, TaskId> = HashMap::new();

        queue.push_back(*start);

        while let Some(current_id) = queue.pop_front() {
            if !visited.insert(current_id) {
                continue;
            }

            if targets.contains(&current_id) {
                // Reconstruct path
                let mut path = Vec::new();
                let mut current = current_id;

                loop {
                    if let Some(task) = self.graph.tasks.get(&current) {
                        let (layer, recipe_name) = if task.recipe_name.contains(':') {
                            let parts: Vec<&str> = task.recipe_name.split(':').collect();
                            (parts[0], parts.get(1).copied().unwrap_or(&task.recipe_name))
                        } else {
                            ("unknown", task.recipe_name.as_str())
                        };

                        path.push(TaskTarget {
                            layer: layer.to_string(),
                            recipe: recipe_name.to_string(),
                            task: task.task_name.clone(),
                            task_id: current,
                        });
                    }

                    if let Some(&p) = parent.get(&current) {
                        current = p;
                    } else {
                        break;
                    }
                }

                path.reverse();
                return Some(path);
            }

            if let Some(task) = self.graph.tasks.get(&current_id) {
                for &dep_id in &task.depends_on {
                    if !visited.contains(&dep_id) {
                        parent.insert(dep_id, current_id);
                        queue.push_back(dep_id);
                    }
                }
            }
        }

        None
    }

    fn find_all_paths(
        &self,
        _from_targets: &[TaskTarget],
        _to_targets: &[TaskTarget],
    ) -> Result<Vec<TaskTarget>, String> {
        // TODO: Implement all paths (can be exponential, need careful handling)
        Err("allpaths() not yet implemented for task queries".to_string())
    }

    fn matches_kind(&self, target: &TaskTarget, pattern: &str) -> bool {
        let task_key = format!("{}:{}", target.recipe, target.task);
        if let Some(spec) = self.task_specs.get(&task_key) {
            let mode_str = format!("{:?}", spec.execution_mode);
            mode_str.contains(pattern)
        } else {
            false
        }
    }

    fn matches_filter(&self, target: &TaskTarget, pattern: &str) -> bool {
        let task_full_name = format!("{}:{}:{}", target.layer, target.recipe, target.task);
        // Simple glob-style matching
        if pattern.starts_with('*') && pattern.ends_with('*') {
            let inner = &pattern[1..pattern.len() - 1];
            task_full_name.contains(inner)
        } else if pattern.starts_with('*') {
            task_full_name.ends_with(&pattern[1..])
        } else if pattern.ends_with('*') {
            task_full_name.starts_with(&pattern[..pattern.len() - 1])
        } else {
            task_full_name.contains(pattern)
        }
    }

    fn matches_attr(&self, target: &TaskTarget, name: &str, value: &str) -> bool {
        let task_key = format!("{}:{}", target.recipe, target.task);
        if let Some(spec) = self.task_specs.get(&task_key) {
            match name {
                "network" | "network_policy" => {
                    format!("{:?}", spec.network_policy).contains(value)
                }
                "execution_mode" | "mode" => format!("{:?}", spec.execution_mode).contains(value),
                _ => false,
            }
        } else {
            false
        }
    }

    fn compute_critical_path(&self, targets: &[TaskTarget]) -> Result<Vec<TaskTarget>, String> {
        // TODO: Implement critical path algorithm
        // For now, just return the targets
        Ok(targets.to_vec())
    }
}
