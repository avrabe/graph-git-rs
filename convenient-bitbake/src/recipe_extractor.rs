// Recipe dependency extractor - populates RecipeGraph from BitBake files
// Combines parsing, variable resolution, and graph construction

use crate::recipe_graph::{RecipeGraph, RecipeId, TaskId};
use crate::task_parser::{parse_addtask_statement, parse_task_flag};
use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};

#[cfg(feature = "python-execution")]
use crate::python_executor::PythonExecutor;

/// Configuration for recipe extraction
#[derive(Debug, Clone, Default)]
pub struct ExtractionConfig {
    /// Use Python executor for variable resolution (requires python-execution feature)
    pub use_python_executor: bool,
    /// Extract task dependencies
    pub extract_tasks: bool,
    /// Resolve virtual providers
    pub resolve_providers: bool,
}

/// Result of extracting a single recipe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeExtraction {
    pub recipe_id: RecipeId,
    pub name: String,
    pub depends: Vec<String>,
    pub rdepends: Vec<String>,
    pub provides: Vec<String>,
    pub rprovides: Vec<String>,
    pub tasks: Vec<String>,
    pub variables: HashMap<String, String>,
}

/// Extracts dependencies from BitBake recipe content
pub struct RecipeExtractor {
    config: ExtractionConfig,
    #[cfg(feature = "python-execution")]
    executor: Option<PythonExecutor>,
}

impl RecipeExtractor {
    pub fn new(config: ExtractionConfig) -> Self {
        #[cfg(feature = "python-execution")]
        let executor = if config.use_python_executor {
            Some(PythonExecutor::new())
        } else {
            None
        };

        Self {
            config,
            #[cfg(feature = "python-execution")]
            executor,
        }
    }

    pub fn new_default() -> Self {
        Self::new(ExtractionConfig::default())
    }

    /// Extract recipe metadata from file content and add to graph
    pub fn extract_from_content(
        &self,
        graph: &mut RecipeGraph,
        recipe_name: impl Into<String>,
        content: &str,
    ) -> Result<RecipeExtraction, String> {
        let recipe_name = recipe_name.into();
        let recipe_id = graph.add_recipe(&recipe_name);

        // Parse variables from content
        let variables = self.parse_variables(content);

        // Extract dependencies
        let depends = self.extract_dependency_list(&variables, "DEPENDS");
        let rdepends = self.extract_dependency_list(&variables, "RDEPENDS");
        let provides = self.extract_list(&variables, "PROVIDES");
        let rprovides = self.extract_list(&variables, "RPROVIDES");

        // Update recipe metadata
        if let Some(recipe) = graph.get_recipe_mut(recipe_id) {
            if let Some(version) = variables.get("PV") {
                recipe.version = Some(version.clone());
            }
            recipe.metadata = variables.clone();
        }

        // Register providers
        if self.config.resolve_providers {
            for provider in &provides {
                graph.register_provider(recipe_id, provider);
            }
            for provider in &rprovides {
                graph.register_provider(recipe_id, provider);
            }
        }

        // Extract tasks if enabled
        let mut task_names = Vec::new();
        if self.config.extract_tasks {
            task_names = self.extract_tasks(graph, recipe_id, content);
        }

        Ok(RecipeExtraction {
            recipe_id,
            name: recipe_name,
            depends,
            rdepends,
            provides,
            rprovides,
            tasks: task_names,
            variables,
        })
    }

    /// Parse simple variable assignments from recipe content
    fn parse_variables(&self, content: &str) -> HashMap<String, String> {
        let mut vars = HashMap::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip comments and empty lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Look for simple assignments: VAR = "value"
            if let Some((left, right)) = line.split_once('=') {
                let var_name = left.trim();
                let value = right.trim().trim_matches('"').trim_matches('\'');

                // Skip complex assignments for now
                if var_name.contains('[') || var_name.contains(':') {
                    continue;
                }

                // Handle different assignment operators
                let (clean_name, should_store) = if var_name.ends_with("?") {
                    (var_name.trim_end_matches('?').trim(), true)
                } else {
                    (var_name, true)
                };

                if should_store && !clean_name.is_empty() {
                    vars.insert(clean_name.to_string(), value.to_string());
                }
            }
        }

        vars
    }

    /// Extract a space-separated list from variables
    fn extract_list(&self, variables: &HashMap<String, String>, key: &str) -> Vec<String> {
        variables
            .get(key)
            .map(|v| {
                v.split_whitespace()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Extract and parse dependency list, handling version constraints
    fn extract_dependency_list(
        &self,
        variables: &HashMap<String, String>,
        key: &str,
    ) -> Vec<String> {
        variables
            .get(key)
            .map(|v| {
                // Split and parse each dependency, handling version constraints
                v.split_whitespace()
                    .filter(|s| !s.is_empty())
                    .filter_map(|dep| {
                        // Skip version constraint parts like "(>=", "2.30)"
                        if dep.starts_with('(') || dep.ends_with(')') {
                            None
                        } else if let Some(pos) = dep.find('(') {
                            // Package has version attached: "glibc(>=2.30)"
                            Some(dep[..pos].trim().to_string())
                        } else {
                            Some(dep.to_string())
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Extract tasks from recipe content
    fn extract_tasks(
        &self,
        graph: &mut RecipeGraph,
        recipe_id: RecipeId,
        content: &str,
    ) -> Vec<String> {
        let mut task_names = Vec::new();
        let mut task_constraints = Vec::new();
        let mut task_flags = Vec::new();

        for line in content.lines() {
            let line = line.trim();

            // Parse addtask statements
            if let Some(task) = parse_addtask_statement(line) {
                let task_id = graph.add_task(recipe_id, &task.name);
                task_names.push(task.name.clone());
                task_constraints.push((task_id, task.after, task.before));
            }

            // Parse task flags
            if let Some((task_name, flag_name, value)) = parse_task_flag(line) {
                task_flags.push((task_name, flag_name, value));
            }
        }

        // Apply constraints (resolve names to IDs first, then apply)
        for (task_id, after_names, before_names) in task_constraints {
            // Resolve names to task IDs
            let after_ids: Vec<TaskId> = after_names
                .iter()
                .filter_map(|name| graph.find_task(recipe_id, name))
                .collect();
            let before_ids: Vec<TaskId> = before_names
                .iter()
                .filter_map(|name| graph.find_task(recipe_id, name))
                .collect();

            // Now apply them
            if let Some(task_node) = graph.get_task_mut(task_id) {
                task_node.after.extend(after_ids);
                task_node.before.extend(before_ids);
            }
        }

        // Apply flags
        for (task_name, flag_name, value) in task_flags {
            if let Some(task_id) = graph.find_task(recipe_id, &task_name) {
                if let Some(task_node) = graph.get_task_mut(task_id) {
                    task_node.flags.insert(flag_name, value);
                }
            }
        }

        task_names
    }

    /// Populate graph with dependencies after all recipes are extracted
    pub fn populate_dependencies(
        &self,
        graph: &mut RecipeGraph,
        extractions: &[RecipeExtraction],
    ) -> Result<(), String> {
        for extraction in extractions {
            // Add build-time dependencies
            for dep_name in &extraction.depends {
                if let Some(dep_id) = graph.resolve_provider(dep_name) {
                    graph.add_dependency(extraction.recipe_id, dep_id);
                }
            }

            // Add runtime dependencies
            for dep_name in &extraction.rdepends {
                if let Some(dep_id) = graph.resolve_provider(dep_name) {
                    graph.add_runtime_dependency(extraction.recipe_id, dep_id);
                }
            }
        }

        Ok(())
    }

    /// Extract recipe from file path
    pub fn extract_from_file(
        &self,
        graph: &mut RecipeGraph,
        file_path: &Path,
    ) -> Result<RecipeExtraction, String> {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let recipe_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "Invalid file name".to_string())?
            .to_string();

        let extraction = self.extract_from_content(graph, recipe_name, &content)?;

        // Update file path
        if let Some(recipe) = graph.get_recipe_mut(extraction.recipe_id) {
            recipe.file_path = Some(file_path.to_path_buf());
        }

        Ok(extraction)
    }

    /// Extract multiple recipes and populate dependencies
    pub fn extract_recipes(
        &self,
        graph: &mut RecipeGraph,
        recipe_files: &[impl AsRef<Path>],
    ) -> Result<Vec<RecipeExtraction>, String> {
        let mut extractions = Vec::new();

        // First pass: extract all recipes
        for file_path in recipe_files {
            match self.extract_from_file(graph, file_path.as_ref()) {
                Ok(extraction) => extractions.push(extraction),
                Err(e) => eprintln!("Warning: Failed to extract {}: {}",
                    file_path.as_ref().display(), e),
            }
        }

        // Second pass: populate dependencies
        self.populate_dependencies(graph, &extractions)?;

        Ok(extractions)
    }
}

impl Default for RecipeExtractor {
    fn default() -> Self {
        Self::new_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_variables() {
        let extractor = RecipeExtractor::new_default();
        let content = r#"
SUMMARY = "Test recipe"
LICENSE = "MIT"
PV = "1.0"
DEPENDS = "glibc openssl"
RDEPENDS = "bash"
"#;

        let vars = extractor.parse_variables(content);
        assert_eq!(vars.get("SUMMARY"), Some(&"Test recipe".to_string()));
        assert_eq!(vars.get("LICENSE"), Some(&"MIT".to_string()));
        assert_eq!(vars.get("PV"), Some(&"1.0".to_string()));
    }

    #[test]
    fn test_extract_list() {
        let extractor = RecipeExtractor::new_default();
        let mut vars = HashMap::new();
        vars.insert("DEPENDS".to_string(), "glibc openssl zlib".to_string());

        let deps = extractor.extract_list(&vars, "DEPENDS");
        assert_eq!(deps, vec!["glibc", "openssl", "zlib"]);
    }

    #[test]
    fn test_extract_from_content() {
        let mut graph = RecipeGraph::new();
        let extractor = RecipeExtractor::new(ExtractionConfig {
            extract_tasks: true,
            resolve_providers: true,
            ..Default::default()
        });

        let content = r#"
SUMMARY = "OpenSSL library"
LICENSE = "Apache-2.0"
PV = "3.0.0"
DEPENDS = "glibc"
PROVIDES = "openssl"

addtask compile after configure before install
"#;

        let extraction = extractor
            .extract_from_content(&mut graph, "openssl", content)
            .unwrap();

        assert_eq!(extraction.name, "openssl");
        assert_eq!(extraction.depends, vec!["glibc"]);
        assert_eq!(extraction.provides, vec!["openssl"]);
        assert!(!extraction.tasks.is_empty());

        // Check recipe was added to graph
        let recipe = graph.get_recipe(extraction.recipe_id).unwrap();
        assert_eq!(recipe.name, "openssl");
        assert_eq!(recipe.version, Some("3.0.0".to_string()));
    }

    #[test]
    fn test_dependency_population() {
        let mut graph = RecipeGraph::new();
        let extractor = RecipeExtractor::new_default();

        let content1 = r#"
SUMMARY = "glibc"
PV = "2.35"
"#;

        let content2 = r#"
SUMMARY = "OpenSSL"
PV = "3.0"
DEPENDS = "glibc"
"#;

        let ext1 = extractor
            .extract_from_content(&mut graph, "glibc", content1)
            .unwrap();
        let ext2 = extractor
            .extract_from_content(&mut graph, "openssl", content2)
            .unwrap();

        let ext1_id = ext1.recipe_id;
        let ext2_id = ext2.recipe_id;

        extractor
            .populate_dependencies(&mut graph, &[ext1, ext2])
            .unwrap();

        // Check dependency was added
        let deps = graph.get_dependencies(ext2_id);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0], ext1_id);
    }

    #[test]
    fn test_version_constraint_parsing() {
        let extractor = RecipeExtractor::new_default();
        let mut vars = HashMap::new();
        vars.insert(
            "DEPENDS".to_string(),
            "glibc (>= 2.30) openssl".to_string(),
        );

        let deps = extractor.extract_dependency_list(&vars, "DEPENDS");
        assert_eq!(deps, vec!["glibc", "openssl"]);
    }

    #[test]
    fn test_provider_resolution() {
        let mut graph = RecipeGraph::new();
        let extractor = RecipeExtractor::new(ExtractionConfig {
            resolve_providers: true,
            ..Default::default()
        });

        let content = r#"
SUMMARY = "Linux Kernel"
PV = "5.15"
PROVIDES = "virtual/kernel"
"#;

        let extraction = extractor
            .extract_from_content(&mut graph, "linux-yocto", content)
            .unwrap();

        // Check provider was registered
        let resolved = graph.resolve_provider("virtual/kernel");
        assert_eq!(resolved, Some(extraction.recipe_id));
    }
}
