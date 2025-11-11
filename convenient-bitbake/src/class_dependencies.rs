// BitBake class dependency mappings
// Maps class names to the build/runtime dependencies they add

use std::collections::HashMap;

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
}
