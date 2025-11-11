// BitBake class dependency mappings
// Maps class names to the build/runtime dependencies they add

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;

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
pub fn parse_class_file(class_path: &Path) -> Option<ClassDependencies> {
    let content = fs::read_to_string(class_path).ok()?;

    let mut deps = ClassDependencies::default();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse DEPENDS lines
        if let Some(extracted) = extract_depends_from_line(trimmed, "DEPENDS") {
            deps.build_deps.extend(extracted);
        }

        // Parse RDEPENDS lines
        if let Some(extracted) = extract_depends_from_line(trimmed, "RDEPENDS") {
            deps.runtime_deps.extend(extracted);
        }
    }

    Some(deps)
}

/// Extract dependencies from a single line
/// Handles: DEPENDS = "foo", DEPENDS += "foo", DEPENDS:append = "foo", etc.
fn extract_depends_from_line(line: &str, var_name: &str) -> Option<Vec<String>> {
    // Handle various forms:
    // DEPENDS = "foo bar"
    // DEPENDS += "foo"
    // DEPENDS:append = " foo"
    // DEPENDS:prepend = "foo "

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
    let value = clean_value(after_var);

    if value.is_empty() {
        return None;
    }

    // Skip lines with Python expressions (we can't evaluate those statically)
    if value.contains("${@") {
        return None;
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
pub fn get_class_deps_dynamic(
    class_name: &str,
    distro_features: &str,
    search_paths: &[PathBuf],
) -> (Vec<String>, Vec<String>) {
    // Try to parse .bbclass file first
    if let Some(class_path) = find_class_file(class_name, search_paths) {
        if let Some(parsed) = parse_class_file(&class_path) {
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
        let line = "DEPENDS = \"cmake-native ninja-native\"";
        let deps = extract_depends_from_line(line, "DEPENDS").unwrap();
        assert_eq!(deps, vec!["cmake-native", "ninja-native"]);
    }

    #[test]
    fn test_extract_depends_append() {
        let line = "DEPENDS:append = \" meson-native ninja-native\"";
        let deps = extract_depends_from_line(line, "DEPENDS").unwrap();
        assert_eq!(deps, vec!["meson-native", "ninja-native"]);
    }

    #[test]
    fn test_extract_depends_prepend() {
        let line = "DEPENDS:prepend = \"cmake-native \"";
        let deps = extract_depends_from_line(line, "DEPENDS").unwrap();
        assert_eq!(deps, vec!["cmake-native"]);
    }

    #[test]
    fn test_extract_depends_with_python() {
        // Should skip Python expressions
        let line = "DEPENDS:append = \"${@'qemu-native' if d.getVar('FOO') else ''}\"";
        let deps = extract_depends_from_line(line, "DEPENDS");
        assert_eq!(deps, None);
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

        let parsed = parse_class_file(&temp_file).unwrap();

        assert_eq!(parsed.build_deps.len(), 2);
        assert!(parsed.build_deps.contains(&"cmake-native".to_string()));
        assert!(parsed.build_deps.contains(&"ninja-native".to_string()));
        assert_eq!(parsed.runtime_deps, vec!["bash"]);

        // Clean up
        std::fs::remove_file(&temp_file).ok();
    }
}
