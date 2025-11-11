// Include file resolver for BitBake recipes
// Handles include and require directives with search paths

use crate::{BitbakeRecipe, IncludeDirective, SimpleResolver};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Resolves and caches include files
#[derive(Debug)]
pub struct IncludeResolver {
    /// Search paths for include files (in priority order)
    search_paths: Vec<PathBuf>,
    /// Cache of already parsed include files
    cache: HashMap<PathBuf, BitbakeRecipe>,
    /// Track files being parsed to detect circular includes
    parsing_stack: HashSet<PathBuf>,
}

impl IncludeResolver {
    /// Create a new include resolver with default search paths
    pub fn new() -> Self {
        Self {
            search_paths: Vec::new(),
            cache: HashMap::new(),
            parsing_stack: HashSet::new(),
        }
    }

    /// Add a search path for include files
    pub fn add_search_path<P: Into<PathBuf>>(&mut self, path: P) {
        let path = path.into();
        if !self.search_paths.contains(&path) {
            self.search_paths.push(path);
        }
    }

    /// Add multiple search paths at once
    pub fn add_search_paths<I, P>(&mut self, paths: I)
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        for path in paths {
            self.add_search_path(path);
        }
    }

    /// Find an include file by searching all search paths
    fn find_include_file(&self, include_path: &str, base_dir: &Path) -> Option<PathBuf> {
        // First try relative to the recipe's directory
        let relative_path = base_dir.join(include_path);
        if relative_path.exists() {
            return Some(relative_path);
        }

        // Then try each search path
        for search_path in &self.search_paths {
            let candidate = search_path.join(include_path);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        None
    }

    /// Parse an include file (with caching and circular detection)
    fn parse_include_file(&mut self, path: &Path) -> Result<BitbakeRecipe, String> {
        // Check for circular includes
        if self.parsing_stack.contains(path) {
            return Err(format!("Circular include detected: {:?}", path));
        }

        // Check cache
        if let Some(cached) = self.cache.get(path) {
            return Ok(cached.clone());
        }

        // Mark as being parsed
        self.parsing_stack.insert(path.to_path_buf());

        // Parse the file
        debug!("Parsing include file: {:?}", path);
        let mut recipe = BitbakeRecipe::parse_file(path).map_err(|e| {
            format!("Failed to parse include file {:?}: {}", path, e)
        })?;

        // Recursively resolve includes in this file
        self.resolve_all_includes(&mut recipe)?;

        // Remove from parsing stack
        self.parsing_stack.remove(path);

        // Cache it
        self.cache.insert(path.to_path_buf(), recipe.clone());

        Ok(recipe)
    }

    /// Resolve a single include directive
    pub fn resolve_include(
        &mut self,
        include: &IncludeDirective,
        base_dir: &Path,
        var_resolver: Option<&SimpleResolver>,
    ) -> Result<Option<BitbakeRecipe>, String> {
        debug!("Resolving include: {:?}", include.path);

        // Expand variables in include path if resolver provided
        let resolved_path = if let Some(resolver) = var_resolver {
            resolver.resolve(&include.path)
        } else {
            include.path.clone()
        };

        debug!("Resolved include path: {}", resolved_path);

        // Find the include file
        let include_path = match self.find_include_file(&resolved_path, base_dir) {
            Some(path) => path,
            None => {
                if include.required {
                    return Err(format!("Required file not found: {}", resolved_path));
                } else {
                    warn!("Include file not found (non-fatal): {}", resolved_path);
                    return Ok(None);
                }
            }
        };

        // Parse it (with caching and recursive resolution)
        let recipe = self.parse_include_file(&include_path)?;

        Ok(Some(recipe))
    }

    /// Resolve all includes in a recipe and merge variables
    pub fn resolve_all_includes(&mut self, recipe: &mut BitbakeRecipe) -> Result<(), String> {
        let base_dir = recipe
            .file_path
            .parent()
            .unwrap_or_else(|| Path::new("."));

        // Create a variable resolver for expanding variables in include paths
        let var_resolver = recipe.create_resolver();

        // Keep track of all included variables
        let mut merged_variables = recipe.variables.clone();

        // Process each include
        for include in &recipe.includes {
            match self.resolve_include(include, base_dir, Some(&var_resolver)) {
                Ok(Some(included_recipe)) => {
                    debug!(
                        "Merging variables from include: {:?}",
                        included_recipe.file_path
                    );

                    // Merge variables - includes are processed in order
                    // Later assignments override earlier ones
                    for (key, value) in &included_recipe.variables {
                        merged_variables.insert(key.clone(), value.clone());
                    }

                    // Merge inherits
                    for inherit in &included_recipe.inherits {
                        if !recipe.inherits.contains(inherit) {
                            recipe.inherits.push(inherit.clone());
                        }
                    }

                    // Merge sources (for SRC_URI +=)
                    for source in &included_recipe.sources {
                        if !recipe.sources.iter().any(|s| s.url == source.url) {
                            recipe.sources.push(source.clone());
                        }
                    }

                    // Merge dependencies
                    for dep in &included_recipe.build_depends {
                        if !recipe.build_depends.contains(dep) {
                            recipe.build_depends.push(dep.clone());
                        }
                    }
                    for dep in &included_recipe.runtime_depends {
                        if !recipe.runtime_depends.contains(dep) {
                            recipe.runtime_depends.push(dep.clone());
                        }
                    }
                }
                Ok(None) => {
                    // Include file not found but non-fatal
                    debug!("Include file not found (non-fatal): {}", include.path);
                }
                Err(e) => {
                    // Fatal error (e.g., require not found, circular include, parse error)
                    return Err(e);
                }
            }
        }

        // Update recipe with merged variables
        recipe.variables = merged_variables;

        Ok(())
    }

    /// Clear the cache (useful for testing)
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.parsing_stack.clear();
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.cache.len(), self.search_paths.len())
    }
}

impl Default for IncludeResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_simple_include() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        // Create base recipe
        let base_content = r#"
SUMMARY = "Test recipe"
include test.inc
FOO = "from-base"
"#;
        let base_path = create_test_recipe(base_dir, "base.bb", base_content);

        // Create include file
        let inc_content = r#"
LICENSE = "MIT"
BAR = "from-inc"
"#;
        create_test_recipe(base_dir, "test.inc", inc_content);

        // Parse and resolve
        let mut recipe = BitbakeRecipe::parse_file(&base_path).unwrap();
        let mut resolver = IncludeResolver::new();
        resolver.add_search_path(base_dir);

        resolver.resolve_all_includes(&mut recipe).unwrap();

        // Check merged variables
        assert_eq!(recipe.variables.get("SUMMARY"), Some(&"Test recipe".to_string()));
        assert_eq!(recipe.variables.get("LICENSE"), Some(&"MIT".to_string()));
        assert_eq!(recipe.variables.get("FOO"), Some(&"from-base".to_string()));
        assert_eq!(recipe.variables.get("BAR"), Some(&"from-inc".to_string()));
    }

    #[test]
    fn test_nested_includes() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        // Create base recipe
        let base_content = r#"
include level1.inc
VAR_BASE = "base"
"#;
        let base_path = create_test_recipe(base_dir, "base.bb", base_content);

        // Create level 1 include
        let level1_content = r#"
include level2.inc
VAR_L1 = "level1"
"#;
        create_test_recipe(base_dir, "level1.inc", level1_content);

        // Create level 2 include
        let level2_content = r#"
VAR_L2 = "level2"
"#;
        create_test_recipe(base_dir, "level2.inc", level2_content);

        // Parse and resolve
        let mut recipe = BitbakeRecipe::parse_file(&base_path).unwrap();
        let mut resolver = IncludeResolver::new();
        resolver.add_search_path(base_dir);

        resolver.resolve_all_includes(&mut recipe).unwrap();

        // Check all variables are merged
        assert_eq!(recipe.variables.get("VAR_BASE"), Some(&"base".to_string()));
        assert_eq!(recipe.variables.get("VAR_L1"), Some(&"level1".to_string()));
        assert_eq!(recipe.variables.get("VAR_L2"), Some(&"level2".to_string()));
    }

    #[test]
    fn test_include_not_found_non_fatal() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        let base_content = r#"
include nonexistent.inc
FOO = "bar"
"#;
        let base_path = create_test_recipe(base_dir, "base.bb", base_content);

        let mut recipe = BitbakeRecipe::parse_file(&base_path).unwrap();
        let mut resolver = IncludeResolver::new();
        resolver.add_search_path(base_dir);

        // Should not error - include is non-fatal
        let result = resolver.resolve_all_includes(&mut recipe);
        assert!(result.is_ok());

        // Base variable should still be present
        assert_eq!(recipe.variables.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_require_not_found_fatal() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        let base_content = r#"
require nonexistent.inc
FOO = "bar"
"#;
        let base_path = create_test_recipe(base_dir, "base.bb", base_content);

        let mut recipe = BitbakeRecipe::parse_file(&base_path).unwrap();
        let mut resolver = IncludeResolver::new();
        resolver.add_search_path(base_dir);

        // Should error - require is fatal
        let result = resolver.resolve_all_includes(&mut recipe);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Required file not found"));
    }

    #[test]
    fn test_circular_include_detection() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        // Create circular includes
        let a_content = r#"
include b.inc
VAR_A = "a"
"#;
        let a_path = create_test_recipe(base_dir, "a.bb", a_content);

        let b_content = r#"
include a.bb
VAR_B = "b"
"#;
        create_test_recipe(base_dir, "b.inc", b_content);

        let mut recipe = BitbakeRecipe::parse_file(&a_path).unwrap();
        let mut resolver = IncludeResolver::new();
        resolver.add_search_path(base_dir);

        // Should detect circular include
        let result = resolver.resolve_all_includes(&mut recipe);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Circular include"));
    }

    #[test]
    fn test_variable_merging() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        let base_content = r#"
FOO = "base"
include vars.inc
BAR = "also-base"
"#;
        let base_path = create_test_recipe(base_dir, "base.bb", base_content);

        let inc_content = r#"
FOO = "from-inc"
BAZ = "from-inc-only"
"#;
        create_test_recipe(base_dir, "vars.inc", inc_content);

        let mut recipe = BitbakeRecipe::parse_file(&base_path).unwrap();
        let mut resolver = IncludeResolver::new();
        resolver.add_search_path(base_dir);

        resolver.resolve_all_includes(&mut recipe).unwrap();

        // Included values override base for same variable
        // Note: Full BitBake preserves assignment order, but we use HashMap
        // so this is a simplified behavior
        assert_eq!(recipe.variables.get("FOO"), Some(&"from-inc".to_string()));
        assert_eq!(recipe.variables.get("BAR"), Some(&"also-base".to_string()));
        assert_eq!(recipe.variables.get("BAZ"), Some(&"from-inc-only".to_string()));
    }

    #[test]
    fn test_caching() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        let inc_content = r#"
SHARED = "value"
"#;
        create_test_recipe(base_dir, "shared.inc", inc_content);

        let recipe1_content = r#"
include shared.inc
R1 = "recipe1"
"#;
        let recipe1_path = create_test_recipe(base_dir, "recipe1.bb", recipe1_content);

        let recipe2_content = r#"
include shared.inc
R2 = "recipe2"
"#;
        let recipe2_path = create_test_recipe(base_dir, "recipe2.bb", recipe2_content);

        let mut resolver = IncludeResolver::new();
        resolver.add_search_path(base_dir);

        // Parse first recipe
        let mut recipe1 = BitbakeRecipe::parse_file(&recipe1_path).unwrap();
        resolver.resolve_all_includes(&mut recipe1).unwrap();

        let (cache_size_1, _) = resolver.cache_stats();
        assert_eq!(cache_size_1, 1); // shared.inc cached

        // Parse second recipe - should use cache
        let mut recipe2 = BitbakeRecipe::parse_file(&recipe2_path).unwrap();
        resolver.resolve_all_includes(&mut recipe2).unwrap();

        let (cache_size_2, _) = resolver.cache_stats();
        assert_eq!(cache_size_2, 1); // Still just shared.inc

        // Both should have the shared variable
        assert_eq!(recipe1.variables.get("SHARED"), Some(&"value".to_string()));
        assert_eq!(recipe2.variables.get("SHARED"), Some(&"value".to_string()));
    }
}
