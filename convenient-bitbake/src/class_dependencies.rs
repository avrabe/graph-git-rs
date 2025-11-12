// BitBake class dependency mappings
// Maps class names to the build/runtime dependencies they add

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use crate::simple_python_eval::SimplePythonEvaluator;

/// Get build dependencies added by a class
pub fn get_class_build_deps(class_name: &str, distro_features: &str) -> Vec<String> {
    match class_name {
        // Build system classes
        "autotools" | "autotools-brokensep" => vec![
            "autoconf-native".to_string(),
            "automake-native".to_string(),
            "libtool-native".to_string(),
            "gnu-config-native".to_string(),
        ],

        "cmake" => vec![
            "cmake-native".to_string(),
            "ninja-native".to_string(),
        ],

        "meson" => vec![
            "meson-native".to_string(),
            "ninja-native".to_string(),
        ],

        "pkgconfig" => vec![
            "pkgconfig-native".to_string(),
        ],

        // Localization
        "gettext" => vec![
            "gettext-native".to_string(),
        ],

        // Systemd (conditional on DISTRO_FEATURES)
        "systemd" => {
            if distro_features.split_whitespace().any(|f| f == "systemd") {
                vec!["systemd".to_string()]
            } else {
                vec![]
            }
        },

        // Init scripts
        "update-rc.d" => vec![
            "update-rc.d-native".to_string(),
        ],

        // Python
        "python3-dir" | "python3native" | "python3targetconfig" => vec![
            "python3-native".to_string(),
        ],

        "setuptools3" | "setuptools3-base" | "python_setuptools_build_meta" => vec![
            "python3-setuptools-native".to_string(),
            "python3-wheel-native".to_string(),
        ],

        "python_pep517" => vec![
            "python3-build-native".to_string(),
            "python3-installer-native".to_string(),
        ],

        // Documentation
        "gtk-doc" => vec![
            "gtk-doc-native".to_string(),
        ],

        "texinfo" => vec![
            "texinfo-native".to_string(),
        ],

        "manpages" => vec![
            "groff-native".to_string(),
        ],

        // Kernel
        "kernel" | "module-base" => vec![
            "bc-native".to_string(),
            "bison-native".to_string(),
            "flex-native".to_string(),
        ],

        // Cargo/Rust
        "cargo" => vec![
            "cargo-native".to_string(),
        ],

        "rust-common" | "rust" => vec![
            "rust-native".to_string(),
        ],

        // Go
        "go" => vec![
            "go-native".to_string(),
        ],

        // Others
        "qemu" => vec![
            "qemu-native".to_string(),
        ],

        "cross-canadian" => vec![
            "nativesdk-gcc-cross-canadian-${TRANSLATED_TARGET_ARCH}".to_string(),
        ],

        // Classes that don't add build dependencies
        "allarch" | "native" | "packagegroup" | "nopackages" |
        "features_check" | "update-alternatives" | "useradd" |
        "systemd-boot" | "image" | "core-image" | "populate_sdk" |
        "deploy" | "devupstream" | "mirrors" | "sanity" => vec![],

        // Unknown class - no deps
        _ => vec![],
    }
}

/// Get runtime dependencies added by a class
pub fn get_class_runtime_deps(class_name: &str, distro_features: &str) -> Vec<String> {
    match class_name {
        "systemd" => {
            if distro_features.split_whitespace().any(|f| f == "systemd") {
                vec!["systemd".to_string()]
            } else {
                vec![]
            }
        },

        "update-rc.d" => vec![
            "update-rc.d".to_string(),
        ],

        "update-alternatives" => vec![
            "update-alternatives-opkg".to_string(),
        ],

        // Most classes don't add runtime deps
        _ => vec![],
    }
}

/// Parse inherit statement and return class names
pub fn parse_inherit_statement(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();

    // Handle "inherit class1 class2 class3"
    if let Some(rest) = trimmed.strip_prefix("inherit ") {
        let classes: Vec<String> = rest
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        if !classes.is_empty() {
            return Some(classes);
        }
    }

    None
}

/// Extract all inherited classes from content
pub fn extract_inherited_classes(content: &str) -> Vec<String> {
    let mut classes = Vec::new();

    for line in content.lines() {
        if let Some(line_classes) = parse_inherit_statement(line) {
            classes.extend(line_classes);
        }
    }

    classes
}

/// Parsed dependencies from a .bbclass file
#[derive(Debug, Clone, Default)]
pub struct ClassDependencies {
    pub build_deps: Vec<String>,
    pub runtime_deps: Vec<String>,
}

/// Parse a .bbclass file and extract dependencies
/// Phase 7f: Now supports Python expression evaluation in .bbclass files
pub fn parse_class_file(
    class_path: &Path,
    variables: &HashMap<String, String>,
) -> Option<ClassDependencies> {
    let content = fs::read_to_string(class_path).ok()?;

    let mut deps = ClassDependencies::default();

    // Create Python evaluator with recipe context
    let evaluator = SimplePythonEvaluator::new(variables.clone());

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse DEPENDS lines (Phase 7f: with Python evaluation)
        if let Some(extracted) = extract_depends_from_line(trimmed, "DEPENDS", &evaluator) {
            deps.build_deps.extend(extracted);
        }

        // Parse RDEPENDS lines (Phase 7f: with Python evaluation)
        if let Some(extracted) = extract_depends_from_line(trimmed, "RDEPENDS", &evaluator) {
            deps.runtime_deps.extend(extracted);
        }
    }

    Some(deps)
}

/// Extract dependencies from a single line
/// Handles: DEPENDS = "foo", DEPENDS += "foo", DEPENDS:append = "foo", etc.
/// Phase 7f: Now evaluates Python expressions using SimplePythonEvaluator
fn extract_depends_from_line(
    line: &str,
    var_name: &str,
    evaluator: &SimplePythonEvaluator,
) -> Option<Vec<String>> {
    // Handle various forms:
    // DEPENDS = "foo bar"
    // DEPENDS += "foo"
    // DEPENDS:append = " foo"
    // DEPENDS:prepend = "foo "
    // Phase 7f: DEPENDS:append = "${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'libsystemd', '', d)}"

    let trimmed = line.trim();

    // Check if line starts with DEPENDS (or DEPENDS:something)
    if !trimmed.starts_with(var_name) {
        return None;
    }

    // Find the assignment operator
    let after_var = if let Some(colon_pos) = trimmed.find(':') {
        // Handle DEPENDS:append, DEPENDS:prepend, etc.
        if let Some(eq_pos) = trimmed[colon_pos..].find('=') {
            &trimmed[colon_pos + eq_pos + 1..]
        } else {
            return None;
        }
    } else if let Some(eq_pos) = trimmed.find('=') {
        // Handle DEPENDS =, DEPENDS +=, etc.
        &trimmed[eq_pos + 1..]
    } else {
        return None;
    };

    // Extract value from quotes or direct assignment
    let mut value = clean_value(after_var);

    if value.is_empty() {
        return None;
    }

    // Phase 7f: Evaluate Python expressions if present
    if value.contains("${@") {
        // Try to evaluate the Python expression
        value = eval_python_in_value(&value, evaluator);

        // If evaluation resulted in empty string, no dependencies
        if value.is_empty() {
            return None;
        }
    }

    // Split by whitespace and filter out empty strings
    let deps: Vec<String> = value
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    if deps.is_empty() {
        None
    } else {
        Some(deps)
    }
}

/// Evaluate Python expressions in a value string (Phase 7f)
/// Finds all ${@...} expressions and evaluates them using SimplePythonEvaluator
fn eval_python_in_value(value: &str, evaluator: &SimplePythonEvaluator) -> String {
    let mut result = String::new();
    let mut start = 0;

    while let Some(expr_start) = value[start..].find("${@") {
        let abs_start = start + expr_start;

        // Add text before the expression
        result.push_str(&value[start..abs_start]);

        // Find the closing brace
        if let Some(expr_end) = find_matching_brace(&value[abs_start + 2..]) {
            let expr = &value[abs_start..abs_start + 3 + expr_end]; // Include ${@ and }

            // Try to evaluate the expression
            if let Some(evaluated) = evaluator.evaluate(expr) {
                result.push_str(&evaluated);
            } else {
                // Evaluation failed - keep original expression
                // But return empty to signal we can't process this
                return String::new();
            }

            start = abs_start + 3 + expr_end;
        } else {
            // No closing brace - keep original and continue
            result.push_str(&value[abs_start..]);
            break;
        }
    }

    // Add remaining text
    result.push_str(&value[start..]);

    result
}

/// Find the matching closing brace for a ${...} expression
/// Returns the position relative to the start (after ${@)
fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 1;
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

/// Clean a value by removing quotes and extra whitespace
fn clean_value(value: &str) -> String {
    let trimmed = value.trim();

    // Remove surrounding quotes
    let unquoted = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    unquoted.trim().to_string()
}

/// Find .bbclass file in search paths
pub fn find_class_file(class_name: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    let class_filename = format!("{}.bbclass", class_name);

    for base_path in search_paths {
        // Try classes-recipe/ subdirectory (modern Yocto layout)
        let recipe_class_path = base_path.join("classes-recipe").join(&class_filename);
        if recipe_class_path.exists() {
            return Some(recipe_class_path);
        }

        // Try classes/ subdirectory (older layout)
        let class_path = base_path.join("classes").join(&class_filename);
        if class_path.exists() {
            return Some(class_path);
        }

        // Try direct path
        let direct_path = base_path.join(&class_filename);
        if direct_path.exists() {
            return Some(direct_path);
        }
    }

    None
}

/// Get dependencies for a class, trying to parse .bbclass file first,
/// falling back to hardcoded mappings
/// Phase 7f: Now accepts variables for Python expression evaluation
pub fn get_class_deps_dynamic(
    class_name: &str,
    distro_features: &str,
    search_paths: &[PathBuf],
    variables: &HashMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    // Try to parse .bbclass file first (Phase 7f: with Python evaluation)
    if let Some(class_path) = find_class_file(class_name, search_paths) {
        if let Some(parsed) = parse_class_file(&class_path, variables) {
            // Successfully parsed - use those dependencies
            if !parsed.build_deps.is_empty() || !parsed.runtime_deps.is_empty() {
                return (parsed.build_deps, parsed.runtime_deps);
            }
        }
    }

    // Fall back to hardcoded mappings
    let build_deps = get_class_build_deps(class_name, distro_features);
    let runtime_deps = get_class_runtime_deps(class_name, distro_features);

    (build_deps, runtime_deps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_inherit_single() {
        let classes = parse_inherit_statement("inherit autotools").unwrap();
        assert_eq!(classes, vec!["autotools"]);
    }

    #[test]
    fn test_parse_inherit_multiple() {
        let classes = parse_inherit_statement("inherit autotools pkgconfig gettext").unwrap();
        assert_eq!(classes, vec!["autotools", "pkgconfig", "gettext"]);
    }

    #[test]
    fn test_autotools_deps() {
        let deps = get_class_build_deps("autotools", "");
        assert!(deps.contains(&"autoconf-native".to_string()));
        assert!(deps.contains(&"automake-native".to_string()));
        assert!(deps.contains(&"libtool-native".to_string()));
    }

    #[test]
    fn test_cmake_deps() {
        let deps = get_class_build_deps("cmake", "");
        assert!(deps.contains(&"cmake-native".to_string()));
        assert!(deps.contains(&"ninja-native".to_string()));
    }

    #[test]
    fn test_systemd_conditional() {
        // With systemd in DISTRO_FEATURES
        let deps = get_class_build_deps("systemd", "systemd pam ipv6");
        assert_eq!(deps, vec!["systemd"]);

        // Without systemd in DISTRO_FEATURES
        let deps = get_class_build_deps("systemd", "pam ipv6");
        assert_eq!(deps, Vec::<String>::new());
    }

    #[test]
    fn test_clean_value() {
        assert_eq!(clean_value("\"foo bar\""), "foo bar");
        assert_eq!(clean_value("'foo bar'"), "foo bar");
        assert_eq!(clean_value("  foo bar  "), "foo bar");
        assert_eq!(clean_value("\" foo bar \""), "foo bar");
    }

    #[test]
    fn test_extract_depends_simple() {
        let vars = HashMap::new();
        let evaluator = SimplePythonEvaluator::new(vars);
        let line = "DEPENDS = \"cmake-native ninja-native\"";
        let deps = extract_depends_from_line(line, "DEPENDS", &evaluator).unwrap();
        assert_eq!(deps, vec!["cmake-native", "ninja-native"]);
    }

    #[test]
    fn test_extract_depends_append() {
        let vars = HashMap::new();
        let evaluator = SimplePythonEvaluator::new(vars);
        let line = "DEPENDS:append = \" meson-native ninja-native\"";
        let deps = extract_depends_from_line(line, "DEPENDS", &evaluator).unwrap();
        assert_eq!(deps, vec!["meson-native", "ninja-native"]);
    }

    #[test]
    fn test_extract_depends_prepend() {
        let vars = HashMap::new();
        let evaluator = SimplePythonEvaluator::new(vars);
        let line = "DEPENDS:prepend = \"cmake-native \"";
        let deps = extract_depends_from_line(line, "DEPENDS", &evaluator).unwrap();
        assert_eq!(deps, vec!["cmake-native"]);
    }

    #[test]
    fn test_extract_depends_with_python_no_context() {
        // Without proper context, Python expression can't be evaluated
        let vars = HashMap::new();
        let evaluator = SimplePythonEvaluator::new(vars);
        let line = "DEPENDS:append = \"${@'qemu-native' if d.getVar('FOO') else ''}\"";
        let deps = extract_depends_from_line(line, "DEPENDS", &evaluator);
        assert_eq!(deps, None); // Can't evaluate without FOO variable
    }

    #[test]
    fn test_extract_depends_with_python_evaluated() {
        // Phase 7f: With proper context, Python expression IS evaluated
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam".to_string());
        let evaluator = SimplePythonEvaluator::new(vars);

        let line = "DEPENDS:append = \" ${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'libsystemd', '', d)}\"";
        let deps = extract_depends_from_line(line, "DEPENDS", &evaluator).unwrap();
        assert_eq!(deps, vec!["libsystemd"]);
    }

    #[test]
    fn test_extract_depends_with_python_empty_result() {
        // Python expression evaluates to empty string
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "pam ipv6".to_string());
        let evaluator = SimplePythonEvaluator::new(vars);

        let line = "DEPENDS:append = \"${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'libsystemd', '', d)}\"";
        let deps = extract_depends_from_line(line, "DEPENDS", &evaluator);
        assert_eq!(deps, None); // Empty result = no dependencies
    }

    #[test]
    fn test_parse_class_content() {
        let content = r#"
# This is a test class
DEPENDS:prepend = "cmake-native "
DEPENDS:append = " ninja-native"
RDEPENDS:${PN} = "bash"
"#;

        // Create a temporary file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test.bbclass");
        std::fs::write(&temp_file, content).unwrap();

        let vars = HashMap::new();
        let parsed = parse_class_file(&temp_file, &vars).unwrap();

        assert_eq!(parsed.build_deps.len(), 2);
        assert!(parsed.build_deps.contains(&"cmake-native".to_string()));
        assert!(parsed.build_deps.contains(&"ninja-native".to_string()));
        assert_eq!(parsed.runtime_deps, vec!["bash"]);

        // Clean up
        std::fs::remove_file(&temp_file).ok();
    }

    #[test]
    fn test_parse_class_with_python_expressions() {
        // Phase 7f: Test Python expression evaluation in .bbclass files
        let content = r#"
# Conditional dependencies based on DISTRO_FEATURES
DEPENDS:append = " ${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'libsystemd', '', d)}"
DEPENDS:append = " ${@bb.utils.contains('DISTRO_FEATURES', 'pam', 'libpam', '', d)}"
RDEPENDS:${PN} = "${@bb.utils.filter('DISTRO_FEATURES', 'systemd pam wayland', d)}"
"#;

        // Create a temporary file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_python.bbclass");
        std::fs::write(&temp_file, content).unwrap();

        // Test with systemd in DISTRO_FEATURES
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam ipv6".to_string());
        let parsed = parse_class_file(&temp_file, &vars).unwrap();

        // Should include libsystemd and libpam
        assert!(parsed.build_deps.contains(&"libsystemd".to_string()));
        assert!(parsed.build_deps.contains(&"libpam".to_string()));

        // Runtime deps should be filtered list
        assert!(parsed.runtime_deps.contains(&"systemd".to_string()));
        assert!(parsed.runtime_deps.contains(&"pam".to_string()));

        // Test without systemd in DISTRO_FEATURES
        let mut vars2 = HashMap::new();
        vars2.insert("DISTRO_FEATURES".to_string(), "ipv6 bluetooth".to_string());
        let parsed2 = parse_class_file(&temp_file, &vars2).unwrap();

        // Should NOT include systemd deps
        assert!(!parsed2.build_deps.contains(&"libsystemd".to_string()));
        assert!(!parsed2.build_deps.contains(&"libpam".to_string()));

        // Clean up
        std::fs::remove_file(&temp_file).ok();
    }

    #[test]
    fn test_find_matching_brace() {
        assert_eq!(find_matching_brace("test}"), Some(4));
        assert_eq!(find_matching_brace("test{nested}end}"), Some(15));
        assert_eq!(find_matching_brace("'}'} more"), Some(3));
        assert_eq!(find_matching_brace("test"), None);
    }

    #[test]
    fn test_eval_python_in_value() {
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam".to_string());
        let evaluator = SimplePythonEvaluator::new(vars);

        // Test single expression
        let value = "${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'yes', 'no', d)}";
        let result = eval_python_in_value(value, &evaluator);
        assert_eq!(result, "yes");

        // Test with surrounding text
        let value = "before ${@bb.utils.contains('DISTRO_FEATURES', 'pam', 'middle', 'skip', d)} after";
        let result = eval_python_in_value(value, &evaluator);
        assert_eq!(result, "before middle after");

        // Test multiple expressions
        let value = "${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'sys', '', d)} ${@bb.utils.contains('DISTRO_FEATURES', 'pam', 'auth', '', d)}";
        let result = eval_python_in_value(value, &evaluator);
        assert_eq!(result, "sys auth");
    }
}
