// Simple variable resolver for BitBake recipes
// Handles basic variable expansion without full BitBake execution

use crate::BitbakeRecipe;
use regex::Regex;
use std::collections::HashMap;

/// Simple variable resolver that expands ${VAR} references
/// Good for 80%+ of real-world cases
pub struct SimpleResolver {
    variables: HashMap<String, String>,
}

impl SimpleResolver {
    /// Create a new resolver from a recipe
    /// Initializes with recipe variables and common built-ins
    pub fn new(recipe: &BitbakeRecipe) -> Self {
        let mut variables = HashMap::new();

        // Load all recipe variables
        for (name, value) in &recipe.variables {
            variables.insert(name.clone(), value.clone());
        }

        // Add common built-in variables if not already present
        if let Some(pn) = &recipe.package_name {
            variables.entry("PN".to_string()).or_insert(pn.clone());

            // BPN is PN without version suffix (e.g., "mypackage-1.0" -> "mypackage")
            // In practice, often BPN == PN for recipes
            let bpn = if pn.contains('-') {
                // Try to split on '-' and take everything before the last part if it looks like a version
                let parts: Vec<&str> = pn.split('-').collect();
                if parts.len() > 1 {
                    let last = parts.last().unwrap();
                    // Check if last part looks like a version (starts with digit)
                    if last.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        parts[..parts.len() - 1].join("-")
                    } else {
                        pn.clone()
                    }
                } else {
                    pn.clone()
                }
            } else {
                pn.clone()
            };
            variables.entry("BPN".to_string()).or_insert(bpn);
        }

        // PV (package version) - use recipe.package_version if available
        // Otherwise try to extract from package name
        if let Some(pv) = &recipe.package_version {
            variables.entry("PV".to_string()).or_insert(pv.clone());
        } else if let Some(pn) = &recipe.package_name {
            // Fallback: try to extract version from package name
            // Find the first '-' or '_' followed by a digit
            for (i, c) in pn.char_indices() {
                if (c == '-' || c == '_') && i + 1 < pn.len() {
                    let rest = &pn[i + 1..];
                    if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        variables
                            .entry("PV".to_string())
                            .or_insert(rest.to_string());
                        break;
                    }
                }
            }
        }

        // Add BP (base package = ${BPN}-${PV})
        if variables.contains_key("BPN") && variables.contains_key("PV") {
            let bp = format!(
                "{}-{}",
                variables.get("BPN").unwrap(),
                variables.get("PV").unwrap()
            );
            variables.entry("BP".to_string()).or_insert(bp);
        }

        // Common directory defaults (these are usually overridden by conf files)
        variables
            .entry("WORKDIR".to_string())
            .or_insert("/tmp/work".to_string());
        variables
            .entry("S".to_string())
            .or_insert("${WORKDIR}/git".to_string());
        variables
            .entry("B".to_string())
            .or_insert("${S}".to_string());
        variables
            .entry("D".to_string())
            .or_insert("${WORKDIR}/image".to_string());

        Self { variables }
    }

    /// Add or override a variable
    pub fn set(&mut self, name: String, value: String) {
        self.variables.insert(name, value);
    }

    /// Get a variable value (unexpanded)
    pub fn get(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(|s| s.as_str())
    }

    /// Resolve a string by expanding all ${VAR} references
    /// Performs iterative expansion up to MAX_DEPTH iterations
    pub fn resolve(&self, input: &str) -> String {
        let var_regex = Regex::new(r"\$\{([^}]+)\}").unwrap();
        let mut result = input.to_string();
        let mut depth = 0;
        const MAX_DEPTH: usize = 10;

        while depth < MAX_DEPTH {
            let before = result.clone();
            let mut changed = false;

            // Find all variable references in current string
            let caps: Vec<_> = var_regex.captures_iter(&before).collect();

            for cap in caps {
                let full_match = cap.get(0).unwrap().as_str();
                let var_name = cap.get(1).unwrap().as_str();

                // Handle special syntax like ${VAR:-default}
                let (actual_var, default) = if var_name.contains(":-") {
                    let parts: Vec<&str> = var_name.splitn(2, ":-").collect();
                    (parts[0], Some(parts[1]))
                } else {
                    (var_name, None)
                };

                // Resolve the variable
                if let Some(value) = self.variables.get(actual_var) {
                    result = result.replace(full_match, value);
                    changed = true;
                } else if let Some(default_val) = default {
                    result = result.replace(full_match, default_val);
                    changed = true;
                } else {
                    // Variable not found - leave as-is
                    // This is expected for variables resolved at BitBake runtime
                }
            }

            if !changed || result == before {
                break; // No more expansions possible
            }

            depth += 1;
        }

        result
    }

    /// Resolve and split a space-separated list (like SRC_URI)
    pub fn resolve_list(&self, input: &str) -> Vec<String> {
        let resolved = self.resolve(input);

        // Split on whitespace, handling line continuations
        resolved
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    }

    /// Get all variables (for debugging)
    pub fn variables(&self) -> &HashMap<String, String> {
        &self.variables
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecipeType;
    use std::path::PathBuf;

    #[test]
    fn test_simple_expansion() {
        let mut recipe = BitbakeRecipe {
            file_path: PathBuf::from("/tmp/test.bb"),
            recipe_type: RecipeType::Recipe,
            package_name: Some("mypackage-1.0".to_string()),
            ..Default::default()
        };

        recipe
            .variables
            .insert("FOO".to_string(), "hello".to_string());
        recipe
            .variables
            .insert("BAR".to_string(), "${FOO} world".to_string());

        let resolver = SimpleResolver::new(&recipe);

        assert_eq!(resolver.resolve("${FOO}"), "hello");
        assert_eq!(resolver.resolve("${BAR}"), "hello world");
    }

    #[test]
    fn test_nested_expansion() {
        let mut recipe = BitbakeRecipe::default();
        recipe
            .variables
            .insert("A".to_string(), "value_a".to_string());
        recipe.variables.insert("B".to_string(), "${A}".to_string());
        recipe
            .variables
            .insert("C".to_string(), "${B}_suffix".to_string());

        let resolver = SimpleResolver::new(&recipe);

        assert_eq!(resolver.resolve("${C}"), "value_a_suffix");
    }

    #[test]
    fn test_bpn_extraction() {
        // Test case 1: Normal case where PN doesn't contain version
        let mut recipe1 = BitbakeRecipe {
            file_path: PathBuf::from("/tmp/test.bb"),
            recipe_type: RecipeType::Recipe,
            package_name: Some("mypackage".to_string()),
            ..Default::default()
        };
        recipe1
            .variables
            .insert("PV".to_string(), "1.0.5".to_string());

        let resolver1 = SimpleResolver::new(&recipe1);
        assert_eq!(resolver1.get("PN"), Some("mypackage"));
        assert_eq!(resolver1.get("BPN"), Some("mypackage"));
        assert_eq!(resolver1.get("PV"), Some("1.0.5"));
        assert_eq!(resolver1.get("BP"), Some("mypackage-1.0.5"));

        // Test case 2: Edge case where PN contains version suffix
        let recipe2 = BitbakeRecipe {
            file_path: PathBuf::from("/tmp/test.bb"),
            recipe_type: RecipeType::Recipe,
            package_name: Some("mypackage-1.0.5".to_string()),
            ..Default::default()
        };

        let resolver2 = SimpleResolver::new(&recipe2);
        assert_eq!(resolver2.get("PN"), Some("mypackage-1.0.5"));
        // BPN should strip version-like suffix (everything after '-' followed by a digit)
        assert_eq!(resolver2.get("BPN"), Some("mypackage"));
        // PV extracted from remaining part after '-'
        assert_eq!(resolver2.get("PV"), Some("1.0.5"));
    }

    #[test]
    fn test_bp_combination() {
        let mut recipe = BitbakeRecipe::default();
        recipe
            .variables
            .insert("BPN".to_string(), "mypackage".to_string());
        recipe
            .variables
            .insert("PV".to_string(), "1.0".to_string());

        let resolver = SimpleResolver::new(&recipe);

        assert_eq!(resolver.get("BP"), Some("mypackage-1.0"));
    }

    #[test]
    fn test_unresolved_variables() {
        let recipe = BitbakeRecipe::default();
        let resolver = SimpleResolver::new(&recipe);

        // Should leave unresolved variables as-is
        let result = resolver.resolve("${UNKNOWN_VAR}/path");
        assert_eq!(result, "${UNKNOWN_VAR}/path");
    }

    #[test]
    fn test_default_syntax() {
        let mut recipe = BitbakeRecipe::default();
        recipe
            .variables
            .insert("FOO".to_string(), "hello".to_string());

        let resolver = SimpleResolver::new(&recipe);

        // Variable exists - use its value
        assert_eq!(resolver.resolve("${FOO:-default}"), "hello");

        // Variable doesn't exist - use default
        assert_eq!(resolver.resolve("${NOEXIST:-fallback}"), "fallback");
    }

    #[test]
    fn test_src_uri_expansion() {
        let mut recipe = BitbakeRecipe {
            file_path: PathBuf::from("/tmp/test.bb"),
            recipe_type: RecipeType::Recipe,
            package_name: Some("mypackage-1.0".to_string()),
            ..Default::default()
        };

        recipe.variables.insert(
            "SRC_URI".to_string(),
            "git://github.com/user/${BPN}.git;protocol=https;branch=main".to_string(),
        );

        let resolver = SimpleResolver::new(&recipe);
        let src_uri = resolver.get("SRC_URI").unwrap();
        let resolved = resolver.resolve(src_uri);

        assert!(resolved.contains("github.com/user/mypackage"));
        assert!(!resolved.contains("${BPN}"));
    }

    #[test]
    fn test_resolve_list() {
        let mut recipe = BitbakeRecipe::default();
        recipe
            .variables
            .insert("BASE".to_string(), "value".to_string());
        recipe.variables.insert(
            "LIST".to_string(),
            "file://${BASE}.txt   file://another.txt".to_string(),
        );

        let resolver = SimpleResolver::new(&recipe);
        let list = resolver.resolve_list(resolver.get("LIST").unwrap());

        assert_eq!(list.len(), 2);
        assert_eq!(list[0], "file://value.txt");
        assert_eq!(list[1], "file://another.txt");
    }
}
