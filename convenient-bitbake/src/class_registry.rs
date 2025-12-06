// Dynamic .bbclass registry with recursive inheritance and caching
// Phase 4: Production Hardening - .bbclass dynamic parsing

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::fs;
use tracing::{debug, warn};

use crate::class_dependencies::{
    find_class_file, parse_class_file, extract_inherited_classes,
    get_class_build_deps, get_class_runtime_deps, ClassDependencies,
};

/// Registry for dynamically loaded .bbclass files with caching
#[derive(Debug, Clone)]
pub struct ClassRegistry {
    /// Cached class definitions: class_name -> ClassDefinition
    classes: HashMap<String, ClassDefinition>,

    /// Search paths for .bbclass files
    search_paths: Vec<PathBuf>,

    /// Variables available for Python expression evaluation
    variables: HashMap<String, String>,
}

/// Complete definition of a .bbclass file
#[derive(Debug, Clone)]
pub struct ClassDefinition {
    /// Class name (e.g., "autotools")
    pub name: String,

    /// Full path to .bbclass file
    pub file_path: PathBuf,

    /// Build dependencies declared in this class (direct only)
    pub build_deps: Vec<String>,

    /// Runtime dependencies declared in this class (direct only)
    pub runtime_deps: Vec<String>,

    /// Classes this class inherits from
    pub inherits: Vec<String>,

    /// Whether this class was successfully parsed or fell back to hardcoded
    pub parsed: bool,
}

impl ClassRegistry {
    /// Create new registry with search paths
    pub fn new(search_paths: Vec<PathBuf>) -> Self {
        Self {
            classes: HashMap::new(),
            search_paths,
            variables: HashMap::new(),
        }
    }

    /// Set variables for Python expression evaluation
    pub fn with_variables(mut self, variables: HashMap<String, String>) -> Self {
        self.variables = variables;
        self
    }

    /// Load a class and all its inherited classes recursively
    pub fn load_class(&mut self, class_name: &str) -> Result<(), ClassError> {
        // Check cache first
        if self.classes.contains_key(class_name) {
            debug!("Class {} already loaded from cache", class_name);
            return Ok(());
        }

        debug!("Loading class: {}", class_name);

        // Try to find and parse .bbclass file
        if let Some(class_path) = find_class_file(class_name, &self.search_paths) {
            // Parse the .bbclass file
            match self.parse_and_cache_class(class_name, &class_path) {
                Ok(class_def) => {
                    // Recursively load inherited classes
                    let inherits = class_def.inherits.clone();
                    for inherited in &inherits {
                        self.load_class(inherited)?;
                    }
                    return Ok(());
                }
                Err(e) => {
                    warn!("Failed to parse {}: {}, falling back to hardcoded", class_name, e);
                }
            }
        }

        // Fall back to hardcoded mapping
        self.add_hardcoded_class(class_name);
        Ok(())
    }

    /// Parse a .bbclass file and add to cache
    fn parse_and_cache_class(
        &mut self,
        class_name: &str,
        class_path: &Path,
    ) -> Result<ClassDefinition, ClassError> {
        debug!("Parsing .bbclass file: {}", class_path.display());

        // Read and parse content
        let content = fs::read_to_string(class_path)
            .map_err(|e| ClassError::IoError(class_path.to_path_buf(), e.to_string()))?;

        // Extract inherited classes
        let inherits = extract_inherited_classes(&content);

        // Parse dependencies with Python evaluation
        let parsed = parse_class_file(class_path, &self.variables)
            .ok_or_else(|| ClassError::ParseFailed(class_name.to_string()))?;

        let class_def = ClassDefinition {
            name: class_name.to_string(),
            file_path: class_path.to_path_buf(),
            build_deps: parsed.build_deps,
            runtime_deps: parsed.runtime_deps,
            inherits,
            parsed: true,
        };

        debug!("Successfully parsed {}: {} build deps, {} runtime deps, inherits {:?}",
               class_name, class_def.build_deps.len(), class_def.runtime_deps.len(), class_def.inherits);

        self.classes.insert(class_name.to_string(), class_def.clone());
        Ok(class_def)
    }

    /// Add hardcoded class mapping as fallback
    fn add_hardcoded_class(&mut self, class_name: &str) {
        let distro_features = self.variables.get("DISTRO_FEATURES")
            .map(|s| s.as_str())
            .unwrap_or("");

        let build_deps = get_class_build_deps(class_name, distro_features);
        let runtime_deps = get_class_runtime_deps(class_name, distro_features);

        let class_def = ClassDefinition {
            name: class_name.to_string(),
            file_path: PathBuf::from(format!("<hardcoded:{}>", class_name)),
            build_deps,
            runtime_deps,
            inherits: vec![],
            parsed: false,
        };

        debug!("Using hardcoded mapping for {}: {} build deps, {} runtime deps",
               class_name, class_def.build_deps.len(), class_def.runtime_deps.len());

        self.classes.insert(class_name.to_string(), class_def);
    }

    /// Get all dependencies for a class, including from inherited classes
    pub fn get_all_class_dependencies(&self, class_name: &str) -> (Vec<String>, Vec<String>) {
        let mut build_deps = Vec::new();
        let mut runtime_deps = Vec::new();
        let mut visited = HashSet::new();

        self.collect_dependencies_recursive(class_name, &mut build_deps, &mut runtime_deps, &mut visited);

        // Deduplicate while preserving order
        build_deps = deduplicate_preserve_order(build_deps);
        runtime_deps = deduplicate_preserve_order(runtime_deps);

        (build_deps, runtime_deps)
    }

    /// Recursively collect dependencies from class and all inherited classes
    fn collect_dependencies_recursive(
        &self,
        class_name: &str,
        build_deps: &mut Vec<String>,
        runtime_deps: &mut Vec<String>,
        visited: &mut HashSet<String>,
    ) {
        if visited.contains(class_name) {
            return;
        }
        visited.insert(class_name.to_string());

        if let Some(class_def) = self.classes.get(class_name) {
            // Add this class's dependencies first
            build_deps.extend(class_def.build_deps.clone());
            runtime_deps.extend(class_def.runtime_deps.clone());

            // Then recursively add inherited classes' dependencies
            for inherited in &class_def.inherits {
                self.collect_dependencies_recursive(inherited, build_deps, runtime_deps, visited);
            }
        }
    }

    /// Get class definition if loaded
    pub fn get_class(&self, class_name: &str) -> Option<&ClassDefinition> {
        self.classes.get(class_name)
    }

    /// Check if class was successfully parsed (vs hardcoded fallback)
    pub fn is_class_parsed(&self, class_name: &str) -> bool {
        self.classes.get(class_name)
            .map(|c| c.parsed)
            .unwrap_or(false)
    }

    /// Get statistics about loaded classes
    pub fn stats(&self) -> ClassRegistryStats {
        let total = self.classes.len();
        let parsed = self.classes.values().filter(|c| c.parsed).count();
        let hardcoded = total - parsed;

        ClassRegistryStats {
            total_classes: total,
            parsed_classes: parsed,
            hardcoded_classes: hardcoded,
        }
    }
}

/// Statistics about loaded classes
#[derive(Debug, Clone)]
pub struct ClassRegistryStats {
    pub total_classes: usize,
    pub parsed_classes: usize,
    pub hardcoded_classes: usize,
}

/// Errors that can occur when loading classes
#[derive(Debug, Clone)]
pub enum ClassError {
    IoError(PathBuf, String),
    ParseFailed(String),
    RecursiveInheritance(String),
}

impl std::fmt::Display for ClassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClassError::IoError(path, msg) => write!(f, "I/O error reading {}: {}", path.display(), msg),
            ClassError::ParseFailed(class) => write!(f, "Failed to parse class: {}", class),
            ClassError::RecursiveInheritance(class) => write!(f, "Recursive inheritance detected: {}", class),
        }
    }
}

impl std::error::Error for ClassError {}

/// Remove duplicates while preserving order (first occurrence wins)
fn deduplicate_preserve_order(mut vec: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    vec.retain(|item| seen.insert(item.clone()));
    vec
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_class(dir: &Path, name: &str, content: &str) -> PathBuf {
        let classes_dir = dir.join("classes");
        fs::create_dir_all(&classes_dir).unwrap();
        let class_path = classes_dir.join(format!("{}.bbclass", name));
        fs::write(&class_path, content).unwrap();
        class_path
    }

    #[test]
    fn test_load_simple_class() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        create_test_class(temp_path, "simple", "DEPENDS = \"cmake-native ninja-native\"");

        let mut registry = ClassRegistry::new(vec![temp_path.to_path_buf()]);
        registry.load_class("simple").unwrap();

        let (build_deps, _) = registry.get_all_class_dependencies("simple");
        assert_eq!(build_deps, vec!["cmake-native", "ninja-native"]);
    }

    #[test]
    fn test_load_class_with_inheritance() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Base class
        create_test_class(temp_path, "base", "DEPENDS = \"base-dep\"");

        // Derived class that inherits base
        create_test_class(temp_path, "derived", "inherit base\nDEPENDS += \"derived-dep\"");

        let mut registry = ClassRegistry::new(vec![temp_path.to_path_buf()]);
        registry.load_class("derived").unwrap();

        let (build_deps, _) = registry.get_all_class_dependencies("derived");
        assert!(build_deps.contains(&"base-dep".to_string()));
        assert!(build_deps.contains(&"derived-dep".to_string()));
    }

    #[test]
    fn test_load_class_with_python_expressions() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        create_test_class(
            temp_path,
            "conditional",
            "DEPENDS:append = \" ${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'libsystemd', '', d)}\""
        );

        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam".to_string());

        let mut registry = ClassRegistry::new(vec![temp_path.to_path_buf()])
            .with_variables(vars);
        registry.load_class("conditional").unwrap();

        let (build_deps, _) = registry.get_all_class_dependencies("conditional");
        assert!(build_deps.contains(&"libsystemd".to_string()));
    }

    #[test]
    fn test_fallback_to_hardcoded() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let mut registry = ClassRegistry::new(vec![temp_path.to_path_buf()]);
        registry.load_class("autotools").unwrap();

        // Should fall back to hardcoded mapping
        assert!(!registry.is_class_parsed("autotools"));

        let (build_deps, _) = registry.get_all_class_dependencies("autotools");
        assert!(build_deps.contains(&"autoconf-native".to_string()));
        assert!(build_deps.contains(&"automake-native".to_string()));
    }

    #[test]
    fn test_deduplication() {
        let vec = vec![
            "foo".to_string(),
            "bar".to_string(),
            "foo".to_string(),
            "baz".to_string(),
            "bar".to_string(),
        ];

        let result = deduplicate_preserve_order(vec);
        assert_eq!(result, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_registry_stats() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        create_test_class(temp_path, "parsed1", "DEPENDS = \"foo\"");
        create_test_class(temp_path, "parsed2", "DEPENDS = \"bar\"");

        let mut registry = ClassRegistry::new(vec![temp_path.to_path_buf()]);
        registry.load_class("parsed1").unwrap();
        registry.load_class("parsed2").unwrap();
        registry.load_class("autotools").unwrap(); // Hardcoded fallback

        let stats = registry.stats();
        assert_eq!(stats.total_classes, 3);
        assert_eq!(stats.parsed_classes, 2);
        assert_eq!(stats.hardcoded_classes, 1);
    }

    #[test]
    fn test_multiple_inheritance() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create inheritance hierarchy:
        // base1 -> DEPENDS=dep1
        // base2 -> DEPENDS=dep2
        // derived -> inherit base1 base2, DEPENDS=dep3

        create_test_class(temp_path, "base1", "DEPENDS = \"dep1\"");
        create_test_class(temp_path, "base2", "DEPENDS = \"dep2\"");
        create_test_class(temp_path, "derived", "inherit base1 base2\nDEPENDS += \"dep3\"");

        let mut registry = ClassRegistry::new(vec![temp_path.to_path_buf()]);
        registry.load_class("derived").unwrap();

        let (build_deps, _) = registry.get_all_class_dependencies("derived");
        assert!(build_deps.contains(&"dep1".to_string()));
        assert!(build_deps.contains(&"dep2".to_string()));
        assert!(build_deps.contains(&"dep3".to_string()));
    }
}
