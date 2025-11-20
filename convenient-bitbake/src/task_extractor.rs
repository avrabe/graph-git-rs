//! Task implementation extractor for BitBake recipes
//!
//! Extracts actual task implementation code (shell scripts and Python functions)
//! from BitBake recipe files.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Type of task implementation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskImplementationType {
    /// Shell script (bash)
    Shell,
    /// Python function
    Python,
    /// Fakeroot shell script
    FakerootShell,
}

/// A task implementation extracted from a recipe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskImplementation {
    /// Task name (e.g., "compile" not "do_compile" - normalized without "do_" prefix)
    pub name: String,
    /// Type of implementation
    pub impl_type: TaskImplementationType,
    /// The actual code
    pub code: String,
    /// Line number where it starts
    pub line_number: usize,
    /// Override suffix (e.g., ":append", ":prepend", ":machine")
    pub override_suffix: Option<String>,
}

/// Complete set of implementations extracted from a recipe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeImplementations {
    /// Task implementations (functions starting with do_)
    pub tasks: HashMap<String, TaskImplementation>,
    /// Helper functions (non-task shell functions)
    pub helpers: HashMap<String, TaskImplementation>,
}

/// Normalize task name by removing "do_" prefix if present
/// This ensures consistency with task_parser.rs normalization
fn normalize_task_name(name: &str) -> String {
    name.strip_prefix("do_").unwrap_or(name).to_string()
}

/// Extractor for task implementations
pub struct TaskExtractor {
    /// Regex for shell functions: do_taskname() {
    shell_func_regex: Regex,
    /// Regex for python functions: python do_taskname() {
    python_func_regex: Regex,
    /// Regex for fakeroot shell functions: fakeroot do_taskname() {
    fakeroot_func_regex: Regex,
    /// Regex for helper shell functions: funcname() {
    helper_func_regex: Regex,
}

impl Default for TaskExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskExtractor {
    pub fn new() -> Self {
        // Match: do_taskname() { or do_taskname:append() {
        let shell_func_regex = Regex::new(
            r"^(do_\w+)(\:[a-zA-Z_]+)?\s*\(\s*\)\s*\{"
        ).unwrap();

        // Match: python do_taskname() { or python do_taskname:prepend() {
        let python_func_regex = Regex::new(
            r"^python\s+(do_\w+)(\:[a-zA-Z_]+)?\s*\(\s*\)\s*\{"
        ).unwrap();

        // Match: fakeroot do_taskname() {
        let fakeroot_func_regex = Regex::new(
            r"^fakeroot\s+(do_\w+)(\:[a-zA-Z_]+)?\s*\(\s*\)\s*\{"
        ).unwrap();

        // Match: any shell function that's not a task: funcname() {
        // Exclude do_* (tasks), python/fakeroot (handled above), and python functions
        let helper_func_regex = Regex::new(
            r"^([a-z_][a-z0-9_]*)\s*\(\s*\)\s*\{"
        ).unwrap();

        Self {
            shell_func_regex,
            python_func_regex,
            fakeroot_func_regex,
            helper_func_regex,
        }
    }

    /// Extract all implementations (tasks and helpers) from recipe content
    pub fn extract_all_from_content(&self, content: &str) -> RecipeImplementations {
        let mut tasks = HashMap::new();
        let mut helpers = HashMap::new();
        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // Try to match shell task function (do_*)
            if let Some(caps) = self.shell_func_regex.captures(line) {
                let raw_task_name = caps.get(1).unwrap().as_str();
                let task_name = normalize_task_name(raw_task_name);
                let override_suffix = caps.get(2).map(|m| m.as_str().to_string());

                let (code, end_line) = self.extract_function_body(&lines, i);

                let key = if let Some(ref suffix) = override_suffix {
                    format!("{}{}", task_name, suffix)
                } else {
                    task_name.clone()
                };

                tasks.insert(key, TaskImplementation {
                    name: task_name,
                    impl_type: TaskImplementationType::Shell,
                    code,
                    line_number: i + 1,
                    override_suffix,
                });

                i = end_line + 1;
                continue;
            }

            // Try to match python task function
            if let Some(caps) = self.python_func_regex.captures(line) {
                let raw_task_name = caps.get(1).unwrap().as_str();
                let task_name = normalize_task_name(raw_task_name);
                let override_suffix = caps.get(2).map(|m| m.as_str().to_string());

                let (code, end_line) = self.extract_function_body(&lines, i);

                let key = if let Some(ref suffix) = override_suffix {
                    format!("{}{}", task_name, suffix)
                } else {
                    task_name.clone()
                };

                tasks.insert(key, TaskImplementation {
                    name: task_name,
                    impl_type: TaskImplementationType::Python,
                    code,
                    line_number: i + 1,
                    override_suffix,
                });

                i = end_line + 1;
                continue;
            }

            // Try to match fakeroot task function
            if let Some(caps) = self.fakeroot_func_regex.captures(line) {
                let raw_task_name = caps.get(1).unwrap().as_str();
                let task_name = normalize_task_name(raw_task_name);
                let override_suffix = caps.get(2).map(|m| m.as_str().to_string());

                let (code, end_line) = self.extract_function_body(&lines, i);

                let key = if let Some(ref suffix) = override_suffix {
                    format!("{}{}", task_name, suffix)
                } else {
                    task_name.clone()
                };

                tasks.insert(key, TaskImplementation {
                    name: task_name,
                    impl_type: TaskImplementationType::FakerootShell,
                    code,
                    line_number: i + 1,
                    override_suffix,
                });

                i = end_line + 1;
                continue;
            }

            // Try to match helper function (non-task shell function)
            // Check it doesn't start with "do_" or "python" or "fakeroot"
            if !line.starts_with("do_")
                && !line.starts_with("python ")
                && !line.starts_with("fakeroot ") {
                if let Some(caps) = self.helper_func_regex.captures(line) {
                    let func_name = caps.get(1).unwrap().as_str().to_string();
                    let (code, end_line) = self.extract_function_body(&lines, i);

                    helpers.insert(func_name.clone(), TaskImplementation {
                        name: func_name,
                        impl_type: TaskImplementationType::Shell,
                        code,
                        line_number: i + 1,
                        override_suffix: None,
                    });

                    i = end_line + 1;
                    continue;
                }
            }

            i += 1;
        }

        RecipeImplementations { tasks, helpers }
    }

    /// Extract all task implementations from recipe content (legacy method)
    pub fn extract_from_content(&self, content: &str) -> HashMap<String, TaskImplementation> {
        let mut tasks = HashMap::new();
        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // Try to match shell function
            if let Some(caps) = self.shell_func_regex.captures(line) {
                let raw_task_name = caps.get(1).unwrap().as_str();
                let task_name = normalize_task_name(raw_task_name);
                let override_suffix = caps.get(2).map(|m| m.as_str().to_string());

                let (code, end_line) = self.extract_function_body(&lines, i);

                let key = if let Some(ref suffix) = override_suffix {
                    format!("{}{}", task_name, suffix)
                } else {
                    task_name.clone()
                };

                tasks.insert(key, TaskImplementation {
                    name: task_name,
                    impl_type: TaskImplementationType::Shell,
                    code,
                    line_number: i + 1,
                    override_suffix,
                });

                i = end_line + 1;
                continue;
            }

            // Try to match python function
            if let Some(caps) = self.python_func_regex.captures(line) {
                let raw_task_name = caps.get(1).unwrap().as_str();
                let task_name = normalize_task_name(raw_task_name);
                let override_suffix = caps.get(2).map(|m| m.as_str().to_string());

                let (code, end_line) = self.extract_function_body(&lines, i);

                let key = if let Some(ref suffix) = override_suffix {
                    format!("{}{}", task_name, suffix)
                } else {
                    task_name.clone()
                };

                tasks.insert(key, TaskImplementation {
                    name: task_name,
                    impl_type: TaskImplementationType::Python,
                    code,
                    line_number: i + 1,
                    override_suffix,
                });

                i = end_line + 1;
                continue;
            }

            // Try to match fakeroot function
            if let Some(caps) = self.fakeroot_func_regex.captures(line) {
                let raw_task_name = caps.get(1).unwrap().as_str();
                let task_name = normalize_task_name(raw_task_name);
                let override_suffix = caps.get(2).map(|m| m.as_str().to_string());

                let (code, end_line) = self.extract_function_body(&lines, i);

                let key = if let Some(ref suffix) = override_suffix {
                    format!("{}{}", task_name, suffix)
                } else {
                    task_name.clone()
                };

                tasks.insert(key, TaskImplementation {
                    name: task_name,
                    impl_type: TaskImplementationType::FakerootShell,
                    code,
                    line_number: i + 1,
                    override_suffix,
                });

                i = end_line + 1;
                continue;
            }

            i += 1;
        }

        tasks
    }

    /// Extract function body from opening { to closing }
    /// Returns (function_body, end_line_index)
    fn extract_function_body(&self, lines: &[&str], start_line: usize) -> (String, usize) {
        let mut body = Vec::new();
        let mut brace_count = 0;
        let mut started = false;
        let mut i = start_line;

        while i < lines.len() {
            let line = lines[i];

            // Count braces
            for ch in line.chars() {
                match ch {
                    '{' => {
                        brace_count += 1;
                        started = true;
                    }
                    '}' => {
                        brace_count -= 1;
                        if started && brace_count == 0 {
                            // Function complete
                            return (body.join("\n"), i);
                        }
                    }
                    _ => {}
                }
            }

            // Add line to body (skip the opening line with function declaration)
            if i > start_line && brace_count > 0 {
                body.push(line);
            }

            i += 1;
        }

        // Function body not properly closed, return what we have
        (body.join("\n"), lines.len() - 1)
    }

    /// Extract task implementations from a recipe file
    pub fn extract_from_file(&self, path: &Path) -> Result<HashMap<String, TaskImplementation>, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        Ok(self.extract_from_content(&content))
    }

    /// Merge task implementations (handle :append, :prepend overrides)
    pub fn merge_implementations(
        &self,
        base: &HashMap<String, TaskImplementation>,
        overrides: &HashMap<String, TaskImplementation>,
    ) -> HashMap<String, TaskImplementation> {
        let mut merged = base.clone();

        for (key, override_impl) in overrides {
            if let Some(suffix) = &override_impl.override_suffix {
                // Extract base task name
                let base_name = &override_impl.name;

                if suffix == ":append" {
                    // Append to existing implementation
                    if let Some(base_impl) = merged.get_mut(base_name) {
                        base_impl.code.push('\n');
                        base_impl.code.push_str(&override_impl.code);
                    } else {
                        // No base, just use the append
                        merged.insert(base_name.clone(), override_impl.clone());
                    }
                } else if suffix == ":prepend" {
                    // Prepend to existing implementation
                    if let Some(base_impl) = merged.get_mut(base_name) {
                        let mut new_code = override_impl.code.clone();
                        new_code.push('\n');
                        new_code.push_str(&base_impl.code);
                        base_impl.code = new_code;
                    } else {
                        // No base, just use the prepend
                        merged.insert(base_name.clone(), override_impl.clone());
                    }
                } else {
                    // Other overrides (e.g., :machine) replace entirely
                    merged.insert(key.clone(), override_impl.clone());
                }
            } else {
                // No override suffix, replace entirely
                merged.insert(key.clone(), override_impl.clone());
            }
        }

        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_shell_task() {
        let content = r#"
do_compile() {
    oe_runmake
    echo "Compilation complete"
}

do_install() {
    install -d ${D}${bindir}
    install -m 0755 hello ${D}${bindir}
}
"#;

        let extractor = TaskExtractor::new();
        let tasks = extractor.extract_from_content(content);

        assert_eq!(tasks.len(), 2);
        assert!(tasks.contains_key("do_compile"));
        assert!(tasks.contains_key("do_install"));

        let compile = &tasks["do_compile"];
        assert_eq!(compile.name, "do_compile");
        assert_eq!(compile.impl_type, TaskImplementationType::Shell);
        assert!(compile.code.contains("oe_runmake"));
    }

    #[test]
    fn test_extract_python_task() {
        let content = r#"
python do_package:prepend () {
    d.setVar('PACKAGES', 'package1 package2')
    bb.note("Packaging...")
}
"#;

        let extractor = TaskExtractor::new();
        let tasks = extractor.extract_from_content(content);

        assert_eq!(tasks.len(), 1);
        let task = tasks.values().next().unwrap();
        assert_eq!(task.name, "do_package");
        assert_eq!(task.impl_type, TaskImplementationType::Python);
        assert!(task.code.contains("setVar"));
    }

    #[test]
    fn test_merge_append() {
        let extractor = TaskExtractor::new();

        let base_content = r#"
do_install() {
    echo "Base install"
}
"#;

        let append_content = r#"
do_install:append() {
    echo "Additional install step"
}
"#;

        let base_tasks = extractor.extract_from_content(base_content);
        let append_tasks = extractor.extract_from_content(append_content);
        let merged = extractor.merge_implementations(&base_tasks, &append_tasks);

        assert!(merged.contains_key("do_install"));
        let install_task = &merged["do_install"];
        assert!(install_task.code.contains("Base install"));
        assert!(install_task.code.contains("Additional install step"));
    }
}
