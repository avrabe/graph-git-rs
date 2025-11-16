//! Recipe graph query engine
//!
//! Executes query expressions against a RecipeGraph.

use super::expr::{QueryExpr, TargetPattern};
use crate::recipe_graph::{RecipeGraph, RecipeId};
use std::collections::{HashMap, HashSet, VecDeque};
use serde::{Serialize, Deserialize};

/// Query engine for recipe graphs
pub struct RecipeQueryEngine<'a> {
    graph: &'a RecipeGraph,
}

impl<'a> RecipeQueryEngine<'a> {
    /// Create a new query engine
    pub fn new(graph: &'a RecipeGraph) -> Self {
        Self { graph }
    }

    /// Execute a query and return matching recipe targets
    pub fn execute(&self, expr: &QueryExpr) -> Result<Vec<RecipeTarget>, String> {
        let mut results = self.execute_expr(expr)?;

        // Sort results for deterministic output
        results.sort();
        results.dedup();

        Ok(results)
    }

    fn execute_expr(&self, expr: &QueryExpr) -> Result<Vec<RecipeTarget>, String> {
        match expr {
            QueryExpr::Target(pattern) => self.match_pattern(pattern),

            QueryExpr::Deps { expr, max_depth } => {
                let targets = self.execute_expr(expr)?;
                self.get_dependencies(&targets, *max_depth)
            }

            QueryExpr::ReverseDeps { universe, target } => {
                let universe_targets = self.execute_expr(universe)?;
                let target_set: HashSet<RecipeTarget> = self.execute_expr(target)?.into_iter().collect();
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
                let a_results: HashSet<RecipeTarget> = self.execute_expr(a)?.into_iter().collect();
                let b_results: HashSet<RecipeTarget> = self.execute_expr(b)?.into_iter().collect();
                Ok(a_results.intersection(&b_results).cloned().collect())
            }

            QueryExpr::Union(a, b) => {
                let mut results = self.execute_expr(a)?;
                results.extend(self.execute_expr(b)?);
                Ok(results)
            }

            QueryExpr::Except(a, b) => {
                let a_results: HashSet<RecipeTarget> = self.execute_expr(a)?.into_iter().collect();
                let b_results: HashSet<RecipeTarget> = self.execute_expr(b)?.into_iter().collect();
                Ok(a_results.difference(&b_results).cloned().collect())
            }
        }
    }

    fn match_pattern(&self, pattern: &TargetPattern) -> Result<Vec<RecipeTarget>, String> {
        let mut results = Vec::new();

        for recipe in self.graph.recipes() {
            // Extract layer from recipe name if it contains ":"
            // Otherwise use "unknown" as layer
            let (layer, recipe_name) = if recipe.name.contains(':') {
                let parts: Vec<&str> = recipe.name.split(':').collect();
                (parts[0], parts.get(1).copied().unwrap_or(&recipe.name))
            } else {
                ("unknown", recipe.name.as_str())
            };

            if pattern.matches_recipe(layer, recipe_name) {
                results.push(RecipeTarget {
                    layer: layer.to_string(),
                    recipe: recipe_name.to_string(),
                    recipe_id: recipe.id,
                });
            }
        }

        Ok(results)
    }

    fn get_dependencies(
        &self,
        targets: &[RecipeTarget],
        max_depth: Option<usize>,
    ) -> Result<Vec<RecipeTarget>, String> {
        let mut results = HashSet::new();
        let mut queue = VecDeque::new();

        // Initialize queue with starting targets
        for target in targets {
            queue.push_back((target.recipe_id, 0));
            results.insert(target.clone());
        }

        while let Some((recipe_id, depth)) = queue.pop_front() {
            if let Some(max) = max_depth {
                if depth >= max {
                    continue;
                }
            }

            let deps = self.graph.get_dependencies(recipe_id);
            for dep_id in deps {
                if let Some(recipe) = self.graph.get_recipe(dep_id) {
                    let (layer, recipe_name) = if recipe.name.contains(':') {
                        let parts: Vec<&str> = recipe.name.split(':').collect();
                        (parts[0], parts.get(1).copied().unwrap_or(&recipe.name))
                    } else {
                        ("unknown", recipe.name.as_str())
                    };

                    let target = RecipeTarget {
                        layer: layer.to_string(),
                        recipe: recipe_name.to_string(),
                        recipe_id: dep_id,
                    };

                    if results.insert(target.clone()) {
                        queue.push_back((dep_id, depth + 1));
                    }
                }
            }
        }

        Ok(results.into_iter().collect())
    }

    fn get_reverse_dependencies(
        &self,
        universe: &[RecipeTarget],
        targets: &HashSet<RecipeTarget>,
    ) -> Result<Vec<RecipeTarget>, String> {
        let mut results = HashSet::new();
        let target_ids: HashSet<RecipeId> = targets.iter().map(|t| t.recipe_id).collect();

        // Build reverse dependency map from universe
        let mut rdeps: HashMap<RecipeId, Vec<RecipeId>> = HashMap::new();
        for target in universe {
            let deps = self.graph.get_dependencies(target.recipe_id);
            for dep_id in deps {
                rdeps
                    .entry(dep_id)
                    .or_default()
                    .push(target.recipe_id);
            }
        }

        // Find all recipes that depend on targets (transitively)
        let mut queue: VecDeque<RecipeId> = target_ids.into_iter().collect();

        while let Some(recipe_id) = queue.pop_front() {
            if let Some(dependents) = rdeps.get(&recipe_id) {
                for &dependent_id in dependents {
                    if let Some(recipe) = self.graph.get_recipe(dependent_id) {
                        let (layer, recipe_name) = if recipe.name.contains(':') {
                            let parts: Vec<&str> = recipe.name.split(':').collect();
                            (parts[0], parts.get(1).copied().unwrap_or(&recipe.name))
                        } else {
                            ("unknown", recipe.name.as_str())
                        };

                        let target = RecipeTarget {
                            layer: layer.to_string(),
                            recipe: recipe_name.to_string(),
                            recipe_id: dependent_id,
                        };

                        if results.insert(target.clone()) {
                            queue.push_back(dependent_id);
                        }
                    }
                }
            }
        }

        Ok(results.into_iter().collect())
    }

    fn find_some_path(
        &self,
        from_targets: &[RecipeTarget],
        to_targets: &[RecipeTarget],
    ) -> Result<Vec<RecipeTarget>, String> {
        let to_set: HashSet<RecipeId> = to_targets.iter().map(|t| t.recipe_id).collect();

        // BFS to find a path
        for from_target in from_targets {
            let mut queue = VecDeque::new();
            let mut visited = HashSet::new();
            let mut parent: HashMap<RecipeId, RecipeId> = HashMap::new();

            let start_id = from_target.recipe_id;
            queue.push_back(start_id);
            visited.insert(start_id);

            while let Some(current_id) = queue.pop_front() {
                if to_set.contains(&current_id) {
                    // Found a path, reconstruct it
                    let mut path = Vec::new();
                    let mut node = current_id;

                    loop {
                        if let Some(recipe) = self.graph.get_recipe(node) {
                            let (layer, recipe_name) = if recipe.name.contains(':') {
                                let parts: Vec<&str> = recipe.name.split(':').collect();
                                (parts[0], parts.get(1).copied().unwrap_or(&recipe.name))
                            } else {
                                ("unknown", recipe.name.as_str())
                            };

                            path.push(RecipeTarget {
                                layer: layer.to_string(),
                                recipe: recipe_name.to_string(),
                                recipe_id: node,
                            });
                        }

                        if node == start_id {
                            break;
                        }

                        if let Some(&p) = parent.get(&node) {
                            node = p;
                        } else {
                            break;
                        }
                    }

                    path.reverse();
                    return Ok(path);
                }

                let deps = self.graph.get_dependencies(current_id);
                for dep_id in deps {
                    if visited.insert(dep_id) {
                        parent.insert(dep_id, current_id);
                        queue.push_back(dep_id);
                    }
                }
            }
        }

        Ok(Vec::new()) // No path found
    }

    fn find_all_paths(
        &self,
        from_targets: &[RecipeTarget],
        to_targets: &[RecipeTarget],
    ) -> Result<Vec<RecipeTarget>, String> {
        // Return all nodes on any path (union of all path nodes)
        let to_set: HashSet<RecipeId> = to_targets.iter().map(|t| t.recipe_id).collect();
        let mut all_path_nodes = HashSet::new();

        for from_target in from_targets {
            self.find_all_paths_dfs(
                from_target.recipe_id,
                &to_set,
                &mut HashSet::new(),
                &mut all_path_nodes,
            );
        }

        // Convert RecipeIds to RecipeTargets
        let mut results = Vec::new();
        for recipe_id in all_path_nodes {
            if let Some(recipe) = self.graph.get_recipe(recipe_id) {
                let (layer, recipe_name) = if recipe.name.contains(':') {
                    let parts: Vec<&str> = recipe.name.split(':').collect();
                    (parts[0], parts.get(1).copied().unwrap_or(&recipe.name))
                } else {
                    ("unknown", recipe.name.as_str())
                };

                results.push(RecipeTarget {
                    layer: layer.to_string(),
                    recipe: recipe_name.to_string(),
                    recipe_id,
                });
            }
        }

        Ok(results)
    }

    fn find_all_paths_dfs(
        &self,
        current: RecipeId,
        targets: &HashSet<RecipeId>,
        visited: &mut HashSet<RecipeId>,
        path_nodes: &mut HashSet<RecipeId>,
    ) -> bool {
        if targets.contains(&current) {
            path_nodes.insert(current);
            return true;
        }

        visited.insert(current);

        let mut found_path = false;
        let deps = self.graph.get_dependencies(current);
        for dep_id in deps {
            if !visited.contains(&dep_id) {
                if self.find_all_paths_dfs(dep_id, targets, visited, path_nodes) {
                    found_path = true;
                    path_nodes.insert(current);
                }
            }
        }

        visited.remove(&current);
        found_path
    }

    fn matches_kind(&self, target: &RecipeTarget, pattern: &str) -> bool {
        // Get recipe from graph
        let recipe = match self.graph.get_recipe(target.recipe_id) {
            Some(r) => r,
            None => return false,
        };

        // Detect recipe kind from name patterns and metadata
        let recipe_name = &recipe.name;
        let detected_kind = if recipe_name.ends_with("-native") {
            "native"
        } else if recipe_name.ends_with("-cross") || recipe_name.ends_with("-crosssdk") {
            "cross"
        } else if recipe_name.ends_with("-nativesdk") {
            "nativesdk"
        } else if recipe_name.starts_with("packagegroup-") {
            "packagegroup"
        } else if recipe_name.ends_with("-image") || recipe.metadata.get("IMAGE_INSTALL").is_some() {
            "image"
        } else if let Some(inherit) = recipe.metadata.get("INHERIT") {
            // Check for image class
            if inherit.contains("image") {
                "image"
            } else {
                "recipe"
            }
        } else {
            "recipe"
        };

        // Check if pattern matches detected kind
        wildcard_match(pattern, detected_kind)
    }

    fn matches_filter(&self, target: &RecipeTarget, pattern: &str) -> bool {
        // Simple wildcard matching
        let target_str = format!("{}:{}", target.layer, target.recipe);
        wildcard_match(pattern, &target_str)
    }

    fn matches_attr(&self, target: &RecipeTarget, name: &str, value: &str) -> bool {
        // Get recipe from graph
        let recipe = match self.graph.get_recipe(target.recipe_id) {
            Some(r) => r,
            None => return false,
        };

        // Check if recipe has the attribute and it matches the value pattern
        recipe.metadata.get(name)
            .map(|attr_value| wildcard_match(value, attr_value))
            .unwrap_or(false)
    }
}

/// A recipe target (layer:recipe)
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecipeTarget {
    pub layer: String,
    pub recipe: String,
    #[serde(skip_serializing, skip_deserializing, default = "default_recipe_id")]
    pub recipe_id: RecipeId,
}

fn default_recipe_id() -> RecipeId {
    RecipeId(0)
}

impl std::fmt::Display for RecipeTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.layer, self.recipe)
    }
}

/// Simple wildcard matching (* for any sequence)
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('*').collect();

    if pattern_parts.len() == 1 {
        // No wildcards, exact match
        return pattern == text;
    }

    let mut text_pos = 0;

    for (i, part) in pattern_parts.iter().enumerate() {
        if i == 0 {
            // First part must match start
            if !text.starts_with(part) {
                return false;
            }
            text_pos = part.len();
        } else if i == pattern_parts.len() - 1 {
            // Last part must match end
            if !text.ends_with(part) {
                return false;
            }
        } else {
            // Middle parts must appear in order
            if let Some(pos) = text[text_pos..].find(part) {
                text_pos += pos + part.len();
            } else {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe_graph::RecipeGraph;

    #[test]
    fn test_wildcard_match() {
        assert!(wildcard_match("meta-*", "meta-core"));
        assert!(wildcard_match("*core", "meta-core"));
        assert!(wildcard_match("meta-*-yocto", "meta-poky-yocto"));
        assert!(!wildcard_match("meta-*", "poky"));
    }

    #[test]
    fn test_kind_matching_native() {
        let mut graph = RecipeGraph::new();
        let recipe_id = graph.add_recipe("cmake-native");
        let recipe = graph.get_recipe_mut(recipe_id).unwrap();
        recipe.layer = Some("meta-core".to_string());

        let engine = RecipeQueryEngine::new(&graph);
        let target = RecipeTarget {
            recipe_id,
            layer: "meta-core".to_string(),
            recipe: "cmake-native".to_string(),
        };

        assert!(engine.matches_kind(&target, "native"));
        assert!(!engine.matches_kind(&target, "cross"));
        assert!(!engine.matches_kind(&target, "image"));
    }

    #[test]
    fn test_kind_matching_image() {
        let mut graph = RecipeGraph::new();
        let recipe_id = graph.add_recipe("core-image-minimal");
        let recipe = graph.get_recipe_mut(recipe_id).unwrap();
        recipe.layer = Some("meta-core".to_string());
        recipe.metadata.insert("IMAGE_INSTALL".to_string(), "busybox".to_string());

        let engine = RecipeQueryEngine::new(&graph);
        let target = RecipeTarget {
            recipe_id,
            layer: "meta-core".to_string(),
            recipe: "core-image-minimal".to_string(),
        };

        assert!(engine.matches_kind(&target, "image"));
        assert!(!engine.matches_kind(&target, "native"));
    }

    #[test]
    fn test_kind_matching_cross() {
        let mut graph = RecipeGraph::new();
        let recipe_id = graph.add_recipe("gcc-cross");
        let recipe = graph.get_recipe_mut(recipe_id).unwrap();
        recipe.layer = Some("meta-core".to_string());

        let engine = RecipeQueryEngine::new(&graph);
        let target = RecipeTarget {
            recipe_id,
            layer: "meta-core".to_string(),
            recipe: "gcc-cross".to_string(),
        };

        assert!(engine.matches_kind(&target, "cross"));
        assert!(!engine.matches_kind(&target, "native"));
    }

    #[test]
    fn test_attr_matching() {
        let mut graph = RecipeGraph::new();
        let recipe_id = graph.add_recipe("busybox");
        let recipe = graph.get_recipe_mut(recipe_id).unwrap();
        recipe.layer = Some("meta-core".to_string());
        recipe.metadata.insert("LICENSE".to_string(), "GPLv2".to_string());
        recipe.metadata.insert("SECTION".to_string(), "base".to_string());

        let engine = RecipeQueryEngine::new(&graph);
        let target = RecipeTarget {
            recipe_id,
            layer: "meta-core".to_string(),
            recipe: "busybox".to_string(),
        };

        assert!(engine.matches_attr(&target, "LICENSE", "GPLv2"));
        assert!(engine.matches_attr(&target, "SECTION", "base"));
        assert!(!engine.matches_attr(&target, "LICENSE", "MIT"));
        assert!(!engine.matches_attr(&target, "NONEXISTENT", "value"));
    }

    #[test]
    fn test_attr_matching_wildcard() {
        let mut graph = RecipeGraph::new();
        let recipe_id = graph.add_recipe("linux-yocto");
        let recipe = graph.get_recipe_mut(recipe_id).unwrap();
        recipe.layer = Some("meta-yocto".to_string());
        recipe.metadata.insert("LICENSE".to_string(), "GPLv2".to_string());

        let engine = RecipeQueryEngine::new(&graph);
        let target = RecipeTarget {
            recipe_id,
            layer: "meta-yocto".to_string(),
            recipe: "linux-yocto".to_string(),
        };

        assert!(engine.matches_attr(&target, "LICENSE", "GPL*"));
        assert!(!engine.matches_attr(&target, "LICENSE", "MIT*"));
    }
}
