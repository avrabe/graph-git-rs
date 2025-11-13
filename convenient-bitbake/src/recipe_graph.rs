// Recipe dependency graph with flat structure and ID-based references
// Follows modern compiler IR design (rustc, LLVM, rust-analyzer)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// === ID Types (cheap to copy, use as keys) ===

/// Unique identifier for a recipe
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct RecipeId(pub u32);

impl RecipeId {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// Unique identifier for a task
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct TaskId(pub u32);

impl TaskId {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

// === Core Data Structures (flat, ID-based) ===

/// A BitBake recipe in the dependency graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub id: RecipeId,
    pub name: String,
    pub version: Option<String>,

    // Build-time dependencies (recipe IDs)
    pub depends: Vec<RecipeId>,

    // Runtime dependencies (recipe IDs)
    pub rdepends: Vec<RecipeId>,

    // What this recipe provides (for virtual provider resolution)
    pub provides: Vec<String>,
    pub rprovides: Vec<String>,

    // Tasks in this recipe (task IDs)
    pub tasks: Vec<TaskId>,

    // File metadata
    pub file_path: Option<PathBuf>,
    pub layer: Option<String>,

    // Arbitrary metadata (from variables)
    pub metadata: HashMap<String, String>,
}

impl Recipe {
    pub fn new(id: RecipeId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            version: None,
            depends: Vec::new(),
            rdepends: Vec::new(),
            provides: Vec::new(),
            rprovides: Vec::new(),
            tasks: Vec::new(),
            file_path: None,
            layer: None,
            metadata: HashMap::new(),
        }
    }

    /// Get the full package name with version (e.g., "linux-yocto-5.15")
    pub fn full_name(&self) -> String {
        if let Some(version) = &self.version {
            format!("{}-{}", self.name, version)
        } else {
            self.name.clone()
        }
    }

    /// Check if this recipe provides a given capability
    pub fn provides_capability(&self, capability: &str) -> bool {
        self.provides.iter().any(|p| p == capability)
            || self.rprovides.iter().any(|p| p == capability)
            || self.name == capability
    }
}

/// A task in the dependency graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: TaskId,
    pub recipe_id: RecipeId,
    pub name: String,

    // Task ordering constraints (within same recipe)
    pub after: Vec<TaskId>,
    pub before: Vec<TaskId>,

    // Cross-recipe task dependencies
    pub task_depends: Vec<TaskDependency>,

    // Task flags
    pub flags: HashMap<String, String>,
}

impl TaskNode {
    pub fn new(id: TaskId, recipe_id: RecipeId, name: impl Into<String>) -> Self {
        Self {
            id,
            recipe_id,
            name: name.into(),
            after: Vec::new(),
            before: Vec::new(),
            task_depends: Vec::new(),
            flags: HashMap::new(),
        }
    }
}

/// Cross-recipe task dependency
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskDependency {
    pub recipe_id: RecipeId,
    pub task_id: Option<TaskId>,  // None means any task with same name
    pub task_name: String,         // For resolution
}

impl TaskDependency {
    pub fn new(recipe_id: RecipeId, task_name: impl Into<String>) -> Self {
        Self {
            recipe_id,
            task_id: None,
            task_name: task_name.into(),
        }
    }

    pub fn with_task_id(mut self, task_id: TaskId) -> Self {
        self.task_id = Some(task_id);
        self
    }
}

// === Main Graph Structure ===

/// The complete recipe dependency graph
/// Uses flat structure with ID-based references for efficiency and flexibility
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecipeGraph {
    // Primary storage (arena-like)
    recipes: HashMap<RecipeId, Recipe>,
    tasks: HashMap<TaskId, TaskNode>,

    // Indices for fast lookup
    name_to_recipe: HashMap<String, RecipeId>,
    provider_map: HashMap<String, Vec<RecipeId>>,  // virtual provider -> recipes
    recipe_to_tasks: HashMap<RecipeId, Vec<TaskId>>,

    // ID generators
    next_recipe_id: u32,
    next_task_id: u32,
}

impl RecipeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    // === Recipe Management ===

    /// Add a new recipe to the graph
    pub fn add_recipe(&mut self, name: impl Into<String>) -> RecipeId {
        let name = name.into();
        let id = RecipeId(self.next_recipe_id);
        self.next_recipe_id += 1;

        let recipe = Recipe::new(id, name.clone());
        self.recipes.insert(id, recipe);
        self.name_to_recipe.insert(name, id);

        id
    }

    /// Get a recipe by ID
    pub fn get_recipe(&self, id: RecipeId) -> Option<&Recipe> {
        self.recipes.get(&id)
    }

    /// Get a mutable reference to a recipe
    pub fn get_recipe_mut(&mut self, id: RecipeId) -> Option<&mut Recipe> {
        self.recipes.get_mut(&id)
    }

    /// Find a recipe by name
    pub fn find_recipe(&self, name: &str) -> Option<RecipeId> {
        self.name_to_recipe.get(name).copied()
    }

    /// Get all recipes
    pub fn recipes(&self) -> impl Iterator<Item = &Recipe> {
        self.recipes.values()
    }

    /// Get recipe count
    pub fn recipe_count(&self) -> usize {
        self.recipes.len()
    }

    // === Task Management ===

    /// Add a task to a recipe
    pub fn add_task(&mut self, recipe_id: RecipeId, name: impl Into<String>) -> TaskId {
        let id = TaskId(self.next_task_id);
        self.next_task_id += 1;

        let task = TaskNode::new(id, recipe_id, name);
        self.tasks.insert(id, task);

        // Update recipe's task list
        if let Some(recipe) = self.recipes.get_mut(&recipe_id) {
            recipe.tasks.push(id);
        }

        // Update index
        self.recipe_to_tasks.entry(recipe_id).or_default().push(id);

        id
    }

    /// Get a task by ID
    pub fn get_task(&self, id: TaskId) -> Option<&TaskNode> {
        self.tasks.get(&id)
    }

    /// Get a mutable reference to a task
    pub fn get_task_mut(&mut self, id: TaskId) -> Option<&mut TaskNode> {
        self.tasks.get_mut(&id)
    }

    /// Get all tasks for a recipe
    pub fn get_recipe_tasks(&self, recipe_id: RecipeId) -> Vec<&TaskNode> {
        self.recipe_to_tasks
            .get(&recipe_id)
            .map(|task_ids| {
                task_ids.iter()
                    .filter_map(|&id| self.tasks.get(&id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find a task by recipe and task name
    pub fn find_task(&self, recipe_id: RecipeId, task_name: &str) -> Option<TaskId> {
        self.recipe_to_tasks.get(&recipe_id).and_then(|tasks| {
            tasks.iter()
                .find(|&&task_id| {
                    self.tasks.get(&task_id)
                        .map(|t| t.name == task_name)
                        .unwrap_or(false)
                })
                .copied()
        })
    }

    /// Get task count
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    // === Dependency Management ===

    /// Add a build-time dependency between recipes
    pub fn add_dependency(&mut self, from: RecipeId, to: RecipeId) {
        if let Some(recipe) = self.recipes.get_mut(&from)
            && !recipe.depends.contains(&to) {
                recipe.depends.push(to);
            }
    }

    /// Add a runtime dependency between recipes
    pub fn add_runtime_dependency(&mut self, from: RecipeId, to: RecipeId) {
        if let Some(recipe) = self.recipes.get_mut(&from)
            && !recipe.rdepends.contains(&to) {
                recipe.rdepends.push(to);
            }
    }

    /// Add a task dependency
    pub fn add_task_dependency(&mut self, from: TaskId, dep: TaskDependency) {
        if let Some(task) = self.tasks.get_mut(&from)
            && !task.task_depends.contains(&dep) {
                task.task_depends.push(dep);
            }
    }

    // === Provider Resolution ===

    /// Register a recipe as providing a capability
    pub fn register_provider(&mut self, recipe_id: RecipeId, capability: impl Into<String>) {
        let capability = capability.into();
        self.provider_map.entry(capability).or_default().push(recipe_id);
    }

    /// Resolve a provider (e.g., "virtual/kernel" -> linux-yocto)
    pub fn resolve_provider(&self, capability: &str) -> Option<RecipeId> {
        // First check direct name match
        if let Some(id) = self.name_to_recipe.get(capability) {
            return Some(*id);
        }

        // Then check provider map
        self.provider_map.get(capability)
            .and_then(|providers| providers.first())
            .copied()
    }

    /// Get all recipes that provide a capability
    pub fn get_providers(&self, capability: &str) -> Vec<RecipeId> {
        self.provider_map.get(capability)
            .cloned()
            .unwrap_or_default()
    }

    // === Graph Traversal ===

    /// Get direct dependencies of a recipe
    pub fn get_dependencies(&self, recipe_id: RecipeId) -> Vec<RecipeId> {
        self.recipes.get(&recipe_id)
            .map(|r| r.depends.clone())
            .unwrap_or_default()
    }

    /// Get all transitive dependencies (recursive)
    pub fn get_all_dependencies(&self, recipe_id: RecipeId) -> Vec<RecipeId> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        self.collect_dependencies_recursive(recipe_id, &mut visited, &mut result);
        result
    }

    fn collect_dependencies_recursive(
        &self,
        recipe_id: RecipeId,
        visited: &mut HashSet<RecipeId>,
        result: &mut Vec<RecipeId>,
    ) {
        if visited.contains(&recipe_id) {
            return;
        }
        visited.insert(recipe_id);

        if let Some(recipe) = self.recipes.get(&recipe_id) {
            for &dep_id in &recipe.depends {
                if !visited.contains(&dep_id) {
                    result.push(dep_id);
                    self.collect_dependencies_recursive(dep_id, visited, result);
                }
            }
        }
    }

    /// Get recipes that depend on this recipe (reverse lookup)
    pub fn get_dependents(&self, recipe_id: RecipeId) -> Vec<RecipeId> {
        self.recipes.values()
            .filter(|r| r.depends.contains(&recipe_id))
            .map(|r| r.id)
            .collect()
    }

    /// Detect circular dependencies
    pub fn detect_cycles(&self) -> Vec<Vec<RecipeId>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for &recipe_id in self.recipes.keys() {
            if !visited.contains(&recipe_id) {
                self.detect_cycle_dfs(
                    recipe_id,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        cycles
    }

    fn detect_cycle_dfs(
        &self,
        recipe_id: RecipeId,
        visited: &mut HashSet<RecipeId>,
        rec_stack: &mut HashSet<RecipeId>,
        path: &mut Vec<RecipeId>,
        cycles: &mut Vec<Vec<RecipeId>>,
    ) {
        visited.insert(recipe_id);
        rec_stack.insert(recipe_id);
        path.push(recipe_id);

        if let Some(recipe) = self.recipes.get(&recipe_id) {
            for &dep_id in &recipe.depends {
                if !visited.contains(&dep_id) {
                    self.detect_cycle_dfs(dep_id, visited, rec_stack, path, cycles);
                } else if rec_stack.contains(&dep_id) {
                    // Found a cycle
                    if let Some(cycle_start) = path.iter().position(|&id| id == dep_id) {
                        cycles.push(path[cycle_start..].to_vec());
                    }
                }
            }
        }

        path.pop();
        rec_stack.remove(&recipe_id);
    }

    /// Compute topological sort of all recipes (build order: dependencies first)
    pub fn topological_sort(&self) -> Result<Vec<RecipeId>, String> {
        let mut sorted = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for &recipe_id in self.recipes.keys() {
            if !visited.contains(&recipe_id) {
                self.topo_sort_visit(recipe_id, &mut visited, &mut rec_stack, &mut sorted)?;
            }
        }

        // Result is in post-order (dependencies first)
        Ok(sorted)
    }

    fn topo_sort_visit(
        &self,
        recipe_id: RecipeId,
        visited: &mut HashSet<RecipeId>,
        rec_stack: &mut HashSet<RecipeId>,
        sorted: &mut Vec<RecipeId>,
    ) -> Result<(), String> {
        if rec_stack.contains(&recipe_id) {
            return Err(format!("Circular dependency detected at recipe: {:?}", recipe_id));
        }
        if visited.contains(&recipe_id) {
            return Ok(());
        }

        visited.insert(recipe_id);
        rec_stack.insert(recipe_id);

        if let Some(recipe) = self.recipes.get(&recipe_id) {
            for &dep_id in &recipe.depends {
                self.topo_sort_visit(dep_id, visited, rec_stack, sorted)?;
            }
        }

        rec_stack.remove(&recipe_id);
        sorted.push(recipe_id);
        Ok(())
    }

    // === Export/Analysis ===

    /// Export graph in Graphviz DOT format
    pub fn to_dot(&self) -> String {
        let mut dot = String::from("digraph recipes {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [shape=box];\n\n");

        // Nodes
        for recipe in self.recipes.values() {
            let label = recipe.full_name();
            dot.push_str(&format!("  \"{}\" [label=\"{}\"];\n", recipe.id.0, label));
        }

        dot.push('\n');

        // Edges
        for recipe in self.recipes.values() {
            for &dep_id in &recipe.depends {
                dot.push_str(&format!("  \"{}\" -> \"{}\";\n", recipe.id.0, dep_id.0));
            }
        }

        dot.push_str("}\n");
        dot
    }

    /// Get graph statistics
    pub fn statistics(&self) -> GraphStatistics {
        let total_dependencies: usize = self.recipes.values()
            .map(|r| r.depends.len())
            .sum();

        let max_depth = self.recipes.keys()
            .map(|&id| self.get_all_dependencies(id).len())
            .max()
            .unwrap_or(0);

        GraphStatistics {
            recipe_count: self.recipes.len(),
            task_count: self.tasks.len(),
            total_dependencies,
            provider_count: self.provider_map.len(),
            max_dependency_depth: max_depth,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStatistics {
    pub recipe_count: usize,
    pub task_count: usize,
    pub total_dependencies: usize,
    pub provider_count: usize,
    pub max_dependency_depth: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_graph() {
        let graph = RecipeGraph::new();
        assert_eq!(graph.recipe_count(), 0);
        assert_eq!(graph.task_count(), 0);
    }

    #[test]
    fn test_add_recipe() {
        let mut graph = RecipeGraph::new();
        let id = graph.add_recipe("glibc");

        assert_eq!(graph.recipe_count(), 1);
        let recipe = graph.get_recipe(id).unwrap();
        assert_eq!(recipe.name, "glibc");
        assert_eq!(recipe.id, id);
    }

    #[test]
    fn test_find_recipe() {
        let mut graph = RecipeGraph::new();
        let id = graph.add_recipe("openssl");

        let found_id = graph.find_recipe("openssl");
        assert_eq!(found_id, Some(id));
        assert_eq!(graph.find_recipe("nonexistent"), None);
    }

    #[test]
    fn test_add_task() {
        let mut graph = RecipeGraph::new();
        let recipe_id = graph.add_recipe("linux-yocto");
        let task_id = graph.add_task(recipe_id, "do_compile");

        assert_eq!(graph.task_count(), 1);
        let task = graph.get_task(task_id).unwrap();
        assert_eq!(task.name, "do_compile");
        assert_eq!(task.recipe_id, recipe_id);

        // Check recipe has the task
        let recipe = graph.get_recipe(recipe_id).unwrap();
        assert!(recipe.tasks.contains(&task_id));
    }

    #[test]
    fn test_add_dependency() {
        let mut graph = RecipeGraph::new();
        let glibc = graph.add_recipe("glibc");
        let openssl = graph.add_recipe("openssl");

        graph.add_dependency(openssl, glibc);

        let deps = graph.get_dependencies(openssl);
        assert_eq!(deps, vec![glibc]);
    }

    #[test]
    fn test_provider_resolution() {
        let mut graph = RecipeGraph::new();
        let linux_yocto = graph.add_recipe("linux-yocto");
        graph.register_provider(linux_yocto, "virtual/kernel");

        let resolved = graph.resolve_provider("virtual/kernel");
        assert_eq!(resolved, Some(linux_yocto));
    }

    #[test]
    fn test_direct_name_resolution() {
        let mut graph = RecipeGraph::new();
        let glibc = graph.add_recipe("glibc");

        // Should resolve by name even without explicit provider registration
        let resolved = graph.resolve_provider("glibc");
        assert_eq!(resolved, Some(glibc));
    }

    #[test]
    fn test_get_all_dependencies() {
        let mut graph = RecipeGraph::new();
        let a = graph.add_recipe("a");
        let b = graph.add_recipe("b");
        let c = graph.add_recipe("c");
        let d = graph.add_recipe("d");

        // a -> b -> c
        //   -> d
        graph.add_dependency(a, b);
        graph.add_dependency(a, d);
        graph.add_dependency(b, c);

        let all_deps = graph.get_all_dependencies(a);
        assert_eq!(all_deps.len(), 3);
        assert!(all_deps.contains(&b));
        assert!(all_deps.contains(&c));
        assert!(all_deps.contains(&d));
    }

    #[test]
    fn test_get_dependents() {
        let mut graph = RecipeGraph::new();
        let glibc = graph.add_recipe("glibc");
        let openssl = graph.add_recipe("openssl");
        let bash = graph.add_recipe("bash");

        graph.add_dependency(openssl, glibc);
        graph.add_dependency(bash, glibc);

        let dependents = graph.get_dependents(glibc);
        assert_eq!(dependents.len(), 2);
        assert!(dependents.contains(&openssl));
        assert!(dependents.contains(&bash));
    }

    #[test]
    fn test_topological_sort() {
        let mut graph = RecipeGraph::new();
        let a = graph.add_recipe("a");
        let b = graph.add_recipe("b");
        let c = graph.add_recipe("c");

        // a -> b -> c
        graph.add_dependency(a, b);
        graph.add_dependency(b, c);

        let sorted = graph.topological_sort().unwrap();

        let pos_a = sorted.iter().position(|&id| id == a).unwrap();
        let pos_b = sorted.iter().position(|&id| id == b).unwrap();
        let pos_c = sorted.iter().position(|&id| id == c).unwrap();

        assert!(pos_c < pos_b);
        assert!(pos_b < pos_a);
    }

    #[test]
    fn test_detect_cycles() {
        let mut graph = RecipeGraph::new();
        let a = graph.add_recipe("a");
        let b = graph.add_recipe("b");

        // Create cycle: a -> b -> a
        graph.add_dependency(a, b);
        graph.add_dependency(b, a);

        let cycles = graph.detect_cycles();
        assert!(!cycles.is_empty());
    }

    #[test]
    fn test_statistics() {
        let mut graph = RecipeGraph::new();
        let a = graph.add_recipe("a");
        let b = graph.add_recipe("b");
        graph.add_task(a, "do_compile");
        graph.add_dependency(a, b);
        graph.register_provider(a, "virtual/a");

        let stats = graph.statistics();
        assert_eq!(stats.recipe_count, 2);
        assert_eq!(stats.task_count, 1);
        assert_eq!(stats.total_dependencies, 1);
        assert_eq!(stats.provider_count, 1);
    }

    #[test]
    fn test_recipe_full_name() {
        let mut graph = RecipeGraph::new();
        let id = graph.add_recipe("linux-yocto");
        let recipe = graph.get_recipe_mut(id).unwrap();
        recipe.version = Some("5.15".to_string());

        let recipe = graph.get_recipe(id).unwrap();
        assert_eq!(recipe.full_name(), "linux-yocto-5.15");
    }

    #[test]
    fn test_find_task() {
        let mut graph = RecipeGraph::new();
        let recipe_id = graph.add_recipe("test");
        let compile_id = graph.add_task(recipe_id, "do_compile");

        let found = graph.find_task(recipe_id, "do_compile");
        assert_eq!(found, Some(compile_id));

        let not_found = graph.find_task(recipe_id, "do_nonexistent");
        assert_eq!(not_found, None);
    }
}
