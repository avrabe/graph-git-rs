// Layer context management for BitBake builds
// Handles layer configuration, priorities, and global variable context

use crate::{BitbakeRecipe, IncludeResolver, SimpleResolver};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Layer configuration from layer.conf
#[derive(Debug, Clone)]
pub struct LayerConfig {
    /// Layer directory path
    pub layer_dir: PathBuf,
    /// Layer collection name (e.g., "core", "meta-oe", "fmu")
    pub collection: String,
    /// Layer priority (higher = more important)
    pub priority: i32,
    /// Layer version
    pub version: Option<String>,
    /// Layer dependencies
    pub depends: Vec<String>,
    /// Layer series compatibility
    pub series_compat: Vec<String>,
    /// All variables from layer.conf
    pub variables: HashMap<String, String>,
}

impl LayerConfig {
    /// Parse a layer.conf file
    pub fn parse<P: AsRef<Path>>(layer_conf_path: P) -> Result<Self, String> {
        let path = layer_conf_path.as_ref();
        let layer_dir = path
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| "Invalid layer.conf path".to_string())?
            .to_path_buf();

        let recipe = BitbakeRecipe::parse_file(path)?;

        // Extract layer collection name
        let collection = recipe
            .variables
            .get("BBFILE_COLLECTIONS")
            .and_then(|s| s.split_whitespace().last())
            .unwrap_or("unknown")
            .to_string();

        // Extract priority - look for BBFILE_PRIORITY_<collection>
        let priority_key = format!("BBFILE_PRIORITY_{collection}");
        let priority = recipe
            .variables
            .get(&priority_key)
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);

        // Extract version
        let version_key = format!("LAYERVERSION_{collection}");
        let version = recipe.variables.get(&version_key).cloned();

        // Extract dependencies
        let depends_key = format!("LAYERDEPENDS_{collection}");
        let depends = recipe
            .variables
            .get(&depends_key)
            .map(|s| {
                s.split_whitespace()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Extract series compatibility
        let compat_key = format!("LAYERSERIES_COMPAT_{collection}");
        let series_compat = recipe
            .variables
            .get(&compat_key)
            .map(|s| {
                s.split_whitespace()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(LayerConfig {
            layer_dir,
            collection,
            priority,
            version,
            depends,
            series_compat,
            variables: recipe.variables,
        })
    }

    /// Get BBFILES pattern for this layer
    pub fn get_bbfiles(&self) -> Vec<String> {
        self.variables
            .get("BBFILES")
            .map(|s| vec![s.clone()])
            .unwrap_or_default()
    }
}

/// Build context with all layers and configuration
#[derive(Debug)]
pub struct BuildContext {
    /// All layers in priority order (highest priority first)
    pub layers: Vec<LayerConfig>,
    /// Global variables merged from all configuration files
    pub global_variables: HashMap<String, String>,
    /// MACHINE setting
    pub machine: Option<String>,
    /// DISTRO setting
    pub distro: Option<String>,
    /// Include resolver for configuration files
    include_resolver: IncludeResolver,
    /// Cache of parsed configuration files
    config_cache: HashMap<PathBuf, BitbakeRecipe>,
}

impl BuildContext {
    /// Create a new build context
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            global_variables: HashMap::new(),
            machine: None,
            distro: None,
            include_resolver: IncludeResolver::new(),
            config_cache: HashMap::new(),
        }
    }

    /// Add a layer to the context
    pub fn add_layer(&mut self, layer: LayerConfig) {
        info!("Adding layer: {} (priority: {})", layer.collection, layer.priority);
        self.layers.push(layer);
        // Sort by priority (highest first)
        self.layers.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Add a layer by parsing its layer.conf
    pub fn add_layer_from_conf<P: AsRef<Path>>(&mut self, layer_conf: P) -> Result<(), String> {
        let layer = LayerConfig::parse(layer_conf)?;
        self.add_layer(layer);
        Ok(())
    }

    /// Set machine configuration
    pub fn set_machine(&mut self, machine: String) {
        info!("Setting MACHINE: {}", machine);
        self.machine = Some(machine.clone());
        self.global_variables.insert("MACHINE".to_string(), machine);
    }

    /// Set distro configuration
    pub fn set_distro(&mut self, distro: String) {
        info!("Setting DISTRO: {}", distro);
        self.distro = Some(distro.clone());
        self.global_variables.insert("DISTRO".to_string(), distro);
    }

    /// Load a configuration file (distro, machine, or other)
    pub fn load_conf_file<P: AsRef<Path>>(&mut self, conf_file: P) -> Result<(), String> {
        let path = conf_file.as_ref();

        // Check cache
        if let Some(cached) = self.config_cache.get(path) {
            let vars = cached.variables.clone();
            self.merge_config_variables(&vars);
            return Ok(());
        }

        info!("Loading configuration file: {:?}", path);

        // Parse the config file
        let mut recipe = BitbakeRecipe::parse_file(path)?;

        // Set up include resolver with layer search paths
        for layer in &self.layers {
            self.include_resolver.add_search_path(&layer.layer_dir);
            self.include_resolver.add_search_path(layer.layer_dir.join("conf"));
        }

        // Resolve includes
        self.include_resolver.resolve_all_includes(&mut recipe)?;

        // Merge variables into global context
        self.merge_config_variables(&recipe.variables);

        // Cache it
        self.config_cache.insert(path.to_path_buf(), recipe);

        Ok(())
    }

    /// Merge configuration variables into global context
    fn merge_config_variables(&mut self, variables: &HashMap<String, String>) {
        for (key, value) in variables {
            self.global_variables.insert(key.clone(), value.clone());
        }
    }

    /// Find machine configuration file in layers
    fn find_machine_conf(&self, machine: &str) -> Option<PathBuf> {
        for layer in &self.layers {
            let machine_conf = layer.layer_dir.join(format!("conf/machine/{machine}.conf"));
            if machine_conf.exists() {
                info!("Found machine config: {:?}", machine_conf);
                return Some(machine_conf);
            }
        }
        None
    }

    /// Find distro configuration file in layers
    fn find_distro_conf(&self, distro: &str) -> Option<PathBuf> {
        for layer in &self.layers {
            let distro_conf = layer.layer_dir.join(format!("conf/distro/{distro}.conf"));
            if distro_conf.exists() {
                info!("Found distro config: {:?}", distro_conf);
                return Some(distro_conf);
            }
        }
        None
    }

    /// Load machine configuration
    ///
    /// Finds and loads the machine.conf file for the configured machine,
    /// merging variables into the global context.
    pub fn load_machine_config(&mut self) -> Result<(), String> {
        if let Some(ref machine) = self.machine.clone() {
            if let Some(machine_conf) = self.find_machine_conf(machine) {
                info!("Loading machine configuration for: {}", machine);
                self.load_conf_file(&machine_conf)?;
                return Ok(());
            }
            warn!("Machine configuration not found for: {}", machine);
        }
        Ok(())
    }

    /// Load distro configuration
    ///
    /// Finds and loads the distro.conf file for the configured distro,
    /// merging variables into the global context.
    pub fn load_distro_config(&mut self) -> Result<(), String> {
        if let Some(ref distro) = self.distro.clone() {
            if let Some(distro_conf) = self.find_distro_conf(distro) {
                info!("Loading distro configuration for: {}", distro);
                self.load_conf_file(&distro_conf)?;
                return Ok(());
            }
            warn!("Distro configuration not found for: {}", distro);
        }
        Ok(())
    }

    /// Find all recipes in all layers
    pub fn find_recipes(&self) -> Vec<PathBuf> {
        let mut recipes = Vec::new();

        for layer in &self.layers {
            let recipes_dir = layer.layer_dir.join("recipes-*");
            debug!("Searching for recipes in: {:?}", recipes_dir);

            // Search for .bb and .bbappend files
            if let Ok(entries) = glob::glob(recipes_dir.to_str().unwrap_or("")) {
                for entry in entries.flatten() {
                    if entry.is_dir() {
                        // Search in subdirectories
                        for pattern in &["**/*.bb", "**/*.bbappend"] {
                            let search = entry.join(pattern);
                            if let Ok(files) = glob::glob(search.to_str().unwrap_or("")) {
                                recipes.extend(files.flatten());
                            }
                        }
                    }
                }
            }
        }

        recipes
    }

    /// Find bbappend files for a recipe
    pub fn find_bbappends_for(&self, recipe_name: &str) -> Vec<PathBuf> {
        let mut bbappends = Vec::new();

        for layer in &self.layers {
            // Search for .bbappend files matching the recipe name
            let pattern = format!("{}*/recipes-*/*/*/{}.bbappend",
                layer.layer_dir.display(), recipe_name);

            if let Ok(entries) = glob::glob(&pattern) {
                bbappends.extend(entries.flatten());
            }
        }

        // Sort by layer priority
        bbappends.sort_by(|a, b| {
            let layer_a = self.get_layer_for_path(a);
            let layer_b = self.get_layer_for_path(b);
            match (layer_a, layer_b) {
                (Some(a), Some(b)) => b.priority.cmp(&a.priority),
                _ => std::cmp::Ordering::Equal,
            }
        });

        bbappends
    }

    /// Get the layer that contains a given path
    fn get_layer_for_path(&self, path: &Path) -> Option<&LayerConfig> {
        self.layers
            .iter()
            .find(|layer| path.starts_with(&layer.layer_dir))
    }

    /// Parse a recipe with full context (includes + bbappends)
    pub fn parse_recipe_with_context<P: AsRef<Path>>(
        &mut self,
        recipe_path: P,
    ) -> Result<BitbakeRecipe, String> {
        let path = recipe_path.as_ref();

        // Parse base recipe
        let mut recipe = BitbakeRecipe::parse_file(path)?;

        // Set up include resolver
        for layer in &self.layers {
            self.include_resolver.add_search_path(&layer.layer_dir);
        }

        // Resolve includes
        self.include_resolver.resolve_all_includes(&mut recipe)?;

        // Find and apply bbappend files
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            let bbappends = self.find_bbappends_for(stem);

            for bbappend_path in bbappends {
                info!("Applying bbappend: {:?}", bbappend_path);

                let mut bbappend = BitbakeRecipe::parse_file(&bbappend_path)?;
                self.include_resolver.resolve_all_includes(&mut bbappend)?;

                // Merge bbappend into recipe
                self.merge_bbappend(&mut recipe, &bbappend);
            }
        }

        Ok(recipe)
    }

    /// Merge a bbappend file into a recipe
    fn merge_bbappend(&self, recipe: &mut BitbakeRecipe, bbappend: &BitbakeRecipe) {
        // Merge variables (bbappend overrides base)
        for (key, value) in &bbappend.variables {
            recipe.variables.insert(key.clone(), value.clone());
        }

        // Merge sources (append operations)
        for source in &bbappend.sources {
            if !recipe.sources.iter().any(|s| s.url == source.url) {
                recipe.sources.push(source.clone());
            }
        }

        // Merge dependencies
        for dep in &bbappend.build_depends {
            if !recipe.build_depends.contains(dep) {
                recipe.build_depends.push(dep.clone());
            }
        }

        for dep in &bbappend.runtime_depends {
            if !recipe.runtime_depends.contains(dep) {
                recipe.runtime_depends.push(dep.clone());
            }
        }

        // Merge inherits
        for inherit in &bbappend.inherits {
            if !recipe.inherits.contains(inherit) {
                recipe.inherits.push(inherit.clone());
            }
        }
    }

    /// Create a resolver with full build context
    pub fn create_resolver(&self, recipe: &BitbakeRecipe) -> SimpleResolver {
        let mut resolver = SimpleResolver::new(recipe);

        // Add global variables from build context
        for (key, value) in &self.global_variables {
            resolver.set(key.clone(), value.clone());
        }

        // Add layer-specific variables
        for layer in &self.layers {
            // Add LAYERDIR for each layer
            let layerdir_var = format!("LAYERDIR_{}", layer.collection);
            resolver.set(
                layerdir_var,
                layer.layer_dir.to_string_lossy().to_string(),
            );
        }

        resolver
    }

    /// Create an override resolver with MACHINE/DISTRO overrides configured
    pub fn create_override_resolver(&self, recipe: &BitbakeRecipe) -> crate::override_resolver::OverrideResolver {
        // Create base resolver with build context
        let base_resolver = self.create_resolver(recipe);

        // Create override resolver
        let mut override_resolver = crate::override_resolver::OverrideResolver::new(base_resolver);

        // Configure overrides from MACHINE and DISTRO
        override_resolver.build_overrides_from_context(
            self.machine.as_deref(),
            self.distro.as_deref(),
            &[], // No additional overrides for now
        );

        override_resolver
    }

    /// Get layer information
    pub fn get_layers_info(&self) -> Vec<(String, i32, PathBuf)> {
        self.layers
            .iter()
            .map(|l| (l.collection.clone(), l.priority, l.layer_dir.clone()))
            .collect()
    }

    /// Verify layer dependencies
    pub fn verify_dependencies(&self) -> Result<(), String> {
        let available: HashSet<_> = self.layers.iter().map(|l| l.collection.as_str()).collect();

        for layer in &self.layers {
            for dep in &layer.depends {
                if !available.contains(dep.as_str()) {
                    return Err(format!(
                        "Layer '{}' depends on '{}' which is not available",
                        layer.collection, dep
                    ));
                }
            }
        }

        Ok(())
    }
}

impl Default for BuildContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_layer(dir: &Path, name: &str, priority: i32) -> PathBuf {
        let layer_dir = dir.join(name);
        fs::create_dir_all(layer_dir.join("conf")).unwrap();

        let layer_conf = format!(
            r#"
BBPATH .= ":${{LAYERDIR}}"
BBFILES += "${{LAYERDIR}}/recipes-*/*/*.bb"
BBFILE_COLLECTIONS += "{name}"
BBFILE_PATTERN_{name} = "^${{LAYERDIR}}/"
BBFILE_PRIORITY_{name} = "{priority}"
LAYERVERSION_{name} = "1"
"#,
            name = name,
            priority = priority
        );

        let conf_path = layer_dir.join("conf/layer.conf");
        fs::write(&conf_path, layer_conf).unwrap();

        layer_dir
    }

    #[test]
    fn test_layer_config_parse() {
        let temp_dir = TempDir::new().unwrap();
        let layer_dir = create_test_layer(temp_dir.path(), "test-layer", 5);

        let layer_conf = layer_dir.join("conf/layer.conf");
        let config = LayerConfig::parse(&layer_conf).unwrap();

        assert_eq!(config.collection, "test-layer");
        assert_eq!(config.priority, 5);
        assert_eq!(config.version, Some("1".to_string()));
    }

    #[test]
    fn test_build_context_layer_priority() {
        let temp_dir = TempDir::new().unwrap();

        let layer1 = create_test_layer(temp_dir.path(), "layer1", 10);
        let layer2 = create_test_layer(temp_dir.path(), "layer2", 5);
        let layer3 = create_test_layer(temp_dir.path(), "layer3", 15);

        let mut context = BuildContext::new();
        context.add_layer_from_conf(layer1.join("conf/layer.conf")).unwrap();
        context.add_layer_from_conf(layer2.join("conf/layer.conf")).unwrap();
        context.add_layer_from_conf(layer3.join("conf/layer.conf")).unwrap();

        // Should be sorted by priority (highest first)
        assert_eq!(context.layers[0].collection, "layer3"); // priority 15
        assert_eq!(context.layers[1].collection, "layer1"); // priority 10
        assert_eq!(context.layers[2].collection, "layer2"); // priority 5
    }

    #[test]
    fn test_build_context_global_variables() {
        let mut context = BuildContext::new();

        context.set_machine("qemuarm64".to_string());
        context.set_distro("poky".to_string());

        assert_eq!(context.machine, Some("qemuarm64".to_string()));
        assert_eq!(context.distro, Some("poky".to_string()));
        assert_eq!(
            context.global_variables.get("MACHINE"),
            Some(&"qemuarm64".to_string())
        );
        assert_eq!(
            context.global_variables.get("DISTRO"),
            Some(&"poky".to_string())
        );
    }

    #[test]
    fn test_bbappend_merging() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        // Create base recipe
        let recipe_content = r#"
SUMMARY = "Base recipe"
SRC_URI = "file://base.txt"
DEPENDS = "base-dep"
"#;
        let recipe_path = base_dir.join("recipe.bb");
        fs::write(&recipe_path, recipe_content).unwrap();

        // Create bbappend
        let bbappend_content = r#"
SUMMARY = "Modified by bbappend"
SRC_URI += "file://extra.txt"
DEPENDS += "extra-dep"
"#;
        let bbappend_path = base_dir.join("recipe.bbappend");
        fs::write(&bbappend_path, bbappend_content).unwrap();

        // Parse both
        let mut recipe = BitbakeRecipe::parse_file(&recipe_path).unwrap();
        let bbappend = BitbakeRecipe::parse_file(&bbappend_path).unwrap();

        // Merge
        let context = BuildContext::new();
        context.merge_bbappend(&mut recipe, &bbappend);

        // Check merged results
        assert_eq!(
            recipe.variables.get("SUMMARY"),
            Some(&"Modified by bbappend".to_string())
        );
    }

    #[test]
    fn test_layer_dependencies() {
        let temp_dir = TempDir::new().unwrap();

        // Create layer with dependencies
        let layer_dir = temp_dir.path().join("meta-test");
        fs::create_dir_all(layer_dir.join("conf")).unwrap();

        let layer_conf = r#"
BBFILE_COLLECTIONS += "test"
BBFILE_PRIORITY_test = "5"
LAYERDEPENDS_test = "core meta-oe"
"#;
        fs::write(layer_dir.join("conf/layer.conf"), layer_conf).unwrap();

        let config = LayerConfig::parse(layer_dir.join("conf/layer.conf")).unwrap();

        assert_eq!(config.depends, vec!["core", "meta-oe"]);
    }

    #[test]
    fn test_override_resolver_integration() {
        let temp_dir = TempDir::new().unwrap();
        let layer_dir = temp_dir.path().join("meta-test");
        fs::create_dir_all(&layer_dir).unwrap();

        // Create a simple recipe with machine/distro overrides
        let recipe_content = r#"
DESCRIPTION = "Test recipe with overrides"
LICENSE = "MIT"

SRC_URI = "file://base.tar.gz"
SRC_URI:append:qemux86-64 = " file://x86-patch.patch"

DEPENDS = "base-dep"
DEPENDS:append:poky = " poky-dep"
"#;
        let recipe_path = layer_dir.join("test-recipe_1.0.bb");
        fs::write(&recipe_path, recipe_content).unwrap();

        // Create BuildContext with MACHINE and DISTRO
        let mut build_context = BuildContext::new();
        build_context.set_machine("qemux86-64".to_string());
        build_context.set_distro("poky".to_string());

        // Parse recipe
        let recipe = BitbakeRecipe::parse_file(&recipe_path).unwrap();

        // Create override resolver with build context
        let resolver = build_context.create_override_resolver(&recipe);

        // Verify overrides are properly configured
        let overrides = resolver.active_overrides();
        assert!(overrides.contains(&"qemux86-64".to_string()), "Should have qemux86-64 override");
        assert!(overrides.contains(&"poky".to_string()), "Should have poky override");
        assert!(overrides.contains(&"x86".to_string()), "Should auto-detect x86 from qemux86-64");
        assert!(overrides.contains(&"64".to_string()), "Should auto-detect 64 from qemux86-64");
    }

    #[test]
    fn test_build_context_machine_distro() {
        let mut ctx = BuildContext::new();

        ctx.set_machine("qemuarm64".to_string());
        ctx.set_distro("poky".to_string());

        assert_eq!(ctx.machine, Some("qemuarm64".to_string()));
        assert_eq!(ctx.distro, Some("poky".to_string()));
        assert_eq!(ctx.global_variables.get("MACHINE"), Some(&"qemuarm64".to_string()));
        assert_eq!(ctx.global_variables.get("DISTRO"), Some(&"poky".to_string()));
    }
}
