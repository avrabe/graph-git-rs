//! BitBake script preprocessor
//!
//! Handles BitBake-specific syntax before script execution:
//! - ${@python_expression} - Inline Python evaluation using RustPython
//! - ${VAR[flag]} - Variable flags conversion
//! - Variable expansion from recipe DataStore
//!
//! This allows BitBake recipes to be executed as clean bash/python scripts.

use regex::Regex;
use std::collections::HashMap;
use tracing::{debug, warn};

#[cfg(feature = "python-execution")]
use crate::python_executor::PythonExecutor;

/// Preprocesses BitBake scripts to handle special syntax
pub struct ScriptPreprocessor {
    /// Variables from recipe parsing (DataStore)
    datastore: HashMap<String, String>,

    /// Python executor for inline expressions
    #[cfg(feature = "python-execution")]
    python_executor: Option<PythonExecutor>,
}

impl ScriptPreprocessor {
    /// Create new preprocessor with recipe variables
    pub fn new(vars: HashMap<String, String>) -> Self {
        #[cfg(feature = "python-execution")]
        let python_executor = Some(PythonExecutor::new());

        Self {
            datastore: vars,
            #[cfg(feature = "python-execution")]
            python_executor,
        }
    }

    /// Preprocess BitBake script to bash-compatible form
    ///
    /// Handles:
    /// 1. Python inline expressions: ${@...} → evaluated result
    /// 2. Variable flags: ${VAR[flag]} → ${VAR_flag}
    /// 3. Variable expansion: ${VAR} → value from datastore
    pub fn preprocess(&self, script: &str) -> Result<String, String> {
        let mut processed = script.to_string();

        // Count transformations for logging
        let original_len = processed.len();

        // 1. Expand Python inline expressions first (they may contain variables)
        processed = self.expand_python_expressions(&processed)?;

        // 2. Convert variable flag syntax
        processed = self.convert_variable_flags(&processed);

        // 3. Expand simple variable references (optional, bash can do this)
        // Only expand if we have the value to avoid breaking ${VAR:-default} patterns
        // processed = self.expand_variables(&processed);

        debug!(
            "Script preprocessing: {} bytes → {} bytes",
            original_len,
            processed.len()
        );

        Ok(processed)
    }

    /// Expand ${@python_code} using RustPython
    ///
    /// Examples:
    /// - ${@d.getVar('CFLAGS')} → "-O2 -pipe"
    /// - ${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'yes', 'no', d)} → "yes"
    #[cfg(feature = "python-execution")]
    fn expand_python_expressions(&self, script: &str) -> Result<String, String> {
        // Match ${@...} including nested braces
        // This regex handles simple cases; complex nesting needs a parser
        let re = Regex::new(r"\$\{@([^}]+)\}").map_err(|e| e.to_string())?;

        let mut result = script.to_string();
        let mut replacements = 0;

        for cap in re.captures_iter(script) {
            let full_match = cap.get(0).unwrap().as_str();
            let python_code = cap.get(1).unwrap().as_str();

            debug!("Evaluating Python expression: {}", python_code);

            if let Some(ref executor) = self.python_executor {
                match executor.execute(python_code, &self.datastore) {
                    Ok(output) => {
                        let value = output.stdout.trim();
                        debug!("  → Result: {}", value);

                        result = result.replace(full_match, value);
                        replacements += 1;
                    }
                    Err(e) => {
                        // Log warning but continue (BitBake's behavior)
                        warn!("Failed to evaluate Python expression '{}': {}", python_code, e);

                        // Replace with empty string (BitBake behavior)
                        result = result.replace(full_match, "");
                    }
                }
            }
        }

        if replacements > 0 {
            debug!("Expanded {} Python expressions", replacements);
        }

        Ok(result)
    }

    /// Fallback when Python execution is not available - just remove expressions
    #[cfg(not(feature = "python-execution"))]
    fn expand_python_expressions(&self, script: &str) -> Result<String, String> {
        // Fast path: if no Python expressions, return early
        if !script.contains("${@") {
            return Ok(script.to_string());
        }

        // Remove Python expressions without logging (too verbose)
        let re = Regex::new(r"\$\{@([^}]+)\}").map_err(|e| e.to_string())?;
        let result = re.replace_all(script, "").to_string();

        Ok(result)
    }

    /// Convert ${VAR[flag]} to ${VAR_flag}
    ///
    /// BitBake uses square brackets for variable flags/metadata.
    /// Convert to underscore notation that bash can handle.
    ///
    /// Examples:
    /// - ${DEPENDS[depends]} → ${DEPENDS_depends}
    /// - ${SRCPV[vardeps]} → ${SRCPV_vardeps}
    fn convert_variable_flags(&self, script: &str) -> String {
        let re = Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\[([a-z_]+)\]\}").unwrap();

        let mut count = 0;
        let result = re.replace_all(script, |caps: &regex::Captures| {
            let var_name = &caps[1];
            let flag_name = &caps[2];
            count += 1;
            format!("${{{}_{}}}", var_name, flag_name)
        }).to_string();

        if count > 0 {
            debug!("Converted {} variable flag expressions", count);
        }

        result
    }

    /// Expand ${VAR} using datastore (optional, usually bash does this)
    ///
    /// Only use this if we want to pre-expand variables.
    /// Usually better to let bash do it at runtime.
    #[allow(dead_code)]
    fn expand_variables(&self, script: &str) -> String {
        let re = Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").unwrap();

        re.replace_all(script, |caps: &regex::Captures| {
            let var_name = &caps[1];

            // Look up in datastore
            if let Some(value) = self.datastore.get(var_name) {
                value.clone()
            } else {
                // Keep original if not found (let bash handle it)
                caps[0].to_string()
            }
        }).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_variable_flags() {
        let vars = HashMap::new();
        let preprocessor = ScriptPreprocessor::new(vars);

        let input = r#"DEPENDS="${DEPENDS[depends]}" FLAGS="${SRCPV[vardeps]}""#;
        let output = preprocessor.convert_variable_flags(input);

        assert_eq!(output, r#"DEPENDS="${DEPENDS_depends}" FLAGS="${SRCPV_vardeps}""#);
    }

    #[test]
    fn test_preprocess_no_python() {
        let vars = HashMap::new();
        let preprocessor = ScriptPreprocessor::new(vars);

        // Script with variable flag
        let input = r#"
            CFLAGS="${CFLAGS[extra]}"
            echo "Hello"
        "#;

        let output = preprocessor.preprocess(input).unwrap();

        // Variable flag should be converted
        assert!(output.contains("${CFLAGS_extra}"));
        assert!(!output.contains("[extra]"));
    }

    #[cfg(feature = "python-execution")]
    #[test]
    fn test_expand_python_expression() {
        let mut vars = HashMap::new();
        vars.insert("CFLAGS".to_string(), "-O2 -pipe".to_string());
        vars.insert("DEBUG_BUILD".to_string(), "0".to_string());

        let preprocessor = ScriptPreprocessor::new(vars);

        // Test d.getVar()
        let input = r#"FLAGS="${@d.getVar('CFLAGS')}""#;
        let output = preprocessor.expand_python_expressions(input).unwrap();

        // Should expand to the value
        assert!(output.contains("-O2 -pipe"));
        assert!(!output.contains("${@"));
    }

    #[test]
    fn test_preprocess_real_script() {
        let mut vars = HashMap::new();
        vars.insert("PN".to_string(), "busybox".to_string());
        vars.insert("PV".to_string(), "1.36.1".to_string());

        let preprocessor = ScriptPreprocessor::new(vars);

        let script = r#"
            do_configure() {
                # Variable flag usage
                DEPENDS="${DEPENDS[depends]}"

                # Regular variable
                echo "Building ${PN}-${PV}"

                ./configure
            }
        "#;

        let processed = preprocessor.preprocess(script).unwrap();

        // Variable flags should be converted
        assert!(processed.contains("${DEPENDS_depends}"));
        assert!(!processed.contains("[depends]"));

        // Rest should be unchanged
        assert!(processed.contains("./configure"));
    }
}
