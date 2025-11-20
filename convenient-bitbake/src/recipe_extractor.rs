// Recipe dependency extractor - populates RecipeGraph from BitBake files
// Combines parsing, variable resolution, and graph construction

use crate::recipe_graph::{RecipeGraph, RecipeId, TaskId};
use crate::task_parser::{parse_addtask_statement, parse_deltask_statement, parse_task_flag};
use crate::simple_python_eval::SimplePythonEvaluator;
use crate::python_ir_parser::PythonIRParser;
use crate::python_ir_executor::IRExecutor;
use crate::python_ir::ExecutionStrategy;
use crate::class_dependencies;
use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[cfg(feature = "python-execution")]
use crate::python_executor::PythonExecutor;

/// Build context for override resolution
#[derive(Debug, Clone, PartialEq)]
pub struct BuildContext {
    /// Class context: "native", "target", "nativesdk", "cross"
    pub class: String,
    /// Libc: "glibc", "musl", "newlib"
    pub libc: String,
    /// Architecture: "x86-64", "arm", "aarch64", etc.
    pub arch: String,
    /// Additional active overrides
    pub overrides: Vec<String>,
}

impl Default for BuildContext {
    fn default() -> Self {
        Self {
            class: "target".to_string(),
            libc: "glibc".to_string(),
            arch: "x86-64".to_string(),
            overrides: Vec::new(),
        }
    }
}

/// Configuration for recipe extraction
#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    /// Use Python executor for variable resolution (requires python-execution feature)
    pub use_python_executor: bool,
    /// Use simple Python expression evaluator (handles bb.utils.contains, bb.utils.filter)
    pub use_simple_python_eval: bool,
    /// Use Python IR parser + executor for enhanced Python block processing (Phase 10)
    pub use_python_ir: bool,
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
    /// Build context for override resolution (Phase 7c)
    pub build_context: BuildContext,
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
            use_python_ir: true,  // Enable by default (Phase 10)
            default_variables,
            extract_tasks: false,
            resolve_providers: false,
            resolve_includes: false,
            resolve_inherit: false,
            extract_class_deps: false,
            class_search_paths: Vec::new(),
            build_context: BuildContext::default(),
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
    /// Phase 9c: Variable flags - VAR[flag] = value
    /// Outer key: variable name, inner HashMap: flag name -> value
    #[serde(default)]
    pub variable_flags: HashMap<String, HashMap<String, String>>,
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

/// Python block information (Phase 10)
#[derive(Debug, Clone)]
struct PythonBlockInfo {
    func_name: String,
    is_anonymous: bool,
    code: String,
    start_pos: usize,
    end_pos: usize,
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

        // Phase 10: Process Python blocks (anonymous functions)
        if self.config.use_python_ir {
            variables = self.process_python_blocks(content, variables);
        }

        // Parse PACKAGECONFIG declarations
        let packageconfig_opts = self.parse_packageconfig(content);

        // Extract PACKAGECONFIG dependencies
        let (pkg_build_deps, pkg_runtime_deps) =
            self.extract_packageconfig_deps(&variables, &packageconfig_opts);

        // Extract dependencies
        let mut depends = self.extract_dependency_list(&variables, "DEPENDS");
        let mut rdepends = self.extract_dependency_list(&variables, "RDEPENDS");

        // Also check for package-specific RDEPENDS:${PN} variants
        if let Some(pn) = variables.get("PN") {
            let pkg_specific_key = format!("RDEPENDS:{}", pn);
            let pkg_rdepends = self.extract_dependency_list(&variables, &pkg_specific_key);
            rdepends.extend(pkg_rdepends);
        }

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

        // Phase 9c: Parse variable flags
        let variable_flags = self.parse_variable_flags(content);

        // Phase 9c: Extract task dependencies from do_*[depends] flags
        let task_depends = self.extract_task_flag_depends(&variable_flags);
        depends.extend(task_depends);
        depends.sort();
        depends.dedup();

        Ok(RecipeExtraction {
            recipe_id,
            name: recipe_name,
            depends,
            rdepends,
            provides,
            rprovides,
            tasks: task_names,
            variables,
            variable_flags,
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
    pub fn parse_variables(&self, content: &str) -> HashMap<String, String> {
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
            // Phase 9b: parse_assignment now returns (var_name, operator, override, value)
            if let Some((var_name, operator, override_suffix, value)) = self.parse_assignment(line) {
                // Skip flag assignments like VAR[flag]
                if var_name.contains('[') {
                    continue;
                }

                // Evaluate Python expressions if enabled
                let mut value = value;
                if self.config.use_simple_python_eval {
                    value = self.eval_python_expressions_in_string(&value, &vars);
                }

                // Phase 9b: override_suffix is now directly extracted by parse_assignment
                // Apply operator with the extracted override
                if !var_name.is_empty() {
                    self.apply_variable_operator(&mut vars, &var_name, &operator, &value, override_suffix.as_deref());
                }
            }
        }

        vars
    }

    /// Phase 9c: Parse variable flags from content
    /// Extracts VAR[flag] = value statements
    /// Returns: HashMap<var_name, HashMap<flag_name, flag_value>>
    fn parse_variable_flags(&self, content: &str) -> HashMap<String, HashMap<String, String>> {
        let mut flags: HashMap<String, HashMap<String, String>> = HashMap::new();

        let joined_content = self.join_continued_lines(content);

        for line in joined_content.lines() {
            let line = line.trim();

            // Skip comments and empty lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Match VAR[flag] = value or VAR[flag] += value patterns
            if let Some((var_part, flag_value)) = line.split_once('=') {
                let var_part = var_part.trim();

                // Check if this is a flag assignment: VAR[flag]
                if let Some(open_bracket) = var_part.find('[')
                    && let Some(close_bracket) = var_part.find(']')
                        && open_bracket < close_bracket {
                            let var_name = var_part[..open_bracket].trim().to_string();
                            let flag_name = var_part[open_bracket + 1..close_bracket].trim().to_string();
                            let value = self.clean_value(flag_value);

                            // Store the flag
                            flags
                                .entry(var_name)
                                .or_default()
                                .insert(flag_name, value);
                        }
            }
        }

        flags
    }

    /// Phase 9c: Extract task dependencies from do_*[depends] flags
    /// Parses dependencies like: do_compile[depends] = "virtual/kernel:do_shared_workdir"
    /// Returns: Vec of recipe dependencies extracted from task flags
    fn extract_task_flag_depends(&self, flags: &HashMap<String, HashMap<String, String>>) -> Vec<String> {
        let mut depends = Vec::new();

        for (var_name, var_flags) in flags {
            // Look for do_* tasks with depends flags
            if var_name.starts_with("do_")
                && let Some(depends_value) = var_flags.get("depends") {
                    // Parse dependencies: "recipe1:do_task1 recipe2:do_task2"
                    for dep in depends_value.split_whitespace() {
                        // Extract recipe name before the colon
                        if let Some(colon_pos) = dep.find(':') {
                            let recipe = dep[..colon_pos].trim();
                            if !recipe.is_empty() {
                                depends.push(recipe.to_string());
                            }
                        } else {
                            // No colon, treat entire string as recipe name
                            if !dep.is_empty() {
                                depends.push(dep.to_string());
                            }
                        }
                    }
                }
        }

        depends
    }

    /// Parse an assignment line and extract variable name, operator, override, and value
    /// Phase 9b: Enhanced to properly handle chained overrides like DEPENDS:append:qemux86
    /// Returns: (var_name, operator, override_suffix, value)
    fn parse_assignment(&self, line: &str) -> Option<(String, String, Option<String>, String)> {
        // Try to find assignment operators (in order of specificity)
        // Handle :append, :prepend, :remove first (they contain :)
        if let Some(pos) = line.find(":append") {
            let before = &line[..pos];
            if let Some(eq_pos) = line[pos..].find('=') {
                let var_name = before.trim().to_string();

                // Phase 9b: Check for override suffix after :append
                // DEPENDS:append:qemux86 = "foo" -> override = Some("qemux86")
                let after_append = &line[pos + 7..pos + eq_pos]; // Skip ":append"
                let override_suffix = if after_append.starts_with(':') {
                    Some(after_append[1..].trim().to_string())
                } else {
                    None
                };

                let value = self.clean_value(&line[pos + eq_pos + 1..]);
                return Some((var_name, ":append".to_string(), override_suffix, value));
            }
        }

        if let Some(pos) = line.find(":prepend") {
            let before = &line[..pos];
            if let Some(eq_pos) = line[pos..].find('=') {
                let var_name = before.trim().to_string();

                // Phase 9b: Check for override suffix after :prepend
                let after_prepend = &line[pos + 8..pos + eq_pos]; // Skip ":prepend"
                let override_suffix = if after_prepend.starts_with(':') {
                    Some(after_prepend[1..].trim().to_string())
                } else {
                    None
                };

                let value = self.clean_value(&line[pos + eq_pos + 1..]);
                return Some((var_name, ":prepend".to_string(), override_suffix, value));
            }
        }

        if let Some(pos) = line.find(":remove") {
            let before = &line[..pos];
            if let Some(eq_pos) = line[pos..].find('=') {
                let var_name = before.trim().to_string();

                // Phase 9b: Check for override suffix after :remove
                let after_remove = &line[pos + 7..pos + eq_pos]; // Skip ":remove"
                let override_suffix = if after_remove.starts_with(':') {
                    Some(after_remove[1..].trim().to_string())
                } else {
                    None
                };

                let value = self.clean_value(&line[pos + eq_pos + 1..]);
                return Some((var_name, ":remove".to_string(), override_suffix, value));
            }
        }

        // Handle +=, ?=, ??=, .=, =+, =. operators
        // Phase 7g: Add ??= (weak default) support
        for (op, op_str) in &[("??=", "??="), ("+=", "+="), ("?=", "?="), (".=", ".="), ("=+", "=+"), ("=.", "=.")] {
            if let Some(pos) = line.find(op) {
                let var_name = line[..pos].trim().to_string();
                let value = self.clean_value(&line[pos + op.len()..]);
                // These operators don't support override syntax
                return Some((var_name, op_str.to_string(), None, value));
            }
        }

        // Handle simple assignment =
        // Phase 9b: Support VAR:override = value syntax
        if let Some((left, right)) = line.split_once('=') {
            let var_part = left.trim();
            let value = self.clean_value(right);

            // Check if there's an override suffix: VAR:machine = value
            if let Some(colon_pos) = var_part.find(':') {
                let var_name = var_part[..colon_pos].trim().to_string();
                let override_suffix = var_part[colon_pos + 1..].trim().to_string();
                // Don't treat known operators as overrides
                if !["append", "prepend", "remove"].contains(&override_suffix.as_str()) {
                    return Some((var_name, "=".to_string(), Some(override_suffix), value));
                }
            }

            return Some((var_part.to_string(), "=".to_string(), None, value));
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

    /// Check if an override is active based on build context (Phase 7c)
    fn is_override_active(&self, override_str: &str) -> bool {
        if override_str.is_empty() {
            return true; // No override - always active
        }

        let ctx = &self.config.build_context;

        match override_str {
            // Class overrides
            "class-native" => ctx.class == "native",
            "class-target" => ctx.class == "target",
            "class-nativesdk" => ctx.class == "nativesdk",
            "class-cross" => ctx.class == "cross",

            // Libc overrides
            "libc-glibc" => ctx.libc == "glibc",
            "libc-musl" => ctx.libc == "musl",
            "libc-newlib" => ctx.libc == "newlib",

            // Architecture overrides (common ones)
            "x86-64" | "amd64" => ctx.arch == "x86-64",
            "arm" => ctx.arch == "arm",
            "aarch64" | "arm64" => ctx.arch == "aarch64",
            "mips" => ctx.arch == "mips",
            "powerpc" => ctx.arch == "powerpc",
            "riscv64" => ctx.arch == "riscv64",

            // Check custom overrides list
            _ => ctx.overrides.iter().any(|o| o == override_str),
        }
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
                // Simple assignment - for dependency extraction, be inclusive
                if let Some(override_str) = override_suffix {
                    // Phase 7c: For dependency extraction, include all override variants
                    // This gives us a complete picture of dependencies across contexts
                    if let Some(existing) = vars.get(var_name) {
                        // Merge with existing value if override is active or if we want complete deps
                        if self.is_override_active(override_str) || existing.is_empty() {
                            let mut combined = existing.clone();
                            if !combined.is_empty() && !value.is_empty() {
                                combined.push(' ');
                            }
                            combined.push_str(value);
                            vars.insert(var_name.to_string(), combined);
                        }
                    } else {
                        vars.insert(var_name.to_string(), value.to_string());
                    }
                } else {
                    // No override - simple assignment
                    vars.insert(var_name.to_string(), value.to_string());
                }
            }
            "+=" | ":append" | ".=" => {
                // Append to existing value
                // For dependency extraction, include all variants regardless of override
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
                // For dependency extraction, include all variants regardless of override
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
                // Remove items from existing value - only apply if override is active (Phase 7c)
                if let Some(override_str) = override_suffix
                    && !self.is_override_active(override_str) {
                        return; // Skip removal if override not active
                    }

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
            "??=" => {
                // Phase 7g: Weak default assignment - lowest precedence
                // Only set if not already set (like ?=, but even weaker)
                // For static analysis, treat same as ?=
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

    /// Dedent Python code by removing common leading whitespace
    #[cfg(feature = "python-execution")]
    fn dedent_python(&self, code: &str) -> String {
        let lines: Vec<&str> = code.lines().collect();

        // Find minimum indentation (ignoring empty lines and lines with only closing braces)
        let min_indent = lines.iter()
            .filter(|line| !line.trim().is_empty())
            .filter(|line| line.trim() != "}")  // Ignore BitBake closing braces
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);

        // Remove that amount of indentation from each line, and filter out closing braces
        lines.iter()
            .filter_map(|line| {
                if line.trim() == "}" {
                    None  // Skip closing braces
                } else if line.trim().is_empty() {
                    Some("")
                } else {
                    Some(&line[min_indent.min(line.len())..])
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Process Python blocks (anonymous functions) and merge variable changes
    /// Phase 10: Extract and execute Python blocks via IR system
    fn process_python_blocks(&self, content: &str, mut vars: HashMap<String, String>) -> HashMap<String, String> {
        // Find all Python blocks: python __anonymous() { ... } or python do_*() { ... }
        let python_blocks = self.extract_python_blocks(content);

        // Merge default variables with recipe variables
        let mut eval_vars = self.config.default_variables.clone();
        eval_vars.extend(vars.clone());

        for python_block in python_blocks {
            // Only process anonymous blocks for now (they run at parse time)
            if python_block.is_anonymous {
                // Try to parse to IR first
                let parser = PythonIRParser::new();
                let ir_result = parser.parse(&python_block.code, eval_vars.clone());

                if let Some(ir) = ir_result {
                    // Execute based on complexity
                    match ir.execution_strategy() {
                        ExecutionStrategy::Static | ExecutionStrategy::Hybrid => {
                            let mut executor = IRExecutor::new(eval_vars.clone());
                            let result = executor.execute(&ir);

                            if result.success {
                                // Merge variable changes back
                                for (var_name, value) in result.variables_set {
                                    vars.insert(var_name.clone(), value.clone());
                                    eval_vars.insert(var_name, value);
                                }
                            }
                        }
                        ExecutionStrategy::RustPython => {
                            // For complex blocks, use RustPython executor if enabled
                            #[cfg(feature = "python-execution")]
                            {
                                if let Some(ref executor) = self.executor {
                                    let dedented_code = self.dedent_python(&python_block.code);
                                    let result = executor.execute(&dedented_code, &eval_vars);
                                    if result.success {
                                        for (var_name, value) in result.variables_set {
                                            vars.insert(var_name.clone(), value.clone());
                                            eval_vars.insert(var_name, value);
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // IR parser failed - fall back to RustPython if available
                    #[cfg(feature = "python-execution")]
                    {
                        if let Some(ref executor) = self.executor {
                            let dedented_code = self.dedent_python(&python_block.code);
                            let result = executor.execute(&dedented_code, &eval_vars);
                            if result.success {
                                for (var_name, value) in result.variables_set {
                                    vars.insert(var_name.clone(), value.clone());
                                    eval_vars.insert(var_name, value);
                                }
                            }
                        }
                    }
                }
            }
        }

        vars
    }

    /// Extract Python function blocks from recipe content
    fn extract_python_blocks(&self, content: &str) -> Vec<PythonBlockInfo> {
        let mut blocks = Vec::new();

        // Pattern: python __anonymous() { ... } or python do_*() { ... }
        // We need to handle brace matching properly
        // Build a vector of (byte_pos, char) for proper UTF-8 handling
        let char_indices: Vec<(usize, char)> = content.char_indices().collect();
        let mut idx = 0;

        while idx < char_indices.len() {
            let (byte_pos, _) = char_indices[idx];

            // Look for "python " keyword
            if content[byte_pos..].starts_with("python ") {
                let block_start = byte_pos;
                idx += 7; // Skip "python " (7 chars)
                if idx >= char_indices.len() { break; }

                // Skip whitespace
                while idx < char_indices.len() {
                    let (_, ch) = char_indices[idx];
                    if !ch.is_whitespace() || ch == '\n' {
                        break;
                    }
                    idx += 1;
                }
                if idx >= char_indices.len() { break; }

                // Get function name
                let name_start = char_indices[idx].0;
                while idx < char_indices.len() {
                    let (_, ch) = char_indices[idx];
                    if ch == '(' || ch.is_whitespace() {
                        break;
                    }
                    idx += 1;
                }
                if idx >= char_indices.len() { break; }
                let name_end = char_indices[idx].0;
                let func_name = content[name_start..name_end].to_string();

                // Skip to opening brace
                while idx < char_indices.len() && char_indices[idx].1 != '{' {
                    idx += 1;
                }

                if idx < char_indices.len() {
                    idx += 1; // Skip '{'
                    if idx >= char_indices.len() { break; }

                    // Extract block body with brace matching
                    let body_start = char_indices[idx].0;
                    let mut depth = 1;
                    let mut in_string = false;
                    let mut string_char = ' ';

                    while idx < char_indices.len() && depth > 0 {
                        let (_, ch) = char_indices[idx];

                        // Handle strings (check for escape by looking at previous char)
                        let is_escaped = if idx > 0 {
                            char_indices[idx - 1].1 == '\\'
                        } else {
                            false
                        };

                        if (ch == '"' || ch == '\'') && !is_escaped {
                            if !in_string {
                                in_string = true;
                                string_char = ch;
                            } else if ch == string_char {
                                in_string = false;
                            }
                        }

                        // Count braces outside strings
                        if !in_string {
                            if ch == '{' {
                                depth += 1;
                            } else if ch == '}' {
                                depth -= 1;
                            }
                        }

                        idx += 1;
                    }

                    if idx > 0 && idx <= char_indices.len() {
                        let body_end = if idx < char_indices.len() {
                            char_indices[idx - 1].0
                        } else {
                            content.len()
                        };
                        let body = content[body_start..body_end].to_string();

                        blocks.push(PythonBlockInfo {
                            func_name: func_name.clone(),
                            is_anonymous: func_name == "__anonymous",
                            code: body,
                            start_pos: block_start,
                            end_pos: body_end,
                        });
                    }
                }
            } else {
                idx += 1;
            }
        }

        blocks
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

                let mut evaluated = None;

                // Phase 10: Try IR parser + executor first if enabled
                if self.config.use_python_ir {
                    // Extract the Python expression (remove ${@ and })
                    let python_expr = if expr.starts_with("${@") && expr.ends_with("}") {
                        &expr[3..expr.len()-1]
                    } else {
                        expr
                    };

                    let parser = PythonIRParser::new();
                    if let Some(ir) = parser.parse_inline_expression(python_expr, eval_vars.clone()) {
                        // Execute the IR based on complexity
                        match ir.execution_strategy() {
                            ExecutionStrategy::Static | ExecutionStrategy::Hybrid => {
                                let mut executor = IRExecutor::new(eval_vars.clone());
                                let ir_result = executor.execute(&ir);

                                if ir_result.success {
                                    // For inline expressions, check if any variables were modified
                                    // If so, use the first modified value as the result
                                    if let Some((_, value)) = ir_result.variables_set.iter().next() {
                                        evaluated = Some(value.clone());
                                    }
                                }
                            }
                            ExecutionStrategy::RustPython => {
                                // Fall through to SimplePythonEvaluator
                            }
                        }
                    }
                }

                // Fallback to SimplePythonEvaluator
                if evaluated.is_none() {
                    let evaluator = SimplePythonEvaluator::new(eval_vars);
                    evaluated = evaluator.evaluate(expr);
                }

                if let Some(evaluated_value) = evaluated {
                    // Replace the expression with the evaluated result
                    result.replace_range(abs_pos..abs_pos + end_pos + 1, &evaluated_value);
                    start = abs_pos + evaluated_value.len();
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
    /// Phase 7g: Now expands variable references in dependency lists
    fn parse_packageconfig(&self, content: &str) -> HashMap<String, PackageConfigOption> {
        let mut configs = HashMap::new();

        for line in content.lines() {
            let line = line.trim();

            // Look for PACKAGECONFIG[option] = "..."
            if line.starts_with("PACKAGECONFIG[")
                && let Some(bracket_end) = line.find(']') {
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

                        let enable = fields.first().unwrap_or(&"").to_string();
                        let disable = fields.get(1).unwrap_or(&"").to_string();

                        // Phase 7g: Parse build_deps field and handle variable references
                        let build_deps_str = fields.get(2).unwrap_or(&"");
                        let build_deps: Vec<String> = self.expand_packageconfig_deps(build_deps_str);

                        let runtime_deps_str = fields.get(3).unwrap_or(&"");
                        let runtime_deps: Vec<String> = self.expand_packageconfig_deps(runtime_deps_str);

                        let runtime_recommends_str = fields.get(4).unwrap_or(&"");
                        let runtime_recommends: Vec<String> = self.expand_packageconfig_deps(runtime_recommends_str);

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

        configs
    }

    /// Phase 7g: Expand variable references in PACKAGECONFIG dependency fields
    /// Handles patterns like ${PACKAGECONFIG_X11} or direct deps like "libx11"
    fn expand_packageconfig_deps(&self, deps_str: &str) -> Vec<String> {
        // For now, split by whitespace and keep both variable refs and direct deps
        // In a full implementation, we'd resolve ${PACKAGECONFIG_*} from default_variables
        deps_str
            .split_whitespace()
            .map(|dep| {
                // Phase 7g: Try to expand simple ${VAR} references from default_variables
                if dep.starts_with("${") && dep.ends_with("}") {
                    let var_name = &dep[2..dep.len() - 1];
                    self.config.default_variables
                        .get(var_name)
                        .cloned()
                        .unwrap_or_else(|| dep.to_string())
                } else {
                    dep.to_string()
                }
            })
            .filter(|s| !s.is_empty())
            .collect()
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
    /// Phase 7h: Now supports nested variable expansion with depth limiting
    fn expand_simple_variables(
        &self,
        value: &str,
        recipe_name: &str,
        variables: &HashMap<String, String>,
    ) -> String {
        // Phase 7h: Recursively expand up to depth 5 to handle nested references
        self.expand_simple_variables_recursive(value, recipe_name, variables, 0)
    }

    /// Phase 7h: Recursive helper for nested variable expansion
    fn expand_simple_variables_recursive(
        &self,
        value: &str,
        recipe_name: &str,
        variables: &HashMap<String, String>,
        depth: usize,
    ) -> String {
        // Prevent infinite loops with depth limit
        if depth >= 5 {
            return value.to_string();
        }

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
                        variables.get("PV").map(|pv| format!("{}-{}", recipe_name, pv))
                    }
                    // Enhanced variable expansion (Phase 7e)
                    // Try to get from variables first, then from default_variables, then use sensible defaults
                    "TARGET_ARCH" => variables.get("TARGET_ARCH")
                        .or_else(|| self.config.default_variables.get("TARGET_ARCH"))
                        .cloned()
                        .or_else(|| Some(self.config.build_context.arch.clone())),
                    "MACHINE" => variables.get("MACHINE")
                        .or_else(|| self.config.default_variables.get("MACHINE"))
                        .cloned(),
                    "TARGET_OS" => variables.get("TARGET_OS")
                        .or_else(|| self.config.default_variables.get("TARGET_OS"))
                        .cloned()
                        .or_else(|| Some("linux".to_string())),
                    "HOST_ARCH" => variables.get("HOST_ARCH")
                        .or_else(|| self.config.default_variables.get("HOST_ARCH"))
                        .cloned()
                        .or_else(|| Some("x86_64".to_string())),
                    "BUILD_ARCH" => variables.get("BUILD_ARCH")
                        .or_else(|| self.config.default_variables.get("BUILD_ARCH"))
                        .cloned()
                        .or_else(|| Some("x86_64".to_string())),
                    "TUNE_ARCH" => variables.get("TUNE_ARCH")
                        .or_else(|| self.config.default_variables.get("TUNE_ARCH"))
                        .cloned()
                        .or_else(|| Some(self.config.build_context.arch.clone())),
                    "TRANSLATED_TARGET_ARCH" => variables.get("TRANSLATED_TARGET_ARCH")
                        .or_else(|| self.config.default_variables.get("TRANSLATED_TARGET_ARCH"))
                        .or_else(|| variables.get("TARGET_ARCH"))
                        .cloned()
                        .or_else(|| Some(self.config.build_context.arch.clone())),
                    "MLPREFIX" => variables.get("MLPREFIX")
                        .or_else(|| self.config.default_variables.get("MLPREFIX"))
                        .cloned()
                        .or_else(|| Some("".to_string())),
                    "TARGET_PREFIX" => variables.get("TARGET_PREFIX")
                        .or_else(|| self.config.default_variables.get("TARGET_PREFIX"))
                        .cloned(),
                    "STAGING_LIBDIR" => variables.get("STAGING_LIBDIR")
                        .or_else(|| self.config.default_variables.get("STAGING_LIBDIR"))
                        .cloned(),

                    // Phase 7h: Additional standard BitBake variables
                    "WORKDIR" => variables.get("WORKDIR")
                        .or_else(|| self.config.default_variables.get("WORKDIR"))
                        .cloned(),
                    "S" => variables.get("S")
                        .or_else(|| self.config.default_variables.get("S"))
                        .cloned(),
                    "B" => variables.get("B")
                        .or_else(|| self.config.default_variables.get("B"))
                        .cloned(),
                    "D" => variables.get("D")
                        .or_else(|| self.config.default_variables.get("D"))
                        .cloned(),
                    "STAGING_DIR" => variables.get("STAGING_DIR")
                        .or_else(|| self.config.default_variables.get("STAGING_DIR"))
                        .cloned(),
                    "STAGING_DIR_HOST" => variables.get("STAGING_DIR_HOST")
                        .or_else(|| self.config.default_variables.get("STAGING_DIR_HOST"))
                        .cloned(),
                    "STAGING_DIR_TARGET" => variables.get("STAGING_DIR_TARGET")
                        .or_else(|| self.config.default_variables.get("STAGING_DIR_TARGET"))
                        .cloned(),
                    "STAGING_DIR_NATIVE" => variables.get("STAGING_DIR_NATIVE")
                        .or_else(|| self.config.default_variables.get("STAGING_DIR_NATIVE"))
                        .cloned(),
                    "STAGING_INCDIR" => variables.get("STAGING_INCDIR")
                        .or_else(|| self.config.default_variables.get("STAGING_INCDIR"))
                        .cloned(),
                    "STAGING_BINDIR" => variables.get("STAGING_BINDIR")
                        .or_else(|| self.config.default_variables.get("STAGING_BINDIR"))
                        .cloned(),
                    "STAGING_DATADIR" => variables.get("STAGING_DATADIR")
                        .or_else(|| self.config.default_variables.get("STAGING_DATADIR"))
                        .cloned(),
                    "includedir" => variables.get("includedir")
                        .or_else(|| self.config.default_variables.get("includedir"))
                        .cloned()
                        .or_else(|| Some("/usr/include".to_string())),
                    "libdir" => variables.get("libdir")
                        .or_else(|| self.config.default_variables.get("libdir"))
                        .cloned()
                        .or_else(|| Some("/usr/lib".to_string())),
                    "bindir" => variables.get("bindir")
                        .or_else(|| self.config.default_variables.get("bindir"))
                        .cloned()
                        .or_else(|| Some("/usr/bin".to_string())),
                    "datadir" => variables.get("datadir")
                        .or_else(|| self.config.default_variables.get("datadir"))
                        .cloned()
                        .or_else(|| Some("/usr/share".to_string())),

                    // Keep other variables as-is (e.g., VIRTUAL-RUNTIME_*, complex expressions, etc.)
                    _ => None,
                };

                if let Some(repl) = replacement {
                    // Phase 7h: Recursively expand the replacement in case it contains variables
                    let expanded_repl = self.expand_simple_variables_recursive(
                        &repl,
                        recipe_name,
                        variables,
                        depth + 1,
                    );

                    // Replace the variable reference
                    result.replace_range(abs_pos..abs_pos + end_pos + 3, &expanded_repl);
                    start = abs_pos + expanded_repl.len();
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
            // Try dynamic .bbclass parsing first (Phase 7b+7f), fall back to hardcoded
            let (class_build_deps, class_runtime_deps) = if !self.config.class_search_paths.is_empty() {
                // Phase 7f: Pass recipe variables for Python expression evaluation in .bbclass files
                class_dependencies::get_class_deps_dynamic(
                    &class_name,
                    distro_features,
                    &self.config.class_search_paths,
                    variables,
                )
            } else {
                // No search paths configured - use hardcoded mappings only
                (
                    class_dependencies::get_class_build_deps(&class_name, distro_features),
                    class_dependencies::get_class_runtime_deps(&class_name, distro_features),
                )
            };

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
        let mut disabled_tasks = std::collections::HashSet::new();

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

            // Parse deltask statements (must be before addtask to handle re-add cases)
            if let Some(task_name) = parse_deltask_statement(line) {
                disabled_tasks.insert(task_name);
            }

            // Parse addtask statements
            if let Some(task) = parse_addtask_statement(line) {
                // Check if task already exists (from inherited classes)
                let task_id = if let Some(existing_id) = graph.find_task(recipe_id, &task.name) {
                    existing_id
                } else {
                    graph.add_task(recipe_id, &task.name)
                };

                // Only add to task_names if it's a new task
                if !task_names.contains(&task.name) {
                    task_names.push(task.name.clone());
                }

                task_constraints.push((task_id, task.after, task.before));
            }

            // Parse task flags
            if let Some((task_name, flag_name, value)) = parse_task_flag(line) {
                task_flags.push((task_name, flag_name, value));
            }
        }

        // Debug: Count busybox task constraints
        let busybox_constraints = if let Some(recipe) = graph.get_recipe(recipe_id) {
            if recipe.name == "busybox" {
                debug!("Processing {} task constraints for busybox", task_constraints.len());
                true
            } else {
                false
            }
        } else {
            false
        };

        // Apply constraints (resolve names to IDs first, then apply)
        for (task_id, after_names, before_names) in task_constraints {
            // Resolve names to task IDs, trying both with and without "do_" prefix
            let after_ids: Vec<TaskId> = after_names
                .iter()
                .filter_map(|name| {
                    graph.find_task(recipe_id, name).or_else(|| {
                        // Try stripping "do_" prefix if present
                        if let Some(stripped) = name.strip_prefix("do_") {
                            graph.find_task(recipe_id, stripped)
                        } else {
                            // Try adding "do_" prefix
                            graph.find_task(recipe_id, &format!("do_{}", name))
                        }
                    })
                })
                .collect();
            let before_ids: Vec<TaskId> = before_names
                .iter()
                .filter_map(|name| {
                    graph.find_task(recipe_id, name).or_else(|| {
                        // Try stripping "do_" prefix if present
                        if let Some(stripped) = name.strip_prefix("do_") {
                            graph.find_task(recipe_id, stripped)
                        } else {
                            // Try adding "do_" prefix
                            graph.find_task(recipe_id, &format!("do_{}", name))
                        }
                    })
                })
                .collect();

            // Now apply them
            // Get recipe info before mutable borrow
            let debug_info = if let Some(task) = graph.get_task(task_id) {
                graph.get_recipe(task.recipe_id).map(|r| (r.name.clone(), task.name.clone()))
            } else {
                None
            };

            if let Some(task_node) = graph.get_task_mut(task_id) {
                let after_count_before = task_node.after.len();

                task_node.after.extend(after_ids.clone());
                task_node.before.extend(before_ids);

                // Debug: Log if busybox task is getting dependencies
                if busybox_constraints {
                    if let Some((recipe_name, task_name)) = debug_info {
                        debug!("  {} after: {} -> {} (added {} from {:?})",
                            task_name, after_count_before, task_node.after.len(),
                            after_ids.len(), after_names);
                    }
                }
            }
        }

        // Apply flags
        for (task_name, flag_name, value) in task_flags {
            if let Some(task_id) = graph.find_task(recipe_id, &task_name)
                && let Some(task_node) = graph.get_task_mut(task_id) {
                    task_node.flags.insert(flag_name, value);
                }
        }

        // Filter out disabled tasks (those removed by deltask)
        // Need to handle both "do_taskname" and "taskname" formats
        task_names.retain(|name| {
            let task_with_do = format!("do_{}", name);
            !disabled_tasks.contains(name) && !disabled_tasks.contains(&task_with_do)
        });

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
        let mut processed_classes = std::collections::HashSet::new();

        // Automatically include base.bbclass (BitBake inherits it for all recipes)
        self.extract_tasks_from_class("base", recipe_path, &mut class_content, &mut processed_classes);

        // Parse explicit inherit statements
        for line in content.lines() {
            let trimmed = line.trim();

            if let Some(classes) = self.parse_inherit_statement(trimmed) {
                // Process each class
                for class_name in classes {
                    self.extract_tasks_from_class(class_name, recipe_path, &mut class_content, &mut processed_classes);
                }
            }
        }

        // Append class tasks to the resolved content
        if !class_content.is_empty() {
            let task_count = class_content.lines().filter(|l| l.trim().starts_with("addtask")).count();
            debug!("Appending {} addtask statements from inherited classes", task_count);

            // Debug: For busybox, show what we're appending
            if recipe_path.file_name().and_then(|n| n.to_str()).map(|n| n.contains("busybox")).unwrap_or(false) {
                debug!("busybox class content (first 5 addtask lines):");
                for (i, line) in class_content.lines().filter(|l| l.trim().starts_with("addtask")).take(5).enumerate() {
                    debug!("  [{}] {}", i, line.trim());
                }
            }

            resolved.push_str("\n# Tasks from inherited classes (base + explicit)\n");
            resolved.push_str(&class_content);
        } else {
            debug!("No tasks found from inherited classes");
        }

        Ok(resolved)
    }

    /// Recursively extract tasks from a class and its inherited classes
    fn extract_tasks_from_class(
        &self,
        class_name: &str,
        recipe_path: &Path,
        output: &mut String,
        processed: &mut std::collections::HashSet<String>,
    ) {
        // Skip if already processed (avoid circular dependencies)
        if processed.contains(class_name) {
            return;
        }
        processed.insert(class_name.to_string());

        if let Some(class_path) = self.find_class_file(class_name, recipe_path) {
            if let Ok(class_content) = std::fs::read_to_string(&class_path) {
                // First, recursively process inherited classes
                for line in class_content.lines() {
                    let trimmed = line.trim();
                    if let Some(inherited_classes) = self.parse_inherit_statement(trimmed) {
                        for inherited_class in inherited_classes {
                            self.extract_tasks_from_class(inherited_class, recipe_path, output, processed);
                        }
                    }
                }

                // Then extract addtask statements from this class
                for line in class_content.lines() {
                    let line_trimmed = line.trim();
                    if line_trimmed.starts_with("addtask ") ||
                       (line_trimmed.starts_with("do_") && line_trimmed.contains('[')) {
                        output.push_str(line);
                        output.push('\n');
                    }
                }
            }
        }
    }

    /// Parse inherit statement
    fn parse_inherit_statement<'a>(&self, line: &'a str) -> Option<Vec<&'a str>> {
        let trimmed = line.trim();

        trimmed.strip_prefix("inherit ").map(|rest| rest.split_whitespace().collect())
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

    // Phase 9c: Variable flags tests

    #[test]
    fn test_parse_variable_flags() {
        let extractor = RecipeExtractor::new_default();

        let content = r#"
SRC_URI[md5sum] = "abc123"
SRC_URI[sha256sum] = "def456"
do_compile[depends] = "virtual/kernel:do_shared_workdir"
do_install[cleandirs] = "${D}"
PACKAGECONFIG[ssl] = "--enable-ssl,--disable-ssl,openssl"
"#;

        let flags = extractor.parse_variable_flags(content);

        // Check SRC_URI flags
        assert!(flags.contains_key("SRC_URI"));
        let src_uri_flags = &flags["SRC_URI"];
        assert_eq!(src_uri_flags.get("md5sum"), Some(&"abc123".to_string()));
        assert_eq!(src_uri_flags.get("sha256sum"), Some(&"def456".to_string()));

        // Check do_compile flags
        assert!(flags.contains_key("do_compile"));
        let compile_flags = &flags["do_compile"];
        assert_eq!(
            compile_flags.get("depends"),
            Some(&"virtual/kernel:do_shared_workdir".to_string())
        );

        // Check do_install flags
        assert!(flags.contains_key("do_install"));
        let install_flags = &flags["do_install"];
        assert_eq!(install_flags.get("cleandirs"), Some(&"${D}".to_string()));

        // Check PACKAGECONFIG flags
        assert!(flags.contains_key("PACKAGECONFIG"));
        let pkg_flags = &flags["PACKAGECONFIG"];
        assert_eq!(
            pkg_flags.get("ssl"),
            Some(&"--enable-ssl,--disable-ssl,openssl".to_string())
        );
    }

    #[test]
    fn test_extract_task_flag_depends() {
        let extractor = RecipeExtractor::new_default();

        let mut flags: HashMap<String, HashMap<String, String>> = HashMap::new();

        // Single dependency
        let mut do_compile = HashMap::new();
        do_compile.insert(
            "depends".to_string(),
            "virtual/kernel:do_shared_workdir".to_string(),
        );
        flags.insert("do_compile".to_string(), do_compile);

        // Multiple dependencies
        let mut do_install = HashMap::new();
        do_install.insert(
            "depends".to_string(),
            "recipe-a:do_populate_sysroot recipe-b:do_configure".to_string(),
        );
        flags.insert("do_install".to_string(), do_install);

        // Non-task flag (should be ignored)
        let mut src_uri = HashMap::new();
        src_uri.insert("md5sum".to_string(), "abc123".to_string());
        flags.insert("SRC_URI".to_string(), src_uri);

        let task_depends = extractor.extract_task_flag_depends(&flags);

        // Should extract recipe names from task dependencies
        assert!(task_depends.contains(&"virtual/kernel".to_string()));
        assert!(task_depends.contains(&"recipe-a".to_string()));
        assert!(task_depends.contains(&"recipe-b".to_string()));
        assert_eq!(task_depends.len(), 3);
    }

    #[test]
    fn test_variable_flags_integration() {
        let mut graph = RecipeGraph::new();
        let extractor = RecipeExtractor::new_default();

        let content = r#"
SUMMARY = "Test recipe with task dependencies"
PV = "1.0"
DEPENDS = "base-dep"

# Task-level dependencies
do_compile[depends] = "virtual/kernel:do_shared_workdir"
do_install[depends] = "other-recipe:do_populate_sysroot"

# Other flags (non-dependency)
SRC_URI[md5sum] = "abc123"
PACKAGECONFIG[feature] = "--enable-feature,--disable-feature,feature-dep"
"#;

        let extraction = extractor
            .extract_from_content(&mut graph, "test-recipe", content)
            .unwrap();

        // Check that task dependencies were extracted
        assert!(extraction.depends.contains(&"base-dep".to_string()));
        assert!(extraction.depends.contains(&"virtual/kernel".to_string()));
        assert!(extraction.depends.contains(&"other-recipe".to_string()));

        // Check variable flags were captured
        assert!(extraction.variable_flags.contains_key("SRC_URI"));
        assert!(extraction.variable_flags.contains_key("do_compile"));
        assert!(extraction.variable_flags.contains_key("do_install"));
        assert!(extraction.variable_flags.contains_key("PACKAGECONFIG"));
    }

    #[test]
    fn test_real_world_task_dependencies() {
        let mut graph = RecipeGraph::new();
        let extractor = RecipeExtractor::new_default();

        // Real-world pattern from linux-yocto recipe
        let content = r#"
SUMMARY = "Linux Kernel"
DEPENDS = "bc-native bison-native"

do_compile[depends] += "virtual/${TARGET_PREFIX}gcc:do_populate_sysroot"
do_compile[depends] += "virtual/${TARGET_PREFIX}binutils:do_populate_sysroot"
do_install[depends] = "depmodwrapper-cross:do_populate_sysroot"
"#;

        let extraction = extractor
            .extract_from_content(&mut graph, "linux-yocto", content)
            .unwrap();

        // Check base dependencies
        assert!(extraction.depends.contains(&"bc-native".to_string()));
        assert!(extraction.depends.contains(&"bison-native".to_string()));

        // Check task-level dependencies were extracted
        // Note: variable expansion isn't done in this test, so we get literal ${...}
        assert!(extraction
            .depends
            .iter()
            .any(|d| d.contains("gcc") || d == "depmodwrapper-cross"));
    }

    // Phase 9b: Enhanced override resolution tests

    #[test]
    fn test_parse_assignment_with_chained_override() {
        let extractor = RecipeExtractor::new_default();

        // Test DEPENDS:append:qemux86 = "foo"
        let result = extractor.parse_assignment("DEPENDS:append:qemux86 = \"foo\"");
        assert!(result.is_some());
        let (var_name, operator, override_suffix, value) = result.unwrap();
        assert_eq!(var_name, "DEPENDS");
        assert_eq!(operator, ":append");
        assert_eq!(override_suffix, Some("qemux86".to_string()));
        assert_eq!(value, "foo");

        // Test RDEPENDS:prepend:class-native = "bar"
        let result = extractor.parse_assignment("RDEPENDS:prepend:class-native = \"bar\"");
        assert!(result.is_some());
        let (var_name, operator, override_suffix, value) = result.unwrap();
        assert_eq!(var_name, "RDEPENDS");
        assert_eq!(operator, ":prepend");
        assert_eq!(override_suffix, Some("class-native".to_string()));
        assert_eq!(value, "bar");

        // Test DEPENDS:remove:arm = "baz"
        let result = extractor.parse_assignment("DEPENDS:remove:arm = \"baz\"");
        assert!(result.is_some());
        let (var_name, operator, override_suffix, value) = result.unwrap();
        assert_eq!(var_name, "DEPENDS");
        assert_eq!(operator, ":remove");
        assert_eq!(override_suffix, Some("arm".to_string()));
        assert_eq!(value, "baz");
    }

    #[test]
    fn test_parse_assignment_with_simple_override() {
        let extractor = RecipeExtractor::new_default();

        // Test DEPENDS:qemux86 = "foo"
        let result = extractor.parse_assignment("DEPENDS:qemux86 = \"foo\"");
        assert!(result.is_some());
        let (var_name, operator, override_suffix, value) = result.unwrap();
        assert_eq!(var_name, "DEPENDS");
        assert_eq!(operator, "=");
        assert_eq!(override_suffix, Some("qemux86".to_string()));
        assert_eq!(value, "foo");

        // Test RDEPENDS:class-native = "bar"
        let result = extractor.parse_assignment("RDEPENDS:class-native = \"bar\"");
        assert!(result.is_some());
        let (var_name, operator, override_suffix, value) = result.unwrap();
        assert_eq!(var_name, "RDEPENDS");
        assert_eq!(operator, "=");
        assert_eq!(override_suffix, Some("class-native".to_string()));
        assert_eq!(value, "bar");
    }

    #[test]
    fn test_chained_override_integration() {
        let mut graph = RecipeGraph::new();

        // Test with qemux86 context
        let config = ExtractionConfig {
            build_context: BuildContext {
                class: "target".to_string(),
                libc: "glibc".to_string(),
                arch: "x86-64".to_string(),
                overrides: vec!["qemux86".to_string()],
            },
            ..Default::default()
        };
        let extractor = RecipeExtractor::new(config);

        let content = r#"
SUMMARY = "Test recipe with chained overrides"
PV = "1.0"
DEPENDS = "base-dep"
DEPENDS:append = " append-dep"
DEPENDS:append:qemux86 = " qemu-specific-dep"
DEPENDS:qemux86 = "qemu-only-dep"
"#;

        let extraction = extractor
            .extract_from_content(&mut graph, "test-recipe", content)
            .unwrap();

        // Should contain dependencies from active overrides
        assert!(extraction.depends.contains(&"base-dep".to_string()));
        assert!(extraction.depends.contains(&"append-dep".to_string()));
        // qemux86 override should be active
        assert!(extraction.depends.contains(&"qemu-specific-dep".to_string()));
        assert!(extraction.depends.contains(&"qemu-only-dep".to_string()));
    }

    #[test]
    fn test_class_override_resolution() {
        let mut graph = RecipeGraph::new();

        // Test with native context
        let config = ExtractionConfig {
            build_context: BuildContext {
                class: "native".to_string(),
                libc: "glibc".to_string(),
                arch: "x86-64".to_string(),
                overrides: Vec::new(),
            },
            ..Default::default()
        };
        let extractor = RecipeExtractor::new(config);

        let content = r#"
SUMMARY = "Test recipe with class overrides"
PV = "1.0"
DEPENDS = "common-dep"
DEPENDS:append:class-native = " native-dep"
DEPENDS:append:class-target = " target-dep"
"#;

        let extraction = extractor
            .extract_from_content(&mut graph, "test-recipe", content)
            .unwrap();

        // Should contain common and native-specific deps
        assert!(extraction.depends.contains(&"common-dep".to_string()));
        assert!(extraction.depends.contains(&"native-dep".to_string()));
        // Should include target deps too (inclusive approach for dependency extraction)
        assert!(extraction.depends.contains(&"target-dep".to_string()));
    }

    #[test]
    fn test_real_world_override_patterns() {
        let mut graph = RecipeGraph::new();

        // Real-world pattern from actual BitBake recipes
        let config = ExtractionConfig {
            build_context: BuildContext {
                class: "target".to_string(),
                libc: "glibc".to_string(),
                arch: "aarch64".to_string(),
                overrides: vec!["raspberrypi4".to_string()],
            },
            ..Default::default()
        };
        let extractor = RecipeExtractor::new(config);

        let content = r#"
SUMMARY = "Real-world recipe with multiple overrides"
PV = "1.0"

# Base dependencies
DEPENDS = "virtual/kernel"

# Architecture-specific
DEPENDS:append:arm = " arm-specific"
DEPENDS:append:aarch64 = " aarch64-specific"

# Machine-specific
DEPENDS:append:raspberrypi4 = " rpi4-hardware"

# Class-specific
DEPENDS:append:class-target = " target-only"

# Distro-specific
DEPENDS:append:poky = " poky-distro"
"#;

        let extraction = extractor
            .extract_from_content(&mut graph, "test-recipe", content)
            .unwrap();

        // Check base dependency
        assert!(extraction.depends.contains(&"virtual/kernel".to_string()));

        // Check architecture-specific (should have aarch64)
        assert!(extraction.depends.contains(&"aarch64-specific".to_string()));

        // Check machine-specific (should have rpi4)
        assert!(extraction.depends.contains(&"rpi4-hardware".to_string()));

        // Check class-specific (should have target)
        assert!(extraction.depends.contains(&"target-only".to_string()));
    }

    #[test]
    fn test_python_block_processing() {
        let mut graph = RecipeGraph::new();

        let mut config = ExtractionConfig::default();
        config.use_python_ir = true;

        let extractor = RecipeExtractor::new(config);

        let content = r#"
PN = "test-recipe"
PV = "1.0"
DEPENDS = "base-dep"

python __anonymous() {
    d.setVar('EXTRA_DEP', 'python-added')
    d.appendVar('DEPENDS', ' python-added')
}

RDEPENDS = "runtime-dep"
"#;

        let extraction = extractor
            .extract_from_content(&mut graph, "test-recipe", content)
            .unwrap();

        // Check that Python block was processed and variable was set
        assert!(extraction.variables.contains_key("EXTRA_DEP"));
        assert_eq!(extraction.variables.get("EXTRA_DEP"), Some(&"python-added".to_string()));

        // Check that DEPENDS was modified by Python block
        assert!(extraction.depends.contains(&"base-dep".to_string()));
        assert!(extraction.depends.contains(&"python-added".to_string()));
    }

    #[test]
    fn test_extract_python_blocks() {
        let extractor = RecipeExtractor::new_default();

        let content = r#"
DEPENDS = "foo"

python __anonymous() {
    d.setVar('TEST', 'value')
}

python do_configure() {
    # Some task code
    pass
}

RDEPENDS = "bar"
"#;

        let blocks = extractor.extract_python_blocks(content);

        // Should find 2 Python blocks
        assert_eq!(blocks.len(), 2);

        // First should be anonymous
        assert_eq!(blocks[0].func_name, "__anonymous");
        assert!(blocks[0].is_anonymous);
        assert!(blocks[0].code.contains("d.setVar"));

        // Second should be do_configure
        assert_eq!(blocks[1].func_name, "do_configure");
        assert!(!blocks[1].is_anonymous);
        assert!(blocks[1].code.contains("pass"));
    }
}


