// Recipe dependency extractor - populates RecipeGraph from BitBake files
// Combines parsing, variable resolution, and graph construction

use crate::recipe_graph::{RecipeGraph, RecipeId, TaskId};
use crate::task_parser::{parse_addtask_statement, parse_task_flag};
use crate::simple_python_eval::SimplePythonEvaluator;
use crate::class_dependencies;
use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};

#[cfg(feature = "python-execution")]
use crate::python_executor::PythonExecutor;

/// Configuration for recipe extraction
#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    /// Use Python executor for variable resolution (requires python-execution feature)
    pub use_python_executor: bool,
    /// Use simple Python expression evaluator (handles bb.utils.contains, bb.utils.filter)
    pub use_simple_python_eval: bool,
    /// Default build-time variables (e.g., DISTRO_FEATURES) for Python expression evaluation
    pub default_variables: HashMap<String, String>,
    /// Extract task dependencies
    pub extract_tasks: bool,
    /// Resolve virtual providers
    pub resolve_providers: bool,
    /// Resolve include/require directives
    pub resolve_includes: bool,
    /// Resolve inherit classes for task extraction
    pub resolve_inherit: bool,
    /// Extract dependencies from inherited classes (Phase 6)
    pub extract_class_deps: bool,
    /// Search paths for .bbclass files
    pub class_search_paths: Vec<std::path::PathBuf>,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        let mut default_variables = HashMap::new();
        // Provide sensible defaults for common build-time variables
        default_variables.insert("DISTRO_FEATURES".to_string(), "systemd pam ipv6 usrmerge".to_string());
        default_variables.insert("PACKAGECONFIG".to_string(), "".to_string());
        default_variables.insert("MACHINE_FEATURES".to_string(), "".to_string());

        Self {
            use_python_executor: false,
            use_simple_python_eval: false,
            default_variables,
            extract_tasks: false,
            resolve_providers: false,
            resolve_includes: false,
            resolve_inherit: false,
            extract_class_deps: false,
            class_search_paths: Vec::new(),
        }
    }
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

/// PACKAGECONFIG option declaration
#[derive(Debug, Clone)]
struct PackageConfigOption {
    name: String,
    enable_flags: String,
    disable_flags: String,
    build_deps: Vec<String>,
    runtime_deps: Vec<String>,
    runtime_recommends: Vec<String>,
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
        let mut variables = self.parse_variables(content);

        // Expand simple variable references (${PN}, ${PV}, ${BPN}, ${P})
        let vars_to_expand: Vec<String> = variables.keys().cloned().collect();
        for var_name in vars_to_expand {
            if let Some(value) = variables.get(&var_name).cloned() {
                let expanded = self.expand_simple_variables(&value, &recipe_name, &variables);
                if expanded != value {
                    variables.insert(var_name, expanded);
                }
            }
        }

        // Parse PACKAGECONFIG declarations
        let packageconfig_opts = self.parse_packageconfig(content);

        // Extract PACKAGECONFIG dependencies
        let (pkg_build_deps, pkg_runtime_deps) =
            self.extract_packageconfig_deps(&variables, &packageconfig_opts);

        // Extract dependencies
        let mut depends = self.extract_dependency_list(&variables, "DEPENDS");
        let mut rdepends = self.extract_dependency_list(&variables, "RDEPENDS");
        let provides = self.extract_list(&variables, "PROVIDES");
        let rprovides = self.extract_list(&variables, "RPROVIDES");

        // Merge PACKAGECONFIG dependencies
        depends.extend(pkg_build_deps);
        rdepends.extend(pkg_runtime_deps);

        // Extract class dependencies (Phase 6)
        if self.config.extract_class_deps {
            let (class_build_deps, class_runtime_deps) = self.extract_class_dependencies(content, &variables);
            depends.extend(class_build_deps);
            rdepends.extend(class_runtime_deps);
        }

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

    /// Handle BitBake line continuations (backslash at end of line)
    fn join_continued_lines(&self, content: &str) -> String {
        let mut result = String::new();
        let mut current_line = String::new();

        for line in content.lines() {
            // Check if line ends with backslash (continuation)
            let trimmed = line.trim_end();
            if trimmed.ends_with('\\') {
                // Remove backslash and append to current line
                current_line.push_str(trimmed.trim_end_matches('\\'));
                current_line.push(' '); // Add space between continued parts
            } else {
                // Complete line - append any continuation and add to result
                current_line.push_str(line);
                result.push_str(&current_line);
                result.push('\n');
                current_line.clear();
            }
        }

        // Handle any remaining partial line
        if !current_line.is_empty() {
            result.push_str(&current_line);
            result.push('\n');
        }

        result
    }

    /// Parse simple variable assignments from recipe content
    fn parse_variables(&self, content: &str) -> HashMap<String, String> {
        let mut vars: HashMap<String, String> = HashMap::new();

        // First, handle line continuations
        let joined_content = self.join_continued_lines(content);

        for line in joined_content.lines() {
            let line = line.trim();

            // Skip comments and empty lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse assignment with operator detection
            if let Some((var_name, operator, value)) = self.parse_assignment(line) {
                // Skip flag assignments like VAR[flag]
                if var_name.contains('[') {
                    continue;
                }

                // Evaluate Python expressions if enabled
                let mut value = value;
                if self.config.use_simple_python_eval {
                    value = self.eval_python_expressions_in_string(&value, &vars);
                }

                // Handle package-specific variables: RDEPENDS:${PN} or RDEPENDS:${PN}-ptest
                // Extract the base variable name and override
                let (clean_name, override_suffix) = if var_name.contains(':') {
                    // RDEPENDS:${PN} -> ("RDEPENDS", Some("${PN}"))
                    // DEPENDS:append -> ("DEPENDS", Some("append"))
                    let parts: Vec<&str> = var_name.splitn(2, ':').collect();
                    (parts[0], parts.get(1).copied())
                } else {
                    (var_name.as_str(), None)
                };

                if !clean_name.is_empty() {
                    // Apply operator
                    self.apply_variable_operator(&mut vars, clean_name, &operator, &value, override_suffix);
                }
            }
        }

        vars
    }

    /// Parse an assignment line and extract variable name, operator, and value
    /// Returns: (var_name, operator, value)
    fn parse_assignment(&self, line: &str) -> Option<(String, String, String)> {
        // Try to find assignment operators (in order of specificity)
        // Handle :append, :prepend, :remove first (they contain :)
        if let Some(pos) = line.find(":append") {
            let before = &line[..pos];
            if let Some(eq_pos) = line[pos..].find('=') {
                let var_name = before.trim().to_string();
                let value = self.clean_value(&line[pos + eq_pos + 1..]);
                return Some((var_name, ":append".to_string(), value));
            }
        }

        if let Some(pos) = line.find(":prepend") {
            let before = &line[..pos];
            if let Some(eq_pos) = line[pos..].find('=') {
                let var_name = before.trim().to_string();
                let value = self.clean_value(&line[pos + eq_pos + 1..]);
                return Some((var_name, ":prepend".to_string(), value));
            }
        }

        if let Some(pos) = line.find(":remove") {
            let before = &line[..pos];
            if let Some(eq_pos) = line[pos..].find('=') {
                let var_name = before.trim().to_string();
                let value = self.clean_value(&line[pos + eq_pos + 1..]);
                return Some((var_name, ":remove".to_string(), value));
            }
        }

        // Handle +=, ?=, .=, =+, =. operators
        for (op, op_str) in &[("+=", "+="), ("?=", "?="), (".=", ".="), ("=+", "=+"), ("=.", "=.")] {
            if let Some(pos) = line.find(op) {
                let var_name = line[..pos].trim().to_string();
                let value = self.clean_value(&line[pos + op.len()..]);
                return Some((var_name, op_str.to_string(), value));
            }
        }

        // Handle simple assignment =
        if let Some((left, right)) = line.split_once('=') {
            let var_name = left.trim().to_string();
            let value = self.clean_value(right);
            return Some((var_name, "=".to_string(), value));
        }

        None
    }

    /// Clean value: trim whitespace, remove quotes, trim backslashes
    fn clean_value(&self, value: &str) -> String {
        value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .trim_end_matches('\\')
            .trim()
            .to_string()
    }

    /// Apply variable operator (=, +=, :append, :prepend, :remove, ?=, etc.)
    fn apply_variable_operator(
        &self,
        vars: &mut HashMap<String, String>,
        var_name: &str,
        operator: &str,
        value: &str,
        override_suffix: Option<&str>,
    ) {
        match operator {
            "=" => {
                // Simple assignment - replace or append based on override
                if let Some(_override) = override_suffix {
                    // For now, treat overrides as append (simplified)
                    // TODO: Proper override resolution in Phase 7
                    if let Some(existing) = vars.get(var_name) {
                        let mut combined = existing.clone();
                        if !combined.is_empty() && !value.is_empty() {
                            combined.push(' ');
                        }
                        combined.push_str(value);
                        vars.insert(var_name.to_string(), combined);
                    } else {
                        vars.insert(var_name.to_string(), value.to_string());
                    }
                } else {
                    vars.insert(var_name.to_string(), value.to_string());
                }
            }
            "+=" | ":append" | ".=" => {
                // Append to existing value
                if let Some(existing) = vars.get(var_name) {
                    let mut combined = existing.clone();
                    if !combined.is_empty() && !value.is_empty() {
                        combined.push(' ');
                    }
                    combined.push_str(value);
                    vars.insert(var_name.to_string(), combined);
                } else {
                    vars.insert(var_name.to_string(), value.to_string());
                }
            }
            "=+" | ":prepend" => {
                // Prepend to existing value
                if let Some(existing) = vars.get(var_name) {
                    let mut combined = value.to_string();
                    if !combined.is_empty() && !existing.is_empty() {
                        combined.push(' ');
                    }
                    combined.push_str(existing);
                    vars.insert(var_name.to_string(), combined);
                } else {
                    vars.insert(var_name.to_string(), value.to_string());
                }
            }
            ":remove" => {
                // Remove items from existing value
                if let Some(existing) = vars.get(var_name) {
                    let items_to_remove: Vec<&str> = value.split_whitespace().collect();
                    let filtered: Vec<&str> = existing
                        .split_whitespace()
                        .filter(|item| !items_to_remove.contains(item))
                        .collect();
                    vars.insert(var_name.to_string(), filtered.join(" "));
                }
                // If variable doesn't exist, :remove does nothing
            }
            "?=" => {
                // Conditional assignment - only set if not already set
                if !vars.contains_key(var_name) {
                    vars.insert(var_name.to_string(), value.to_string());
                }
            }
            _ => {
                // Unknown operator - treat as simple assignment
                vars.insert(var_name.to_string(), value.to_string());
            }
        }
    }

    /// Evaluate Python expressions in a string value
    /// Replaces ${@...} expressions with their evaluated results
    fn eval_python_expressions_in_string(&self, value: &str, vars: &HashMap<String, String>) -> String {
        // Find all ${@...} expressions
        let mut result = value.to_string();
        let mut start = 0;

        while let Some(pos) = result[start..].find("${@") {
            let abs_pos = start + pos;

            // Find matching }
            if let Some(end_pos) = self.find_closing_brace(&result[abs_pos..]) {
                let expr = &result[abs_pos..abs_pos + end_pos + 1];

                // Try to evaluate it
                // Merge default variables with recipe variables (recipe vars take precedence)
                let mut eval_vars = self.config.default_variables.clone();
                eval_vars.extend(vars.clone());

                let evaluator = SimplePythonEvaluator::new(eval_vars);
                if let Some(evaluated) = evaluator.evaluate(expr) {
                    // Replace the expression with the evaluated result
                    result.replace_range(abs_pos..abs_pos + end_pos + 1, &evaluated);
                    start = abs_pos + evaluated.len();
                } else {
                    // Can't evaluate, keep original and move past it
                    start = abs_pos + end_pos + 1;
                }
            } else {
                // No matching }, move past this occurrence
                start = abs_pos + 3;
            }
        }

        result
    }

    /// Find the closing brace for a ${@...} expression
    fn find_closing_brace(&self, s: &str) -> Option<usize> {
        let mut depth = 0;
        let mut in_single_quote = false;
        let mut in_double_quote = false;

        for (i, ch) in s.chars().enumerate() {
            match ch {
                '\'' if !in_double_quote => in_single_quote = !in_single_quote,
                '"' if !in_single_quote => in_double_quote = !in_double_quote,
                '{' if !in_single_quote && !in_double_quote => depth += 1,
                '}' if !in_single_quote && !in_double_quote => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }

        None
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

    /// Parse PACKAGECONFIG declarations and return a map of option -> (enable, disable, bdeps, rdeps, rrecommends)
    fn parse_packageconfig(&self, content: &str) -> HashMap<String, PackageConfigOption> {
        let mut configs = HashMap::new();

        for line in content.lines() {
            let line = line.trim();

            // Look for PACKAGECONFIG[option] = "..."
            if line.starts_with("PACKAGECONFIG[") {
                if let Some(bracket_end) = line.find(']') {
                    let option_name = &line[14..bracket_end]; // Skip "PACKAGECONFIG["

                    // Find the assignment
                    if let Some(eq_pos) = line[bracket_end..].find('=') {
                        let value_start = bracket_end + eq_pos + 1;
                        let value = line[value_start..]
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'');

                        // Parse comma-separated fields
                        let fields: Vec<&str> = value.split(',').map(|s| s.trim()).collect();

                        let enable = fields.get(0).unwrap_or(&"").to_string();
                        let disable = fields.get(1).unwrap_or(&"").to_string();
                        let build_deps: Vec<String> = fields
                            .get(2)
                            .unwrap_or(&"")
                            .split_whitespace()
                            .map(String::from)
                            .collect();
                        let runtime_deps: Vec<String> = fields
                            .get(3)
                            .unwrap_or(&"")
                            .split_whitespace()
                            .map(String::from)
                            .collect();
                        let runtime_recommends: Vec<String> = fields
                            .get(4)
                            .unwrap_or(&"")
                            .split_whitespace()
                            .map(String::from)
                            .collect();

                        configs.insert(
                            option_name.to_string(),
                            PackageConfigOption {
                                name: option_name.to_string(),
                                enable_flags: enable,
                                disable_flags: disable,
                                build_deps,
                                runtime_deps,
                                runtime_recommends,
                            },
                        );
                    }
                }
            }
        }

        configs
    }

    /// Extract dependencies from PACKAGECONFIG
    fn extract_packageconfig_deps(
        &self,
        variables: &HashMap<String, String>,
        configs: &HashMap<String, PackageConfigOption>,
    ) -> (Vec<String>, Vec<String>) {
        let mut build_deps = Vec::new();
        let mut runtime_deps = Vec::new();

        // Get the active PACKAGECONFIG options
        if let Some(active_options) = variables.get("PACKAGECONFIG") {
            for option in active_options.split_whitespace() {
                if let Some(config) = configs.get(option) {
                    // Add build dependencies
                    for dep in &config.build_deps {
                        if !dep.is_empty() && !build_deps.contains(dep) {
                            build_deps.push(dep.clone());
                        }
                    }

                    // Add runtime dependencies
                    for dep in &config.runtime_deps {
                        if !dep.is_empty() && !runtime_deps.contains(dep) {
                            runtime_deps.push(dep.clone());
                        }
                    }
                }
            }
        }

        (build_deps, runtime_deps)
    }

    /// Expand simple variable references in a string
    /// Expands ${PN}, ${PV}, ${BPN}, ${P} while keeping complex variables like ${VIRTUAL-RUNTIME_*}
    fn expand_simple_variables(
        &self,
        value: &str,
        recipe_name: &str,
        variables: &HashMap<String, String>,
    ) -> String {
        let mut result = value.to_string();
        let mut start = 0;

        while let Some(pos) = result[start..].find("${") {
            let abs_pos = start + pos;

            // Find closing brace
            if let Some(end_pos) = result[abs_pos + 2..].find('}') {
                let var_name = &result[abs_pos + 2..abs_pos + 2 + end_pos];

                // Determine replacement value
                let replacement = match var_name {
                    "PN" => Some(recipe_name.to_string()),
                    "BPN" => {
                        // BPN is PN without prefix like nativesdk-
                        let bn = recipe_name
                            .strip_prefix("nativesdk-")
                            .or_else(|| recipe_name.strip_prefix("native-"))
                            .unwrap_or(recipe_name);
                        Some(bn.to_string())
                    }
                    "PV" => variables.get("PV").cloned(),
                    "P" => {
                        // P = ${PN}-${PV}
                        if let Some(pv) = variables.get("PV") {
                            Some(format!("{}-{}", recipe_name, pv))
                        } else {
                            None
                        }
                    }
                    // Keep other variables as-is (e.g., VIRTUAL-RUNTIME_*, TARGET_*, etc.)
                    _ => None,
                };

                if let Some(repl) = replacement {
                    // Replace the variable reference
                    result.replace_range(abs_pos..abs_pos + end_pos + 3, &repl);
                    start = abs_pos + repl.len();
                } else {
                    // Keep the variable as-is, move past it
                    start = abs_pos + end_pos + 3;
                }
            } else {
                // No closing brace found, stop searching
                break;
            }
        }

        result
    }

    /// Extract dependencies from inherited classes (Phase 6)
    fn extract_class_dependencies(
        &self,
        content: &str,
        variables: &HashMap<String, String>,
    ) -> (Vec<String>, Vec<String>) {
        let mut build_deps = Vec::new();
        let mut runtime_deps = Vec::new();

        // Extract inherited classes
        let classes = class_dependencies::extract_inherited_classes(content);

        // Get DISTRO_FEATURES for conditional class dependencies
        let distro_features = variables
            .get("DISTRO_FEATURES")
            .or_else(|| self.config.default_variables.get("DISTRO_FEATURES"))
            .map(|s| s.as_str())
            .unwrap_or("");

        // Get dependencies for each class
        for class_name in classes {
            let class_build_deps = class_dependencies::get_class_build_deps(&class_name, distro_features);
            let class_runtime_deps = class_dependencies::get_class_runtime_deps(&class_name, distro_features);

            // Add build dependencies
            for dep in class_build_deps {
                if !dep.is_empty() && !build_deps.contains(&dep) {
                    build_deps.push(dep);
                }
            }

            // Add runtime dependencies
            for dep in class_runtime_deps {
                if !dep.is_empty() && !runtime_deps.contains(&dep) {
                    runtime_deps.push(dep);
                }
            }
        }

        (build_deps, runtime_deps)
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

        // Add base tasks that all BitBake recipes inherit from base.bbclass
        // These are standard tasks defined in meta/classes-global/base.bbclass
        let base_tasks = vec![
            "fetch",
            "unpack",
            "patch",
            "configure",
            "compile",
            "install",
            "package",
            "populate_sysroot",
            "build",
        ];

        for task_name in base_tasks {
            graph.add_task(recipe_id, task_name);
            task_names.push(task_name.to_string());
        }

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
        let mut content = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let recipe_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "Invalid file name".to_string())?
            .split('_')
            .next()
            .unwrap_or("")
            .to_string();

        // Resolve includes if enabled
        if self.config.resolve_includes {
            content = self.resolve_includes_in_content(&content, file_path, &recipe_name)?;
        }

        // Resolve inherit classes if enabled
        if self.config.resolve_inherit {
            content = self.resolve_inherit_in_content(&content, file_path)?;
        }

        let extraction = self.extract_from_content(graph, recipe_name, &content)?;

        // Update file path
        if let Some(recipe) = graph.get_recipe_mut(extraction.recipe_id) {
            recipe.file_path = Some(file_path.to_path_buf());
        }

        Ok(extraction)
    }

    /// Resolve include/require directives in content
    fn resolve_includes_in_content(
        &self,
        content: &str,
        recipe_path: &Path,
        recipe_name: &str,
    ) -> Result<String, String> {
        let mut resolved = String::new();
        let mut seen_files = std::collections::HashSet::new();

        let recipe_dir = recipe_path
            .parent()
            .ok_or_else(|| "Recipe has no parent directory".to_string())?;

        // Get base name for variable expansion: bash_5.2.21.bb -> bash
        let base_name = recipe_name.to_string();

        for line in content.lines() {
            let trimmed = line.trim();

            // Check for include/require directives
            if let Some(include_path) = self.parse_include_directive(trimmed) {
                // Expand simple variables like ${BPN}
                let expanded = include_path.replace("${BPN}", &base_name);

                // Try to find and read the include file
                match self.find_include_file(&expanded, recipe_dir, &base_name) {
                    Some(include_file_path) => {
                        // Avoid circular includes
                        if seen_files.contains(&include_file_path) {
                            resolved.push_str("# ");
                            resolved.push_str(line);
                            resolved.push('\n');
                            continue;
                        }
                        seen_files.insert(include_file_path.clone());

                        // Read the include file
                        match std::fs::read_to_string(&include_file_path) {
                            Ok(include_content) => {
                                // Recursively resolve includes in the included file
                                match self.resolve_includes_in_content(
                                    &include_content,
                                    &include_file_path,
                                    &base_name,
                                ) {
                                    Ok(resolved_include) => {
                                        resolved.push_str(&resolved_include);
                                        resolved.push('\n');
                                    }
                                    Err(_) => {
                                        // If recursive resolution fails, just use the content
                                        resolved.push_str(&include_content);
                                        resolved.push('\n');
                                    }
                                }
                            }
                            Err(_) => {
                                // Include file not readable - comment it out
                                resolved.push_str("# ");
                                resolved.push_str(line);
                                resolved.push('\n');
                            }
                        }
                    }
                    None => {
                        // Include file not found - check if required
                        if trimmed.starts_with("require ") {
                            // require is fatal if file not found, but we'll be lenient
                            resolved.push_str("# ");
                            resolved.push_str(line);
                            resolved.push('\n');
                        } else {
                            // include is non-fatal
                            resolved.push_str("# ");
                            resolved.push_str(line);
                            resolved.push('\n');
                        }
                    }
                }
            } else {
                // Not an include directive - keep as is
                resolved.push_str(line);
                resolved.push('\n');
            }
        }

        Ok(resolved)
    }

    /// Parse include/require directive from a line
    fn parse_include_directive<'a>(&self, line: &'a str) -> Option<&'a str> {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("require ") {
            return Some(rest.trim());
        }

        if let Some(rest) = trimmed.strip_prefix("include ") {
            return Some(rest.trim());
        }

        None
    }

    /// Find include file by searching relative to recipe directory
    fn find_include_file(
        &self,
        include_path: &str,
        recipe_dir: &Path,
        base_name: &str,
    ) -> Option<std::path::PathBuf> {
        // Try relative to recipe directory
        let candidate = recipe_dir.join(include_path);
        if candidate.exists() {
            return Some(candidate);
        }

        // Try with base name prefix if include path is simple
        // Example: bash_5.2.21.bb + "bash.inc" -> bash.inc in same dir
        if include_path.ends_with(".inc") && include_path.starts_with(base_name) {
            let candidate = recipe_dir.join(include_path);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        None
    }

    /// Resolve inherit statements to extract tasks from classes
    fn resolve_inherit_in_content(
        &self,
        content: &str,
        recipe_path: &Path,
    ) -> Result<String, String> {
        let mut resolved = String::from(content);
        let mut class_content = String::new();

        // Parse inherit statements
        for line in content.lines() {
            let trimmed = line.trim();

            if let Some(classes) = self.parse_inherit_statement(trimmed) {
                // Process each class
                for class_name in classes {
                    if let Some(class_path) = self.find_class_file(&class_name, recipe_path) {
                        match std::fs::read_to_string(&class_path) {
                            Ok(content) => {
                                // Extract just the addtask statements and task flags
                                for line in content.lines() {
                                    let line_trimmed = line.trim();
                                    if line_trimmed.starts_with("addtask ") ||
                                       (line_trimmed.starts_with("do_") && line_trimmed.contains('[')) {
                                        class_content.push_str(line);
                                        class_content.push('\n');
                                    }
                                }
                            }
                            Err(_) => {
                                // Class file not readable, skip
                            }
                        }
                    }
                }
            }
        }

        // Append class tasks to the resolved content
        if !class_content.is_empty() {
            resolved.push_str("\n# Tasks from inherited classes\n");
            resolved.push_str(&class_content);
        }

        Ok(resolved)
    }

    /// Parse inherit statement
    fn parse_inherit_statement<'a>(&self, line: &'a str) -> Option<Vec<&'a str>> {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("inherit ") {
            // Split class names: inherit autotools gettext ptest
            Some(rest.split_whitespace().collect())
        } else {
            None
        }
    }

    /// Find .bbclass file in search paths
    fn find_class_file(&self, class_name: &str, recipe_path: &Path) -> Option<std::path::PathBuf> {
        let class_filename = format!("{}.bbclass", class_name);

        // Try configured search paths first
        for search_path in &self.config.class_search_paths {
            let candidate = search_path.join(&class_filename);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        // Try to find classes relative to recipe (common Yocto structure)
        // Recipe is typically in: meta/recipes-*/category/recipe.bb
        // Classes are in: meta/classes-recipe/*.bbclass or meta/classes/*.bbclass
        if let Some(recipe_dir) = recipe_path.parent() {
            // Go up until we find 'meta' directory
            let mut current = recipe_dir;
            for _ in 0..5 {  // Max 5 levels up
                if let Some(parent) = current.parent() {
                    if current.file_name().and_then(|n| n.to_str()) == Some("meta") ||
                       parent.file_name().and_then(|n| n.to_str()) == Some("meta") {
                        // Found meta directory, check for classes
                        let meta_dir = if current.file_name().and_then(|n| n.to_str()) == Some("meta") {
                            current
                        } else {
                            parent
                        };

                        // Try classes-recipe/ first (newer Yocto)
                        let classes_recipe = meta_dir.join("classes-recipe").join(&class_filename);
                        if classes_recipe.exists() {
                            return Some(classes_recipe);
                        }

                        // Try classes/ (older Yocto)
                        let classes = meta_dir.join("classes").join(&class_filename);
                        if classes.exists() {
                            return Some(classes);
                        }
                    }
                    current = parent;
                } else {
                    break;
                }
            }
        }

        None
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
