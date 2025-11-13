// OVERRIDES resolution for BitBake variables
// Handles :append, :prepend, :remove, and override-qualified variables

use crate::SimpleResolver;
use std::collections::HashMap;
use tracing::debug;

/// Override operation type
#[derive(Debug, Clone, PartialEq)]
pub enum OverrideOp {
    /// Simple assignment (=, :=)
    Assign,
    /// Append operation (+=, :append)
    Append,
    /// Prepend operation (=+, :prepend)
    Prepend,
    /// Remove operation (:remove)
    Remove,
    /// Weak default (?=)
    WeakDefault,
    /// Immediate weak default (??=)
    ImmediateWeakDefault,
}

/// A variable assignment with override information
#[derive(Debug, Clone)]
pub struct OverrideAssignment {
    /// Variable name (without override suffix)
    pub var_name: String,
    /// Value to assign/append/prepend
    pub value: String,
    /// Operation type
    pub operation: OverrideOp,
    /// Override qualifiers (e.g., ["machine", "x86"] for VAR:machine:x86)
    pub overrides: Vec<String>,
}

impl OverrideAssignment {
    /// Parse a variable name with potential override qualifiers
    /// e.g., "DEPENDS:append:x86" -> ("DEPENDS", [Append], ["x86"])
    pub fn parse(var_name: &str, value: String, op: OverrideOp) -> Self {
        let parts: Vec<&str> = var_name.split(':').collect();

        if parts.is_empty() {
            return Self {
                var_name: var_name.to_string(),
                value,
                operation: op,
                overrides: Vec::new(),
            };
        }

        let base_name = parts[0].to_string();
        let mut overrides = Vec::new();
        let mut actual_op = op;

        // Process override qualifiers
        for part in &parts[1..] {
            match *part {
                "append" => actual_op = OverrideOp::Append,
                "prepend" => actual_op = OverrideOp::Prepend,
                "remove" => actual_op = OverrideOp::Remove,
                _ => overrides.push(part.to_string()),
            }
        }

        Self {
            var_name: base_name,
            value,
            operation: actual_op,
            overrides,
        }
    }

    /// Check if this assignment applies given active overrides
    pub fn applies_to(&self, active_overrides: &[String]) -> bool {
        if self.overrides.is_empty() {
            return true; // No qualifiers = always applies
        }

        // All overrides must be active for this assignment to apply
        self.overrides
            .iter()
            .all(|o| active_overrides.contains(o))
    }
}

/// Resolver with OVERRIDES support
pub struct OverrideResolver {
    /// Base variable resolver
    resolver: SimpleResolver,
    /// All assignments collected from recipes (including overridden ones)
    assignments: HashMap<String, Vec<OverrideAssignment>>,
    /// Active overrides (from OVERRIDES variable)
    active_overrides: Vec<String>,
}

impl OverrideResolver {
    /// Create a new override resolver
    pub fn new(resolver: SimpleResolver) -> Self {
        Self {
            resolver,
            assignments: HashMap::new(),
            active_overrides: Vec::new(),
        }
    }

    /// Set the OVERRIDES variable and parse it
    /// OVERRIDES format: "colon:separated:list"
    pub fn set_overrides(&mut self, overrides: &str) {
        self.active_overrides = overrides
            .split(':')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        debug!("Active overrides: {:?}", self.active_overrides);
    }

    /// Build overrides from common BitBake variables
    /// Typical order: MACHINEOVERRIDES:DISTROOVERRIDES:OVERRIDES
    pub fn build_overrides_from_context(
        &mut self,
        machine: Option<&str>,
        distro: Option<&str>,
        additional: &[String],
    ) {
        let mut overrides = Vec::new();

        // Add machine-specific overrides
        if let Some(machine) = machine {
            overrides.push(machine.to_string());
            // Common machine arch overrides
            if machine.contains("arm") {
                overrides.push("arm".to_string());
            }
            if machine.contains("x86") {
                overrides.push("x86".to_string());
            }
            if machine.contains("64") {
                overrides.push("64".to_string());
            }
        }

        // Add distro-specific overrides
        if let Some(distro) = distro {
            overrides.push(distro.to_string());
        }

        // Add additional overrides
        overrides.extend_from_slice(additional);

        // Common default overrides
        overrides.extend_from_slice(&[
            "class-target".to_string(),
            "forcevariable".to_string(),
        ]);

        self.active_overrides = overrides;
        debug!("Built overrides from context: {:?}", self.active_overrides);
    }

    /// Add a variable assignment
    pub fn add_assignment(
        &mut self,
        var_name: &str,
        value: String,
        operation: OverrideOp,
    ) {
        let assignment = OverrideAssignment::parse(var_name, value, operation);
        self.assignments
            .entry(assignment.var_name.clone())
            .or_default()
            .push(assignment);
    }

    /// Resolve a variable with override application
    pub fn resolve(&self, var_name: &str) -> Option<String> {
        // Get all assignments for this variable
        let assignments = match self.assignments.get(var_name) {
            Some(a) => a,
            None => return self.resolver.get(var_name).map(|s| s.to_string()),
        };

        // Start with base value from resolver
        let mut result = self.resolver.get(var_name).map(|s| s.to_string());
        let mut has_value = result.is_some();

        // Apply assignments in order
        for assignment in assignments {
            // Check if this assignment applies given active overrides
            if !assignment.applies_to(&self.active_overrides) {
                continue;
            }

            match assignment.operation {
                OverrideOp::Assign => {
                    result = Some(assignment.value.clone());
                    has_value = true;
                }
                OverrideOp::WeakDefault => {
                    if !has_value {
                        result = Some(assignment.value.clone());
                        has_value = true;
                    }
                }
                OverrideOp::ImmediateWeakDefault => {
                    if !has_value {
                        result = Some(assignment.value.clone());
                        has_value = true;
                    }
                }
                OverrideOp::Append => {
                    if let Some(current) = &result {
                        result = Some(format!("{} {}", current, assignment.value));
                    } else {
                        result = Some(assignment.value.clone());
                        has_value = true;
                    }
                }
                OverrideOp::Prepend => {
                    if let Some(current) = &result {
                        result = Some(format!("{} {}", assignment.value, current));
                    } else {
                        result = Some(assignment.value.clone());
                        has_value = true;
                    }
                }
                OverrideOp::Remove => {
                    if let Some(current) = &result {
                        // Remove all occurrences of the value
                        let parts: Vec<&str> = current
                            .split_whitespace()
                            .filter(|p| *p != assignment.value.trim())
                            .collect();
                        result = Some(parts.join(" "));
                    }
                }
            }
        }

        // Expand variables in the final result
        result.map(|v| self.resolver.resolve(&v))
    }

    /// Resolve all variables into a HashMap
    pub fn resolve_all(&self) -> HashMap<String, String> {
        let mut resolved = HashMap::new();

        // Get all variable names from both resolver and assignments
        let mut all_vars: Vec<String> = self.assignments.keys().cloned().collect();

        // Add variables from base resolver
        for key in self.resolver.variables().keys() {
            if !all_vars.contains(key) {
                all_vars.push(key.clone());
            }
        }

        // Resolve each variable
        for var_name in all_vars {
            if let Some(value) = self.resolve(&var_name) {
                resolved.insert(var_name, value);
            }
        }

        resolved
    }

    /// Get a reference to the base resolver
    pub fn base_resolver(&self) -> &SimpleResolver {
        &self.resolver
    }

    /// Get active overrides
    pub fn active_overrides(&self) -> &[String] {
        &self.active_overrides
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BitbakeRecipe, RecipeType};
    use std::path::PathBuf;

    fn create_test_resolver() -> SimpleResolver {
        let recipe = BitbakeRecipe {
            file_path: PathBuf::from("/tmp/test.bb"),
            recipe_type: RecipeType::Recipe,
            package_name: Some("test-package".to_string()),
            ..Default::default()
        };
        SimpleResolver::new(&recipe)
    }

    #[test]
    fn test_override_assignment_parse() {
        let assign = OverrideAssignment::parse(
            "DEPENDS:append:x86",
            "extra-dep".to_string(),
            OverrideOp::Assign,
        );

        assert_eq!(assign.var_name, "DEPENDS");
        assert_eq!(assign.operation, OverrideOp::Append);
        assert_eq!(assign.overrides, vec!["x86"]);
        assert_eq!(assign.value, "extra-dep");
    }

    #[test]
    fn test_simple_append() {
        let resolver = create_test_resolver();
        let mut override_resolver = OverrideResolver::new(resolver);

        override_resolver.add_assignment("DEPENDS", "base-dep".to_string(), OverrideOp::Assign);
        override_resolver.add_assignment(
            "DEPENDS:append",
            "extra-dep".to_string(),
            OverrideOp::Assign,
        );

        let result = override_resolver.resolve("DEPENDS");
        assert_eq!(result, Some("base-dep extra-dep".to_string()));
    }

    #[test]
    fn test_conditional_override() {
        let resolver = create_test_resolver();
        let mut override_resolver = OverrideResolver::new(resolver);

        override_resolver.set_overrides("x86:class-target");

        override_resolver.add_assignment("DEPENDS", "base-dep".to_string(), OverrideOp::Assign);
        override_resolver.add_assignment(
            "DEPENDS:append:x86",
            "x86-dep".to_string(),
            OverrideOp::Assign,
        );
        override_resolver.add_assignment(
            "DEPENDS:append:arm",
            "arm-dep".to_string(),
            OverrideOp::Assign,
        );

        let result = override_resolver.resolve("DEPENDS");
        // Should include x86-dep but not arm-dep
        assert_eq!(result, Some("base-dep x86-dep".to_string()));
    }

    #[test]
    fn test_prepend_operation() {
        let resolver = create_test_resolver();
        let mut override_resolver = OverrideResolver::new(resolver);

        override_resolver.add_assignment("PATH", "/usr/bin".to_string(), OverrideOp::Assign);
        override_resolver.add_assignment(
            "PATH:prepend",
            "/opt/bin".to_string(),
            OverrideOp::Assign,
        );

        let result = override_resolver.resolve("PATH");
        assert_eq!(result, Some("/opt/bin /usr/bin".to_string()));
    }

    #[test]
    fn test_remove_operation() {
        let resolver = create_test_resolver();
        let mut override_resolver = OverrideResolver::new(resolver);

        override_resolver.add_assignment(
            "DISTRO_FEATURES",
            "acl ipv4 ipv6 bluetooth".to_string(),
            OverrideOp::Assign,
        );
        override_resolver.add_assignment(
            "DISTRO_FEATURES:remove",
            "bluetooth".to_string(),
            OverrideOp::Assign,
        );

        let result = override_resolver.resolve("DISTRO_FEATURES");
        assert_eq!(result, Some("acl ipv4 ipv6".to_string()));
    }

    #[test]
    fn test_weak_default() {
        let resolver = create_test_resolver();
        let mut override_resolver = OverrideResolver::new(resolver);

        // Weak default should not override existing value
        override_resolver.add_assignment("VAR", "existing".to_string(), OverrideOp::Assign);
        override_resolver.add_assignment("VAR", "default".to_string(), OverrideOp::WeakDefault);

        let result = override_resolver.resolve("VAR");
        assert_eq!(result, Some("existing".to_string()));

        // But should set if no existing value
        let result2 = override_resolver.resolve("UNSET_VAR");
        assert_eq!(result2, None); // No assignment for UNSET_VAR
    }

    #[test]
    fn test_multiple_qualifiers() {
        let resolver = create_test_resolver();
        let mut override_resolver = OverrideResolver::new(resolver);

        override_resolver.set_overrides("qemuarm:arm:class-target");

        override_resolver.add_assignment("VAR", "base".to_string(), OverrideOp::Assign);
        override_resolver.add_assignment(
            "VAR:append:qemuarm:arm",
            "arm-specific".to_string(),
            OverrideOp::Assign,
        );

        // Both qemuarm and arm are active, so should apply
        let result = override_resolver.resolve("VAR");
        assert_eq!(result, Some("base arm-specific".to_string()));
    }

    #[test]
    fn test_build_overrides_from_context() {
        let resolver = create_test_resolver();
        let mut override_resolver = OverrideResolver::new(resolver);

        override_resolver.build_overrides_from_context(
            Some("qemuarm64"),
            Some("poky"),
            &["custom-override".to_string()],
        );

        let overrides = override_resolver.active_overrides();
        assert!(overrides.contains(&"qemuarm64".to_string()));
        assert!(overrides.contains(&"poky".to_string()));
        assert!(overrides.contains(&"arm".to_string())); // Auto-detected from qemuarm64
        assert!(overrides.contains(&"64".to_string())); // Auto-detected from qemuarm64
        assert!(overrides.contains(&"custom-override".to_string()));
    }
}
