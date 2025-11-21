// Task dependency parsing for BitBake recipes
// Handles addtask statements and task flags

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Represents a BitBake task (e.g., do_compile, do_install)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Task name (e.g., "do_compile")
    pub name: String,
    /// Tasks this task must run after
    pub after: Vec<String>,
    /// Tasks this task must run before
    pub before: Vec<String>,
    /// Task flags (e.g., depends, rdepends, nostamp)
    pub flags: HashMap<String, String>,
}

impl Task {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            after: Vec::new(),
            before: Vec::new(),
            flags: HashMap::new(),
        }
    }

    pub fn with_after(mut self, after: Vec<String>) -> Self {
        self.after = after;
        self
    }

    pub fn with_before(mut self, before: Vec<String>) -> Self {
        self.before = before;
        self
    }

    pub fn with_flag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.flags.insert(key.into(), value.into());
        self
    }
}

/// Dependency types for tasks
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskDependency {
    /// Build-time dependency (DEPENDS)
    /// Format: "recipe:task"
    BuildTime { recipe: String, task: String },
    /// Runtime dependency (RDEPENDS)
    /// Format: "recipe:task"
    Runtime { recipe: String, task: String },
    /// Recipe-level dependency (just recipe name)
    Recipe(String),
}

impl TaskDependency {
    /// Parse a dependency string like "virtual/libc:do_populate_sysroot"
    pub fn parse_depends(dep_str: &str) -> Vec<Self> {
        dep_str
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(|item| {
                if let Some((recipe, task)) = item.split_once(':') {
                    TaskDependency::BuildTime {
                        recipe: recipe.to_string(),
                        task: task.to_string(),
                    }
                } else {
                    TaskDependency::Recipe(item.to_string())
                }
            })
            .collect()
    }

    /// Parse runtime dependencies
    pub fn parse_rdepends(dep_str: &str) -> Vec<Self> {
        dep_str
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(|item| {
                if let Some((recipe, task)) = item.split_once(':') {
                    TaskDependency::Runtime {
                        recipe: recipe.to_string(),
                        task: task.to_string(),
                    }
                } else {
                    TaskDependency::Recipe(item.to_string())
                }
            })
            .collect()
    }
}

/// Collection of tasks for a recipe
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskCollection {
    /// All tasks defined in the recipe
    pub tasks: HashMap<String, Task>,
    /// Standard task order (fetch -> unpack -> patch -> configure -> compile -> install)
    pub task_order: Vec<String>,
}

impl TaskCollection {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            task_order: Vec::new(),
        }
    }

    /// Add a task to the collection
    pub fn add_task(&mut self, task: Task) {
        self.tasks.insert(task.name.clone(), task);
    }

    /// Get a task by name
    pub fn get_task(&self, name: &str) -> Option<&Task> {
        self.tasks.get(name)
    }

    /// Get all task names
    pub fn task_names(&self) -> Vec<&String> {
        self.tasks.keys().collect()
    }

    /// Build topological order of tasks based on after/before constraints
    pub fn compute_task_order(&mut self) -> Result<(), String> {
        let mut order = Vec::new();
        let mut visited = HashSet::new();
        let mut visiting = HashSet::new();

        fn visit(
            task_name: &str,
            tasks: &HashMap<String, Task>,
            visited: &mut HashSet<String>,
            visiting: &mut HashSet<String>,
            order: &mut Vec<String>,
        ) -> Result<(), String> {
            if visited.contains(task_name) {
                return Ok(());
            }
            if visiting.contains(task_name) {
                return Err(format!("Circular dependency detected involving task: {task_name}"));
            }

            visiting.insert(task_name.to_string());

            if let Some(task) = tasks.get(task_name) {
                // Visit all tasks that must come before this one
                for after_task in &task.after {
                    visit(after_task, tasks, visited, visiting, order)?;
                }
            }

            visiting.remove(task_name);
            visited.insert(task_name.to_string());
            order.push(task_name.to_string());

            Ok(())
        }

        // Visit all tasks
        for task_name in self.tasks.keys() {
            visit(task_name, &self.tasks, &mut visited, &mut visiting, &mut order)?;
        }

        self.task_order = order;
        Ok(())
    }

    /// Get build-time dependencies for a task
    pub fn get_build_dependencies(&self, task_name: &str) -> Vec<TaskDependency> {
        if let Some(task) = self.get_task(task_name)
            && let Some(depends) = task.flags.get("depends") {
                return TaskDependency::parse_depends(depends);
            }
        Vec::new()
    }

    /// Get runtime dependencies for a task
    pub fn get_runtime_dependencies(&self, task_name: &str) -> Vec<TaskDependency> {
        if let Some(task) = self.get_task(task_name)
            && let Some(rdepends) = task.flags.get("rdepends") {
                return TaskDependency::parse_rdepends(rdepends);
            }
        Vec::new()
    }
}

/// Normalize task name by removing "do_" prefix if present
/// BitBake canonical task names don't have the "do_" prefix
/// (e.g., "install" not "do_install", "compile" not "do_compile")
fn normalize_task_name(name: &str) -> String {
    name.strip_prefix("do_").unwrap_or(name).to_string()
}

/// Parse addtask statement
/// Format: addtask TASK [after TASK1 TASK2] [before TASK3 TASK4]
pub fn parse_addtask_statement(line: &str) -> Option<Task> {
    let line = line.trim();

    // Must start with "addtask"
    if !line.starts_with("addtask") {
        return None;
    }

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    // Normalize task name (strip "do_" prefix if present)
    let task_name = normalize_task_name(parts[1]);
    let mut task = Task::new(task_name);

    let mut i = 2;
    while i < parts.len() {
        match parts[i] {
            "after" => {
                i += 1;
                let mut after_tasks = Vec::new();
                while i < parts.len() && parts[i] != "before" {
                    // Normalize task names in dependencies (strip "do_" prefix)
                    after_tasks.push(normalize_task_name(parts[i]));
                    i += 1;
                }
                task.after = after_tasks;
            }
            "before" => {
                i += 1;
                let mut before_tasks = Vec::new();
                while i < parts.len() && parts[i] != "after" {
                    // Normalize task names in dependencies (strip "do_" prefix)
                    before_tasks.push(normalize_task_name(parts[i]));
                    i += 1;
                }
                task.before = before_tasks;
            }
            _ => {
                i += 1;
            }
        }
    }

    Some(task)
}

/// Parse deltask statement
/// Format: deltask TASK
/// Returns the task name to be removed (normalized without "do_" prefix)
pub fn parse_deltask_statement(line: &str) -> Option<String> {
    let line = line.trim();

    // Must start with "deltask"
    if !line.starts_with("deltask") {
        return None;
    }

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    // Normalize task name (strip "do_" prefix if present)
    Some(normalize_task_name(parts[1]))
}

/// Parse task flag assignment
/// Format: do_task[flag] = "value"
pub fn parse_task_flag(line: &str) -> Option<(String, String, String)> {
    let line = line.trim();

    // Look for pattern: task_name[flag_name] = "value"
    if let Some((left, right)) = line.split_once('=') {
        let left = left.trim();
        let right = right.trim().trim_matches('"').trim_matches('\'');

        if let Some(bracket_start) = left.find('[')
            && let Some(bracket_end) = left.find(']') {
                // Normalize task name (strip "do_" prefix if present)
                let task_name = normalize_task_name(left[..bracket_start].trim());
                let flag_name = left[bracket_start + 1..bracket_end].trim().to_string();
                return Some((task_name, flag_name, right.to_string()));
            }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_addtask_simple() {
        let line = "addtask compile";
        let task = parse_addtask_statement(line).unwrap();
        assert_eq!(task.name, "compile");
        assert!(task.after.is_empty());
        assert!(task.before.is_empty());
    }

    #[test]
    fn test_parse_addtask_with_after() {
        let line = "addtask compile after configure";
        let task = parse_addtask_statement(line).unwrap();
        assert_eq!(task.name, "compile");
        assert_eq!(task.after, vec!["configure"]);
        assert!(task.before.is_empty());
    }

    #[test]
    fn test_parse_addtask_with_before() {
        let line = "addtask compile before install";
        let task = parse_addtask_statement(line).unwrap();
        assert_eq!(task.name, "compile");
        assert!(task.after.is_empty());
        assert_eq!(task.before, vec!["install"]);
    }

    #[test]
    fn test_parse_addtask_full() {
        let line = "addtask compile after configure before install";
        let task = parse_addtask_statement(line).unwrap();
        assert_eq!(task.name, "compile");
        assert_eq!(task.after, vec!["configure"]);
        assert_eq!(task.before, vec!["install"]);
    }

    #[test]
    fn test_parse_addtask_multiple_constraints() {
        let line = "addtask compile after configure patch before install package";
        let task = parse_addtask_statement(line).unwrap();
        assert_eq!(task.name, "compile");
        assert_eq!(task.after, vec!["configure", "patch"]);
        assert_eq!(task.before, vec!["install", "package"]);
    }

    #[test]
    fn test_parse_deltask() {
        let line = "deltask do_install_ptest";
        let task_name = parse_deltask_statement(line).unwrap();
        assert_eq!(task_name, "install_ptest");
    }

    #[test]
    fn test_parse_deltask_with_extra_whitespace() {
        let line = "  deltask   do_configure  ";
        let task_name = parse_deltask_statement(line).unwrap();
        assert_eq!(task_name, "configure");
    }

    #[test]
    fn test_parse_deltask_invalid() {
        let line = "addtask do_fetch";
        let result = parse_deltask_statement(line);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_task_flag() {
        let line = "do_compile[depends] = \"virtual/libc:do_populate_sysroot\"";
        let (task, flag, value) = parse_task_flag(line).unwrap();
        assert_eq!(task, "compile");
        assert_eq!(flag, "depends");
        assert_eq!(value, "virtual/libc:do_populate_sysroot");
    }

    #[test]
    fn test_parse_task_flag_multiple_deps() {
        let line = "do_compile[depends] = \"glibc:do_populate_sysroot openssl:do_populate_sysroot\"";
        let (task, flag, value) = parse_task_flag(line).unwrap();
        assert_eq!(task, "compile");
        assert_eq!(flag, "depends");
        assert_eq!(value, "glibc:do_populate_sysroot openssl:do_populate_sysroot");
    }

    #[test]
    fn test_parse_dependencies() {
        let deps = TaskDependency::parse_depends("virtual/libc:do_populate_sysroot openssl:do_compile zlib");
        assert_eq!(deps.len(), 3);

        match &deps[0] {
            TaskDependency::BuildTime { recipe, task } => {
                assert_eq!(recipe, "virtual/libc");
                assert_eq!(task, "do_populate_sysroot");
            }
            _ => panic!("Expected BuildTime dependency"),
        }

        match &deps[2] {
            TaskDependency::Recipe(name) => assert_eq!(name, "zlib"),
            _ => panic!("Expected Recipe dependency"),
        }
    }

    #[test]
    fn test_task_collection_add_and_get() {
        let mut collection = TaskCollection::new();
        let task = Task::new("do_compile");
        collection.add_task(task.clone());

        assert_eq!(collection.get_task("do_compile"), Some(&task));
        assert_eq!(collection.task_names().len(), 1);
    }

    #[test]
    fn test_task_order_computation() {
        let mut collection = TaskCollection::new();

        collection.add_task(Task::new("do_fetch"));
        collection.add_task(Task::new("do_unpack").with_after(vec!["do_fetch".to_string()]));
        collection.add_task(Task::new("do_compile").with_after(vec!["do_unpack".to_string()]));

        collection.compute_task_order().unwrap();

        assert_eq!(collection.task_order.len(), 3);
        let fetch_idx = collection.task_order.iter().position(|t| t == "do_fetch").unwrap();
        let unpack_idx = collection.task_order.iter().position(|t| t == "do_unpack").unwrap();
        let compile_idx = collection.task_order.iter().position(|t| t == "do_compile").unwrap();

        assert!(fetch_idx < unpack_idx);
        assert!(unpack_idx < compile_idx);
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut collection = TaskCollection::new();

        collection.add_task(Task::new("task_a").with_after(vec!["task_b".to_string()]));
        collection.add_task(Task::new("task_b").with_after(vec!["task_a".to_string()]));

        let result = collection.compute_task_order();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Circular dependency"));
    }
}
