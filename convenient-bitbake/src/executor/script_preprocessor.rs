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
use tracing::{debug, trace, warn};

use crate::python_executor::PythonExecutor;
use crate::simple_python_eval::SimplePythonEvaluator;

/// Preprocesses BitBake scripts to handle special syntax
pub struct ScriptPreprocessor {
    /// Variables from recipe parsing (DataStore)
    datastore: HashMap<String, String>,

    /// Python executor for inline expressions
    python_executor: PythonExecutor,
}

impl ScriptPreprocessor {
    /// Create new preprocessor with recipe variables
    pub fn new(vars: HashMap<String, String>) -> Self {
        Self {
            datastore: vars,
            python_executor: PythonExecutor::new(),
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

    /// Expand ${@python_code} using SimplePythonEvaluator or RustPython
    ///
    /// Examples:
    /// - ${@d.getVar('CFLAGS')} → "-O2 -pipe"
    /// - ${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'yes', 'no', d)} → "yes"
    fn expand_python_expressions(&self, script: &str) -> Result<String, String> {
        // Fast path: if no Python expressions, return early
        if !script.contains("${@") {
            return Ok(script.to_string());
        }

        // Match ${@...} including nested braces
        // This regex handles simple cases; complex nesting needs a parser
        let re = Regex::new(r"\$\{@([^}]+)\}").map_err(|e| e.to_string())?;

        let mut result = script.to_string();
        let mut replacements = 0;
        let mut failed_expressions = 0;

        // Create SimplePythonEvaluator for common BitBake patterns
        let simple_eval = SimplePythonEvaluator::new(self.datastore.clone());

        for cap in re.captures_iter(script) {
            let full_match = cap.get(0).unwrap().as_str();
            let python_expr = cap.get(1).unwrap().as_str();

            trace!("Evaluating Python expression: {}", python_expr);

            // Try SimplePythonEvaluator first (handles common BitBake patterns)
            let evaluation_result = if let Some(value) = simple_eval.evaluate(full_match) {
                debug!("  → SimplePythonEvaluator result: {}", value);
                Ok(value)
            } else {
                // Fallback to RustPython for complex expressions
                self.python_executor.eval(python_expr, &self.datastore)
            };

            match evaluation_result {
                Ok(value) => {
                    trace!("  → Result: {}", value);
                    result = result.replace(full_match, &value);
                    replacements += 1;
                }
                Err(e) => {
                    // Many expressions can't be evaluated without full BitBake context
                    // Log at trace level instead of warn to reduce noise
                    trace!("Can't evaluate Python expression '{}': {}", python_expr, e);
                    failed_expressions += 1;
                    // Replace with empty string (BitBake behavior)
                    result = result.replace(full_match, "");
                }
            }
        }

        if replacements > 0 {
            debug!("Expanded {} Python expressions ({} couldn't be evaluated)",
                   replacements, failed_expressions);
        }

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

    #[test]
    fn test_bb_utils_contains() {
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd x11 wayland".to_string());
        vars.insert("PACKAGECONFIG".to_string(), "ssl ipv6".to_string());

        let preprocessor = ScriptPreprocessor::new(vars);

        // Test bb.utils.contains with systemd (present)
        let input1 = r#"INIT_SYSTEM="${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'systemd', 'sysvinit', d)}""#;
        let output1 = preprocessor.preprocess(input1).unwrap();
        assert!(output1.contains("systemd"), "Expected 'systemd' but got: {}", output1);
        assert!(!output1.contains("sysvinit"));

        // Test bb.utils.contains with missing feature
        let input2 = r#"HAS_ALSA="${@bb.utils.contains('DISTRO_FEATURES', 'alsa', 'yes', 'no', d)}""#;
        let output2 = preprocessor.preprocess(input2).unwrap();
        assert!(output2.contains("no"), "Expected 'no' but got: {}", output2);
        assert!(!output2.contains("yes"));

        // Test with PACKAGECONFIG
        let input3 = r#"SSL_FLAGS="${@bb.utils.contains('PACKAGECONFIG', 'ssl', '--with-ssl', '', d)}""#;
        let output3 = preprocessor.preprocess(input3).unwrap();
        assert!(output3.contains("--with-ssl"), "Expected '--with-ssl' but got: {}", output3);
    }

    #[test]
    fn test_preprocessing_performance() {
        use std::time::Instant;

        let mut vars = HashMap::new();
        vars.insert("PN".to_string(), "test-package".to_string());
        vars.insert("PV".to_string(), "1.0".to_string());
        vars.insert("DISTRO_FEATURES".to_string(), "systemd x11 wayland".to_string());

        let preprocessor = ScriptPreprocessor::new(vars);

        // Test script with multiple Python expressions
        let script = r#"
            do_configure() {
                FLAGS="${CFLAGS[extra]}"
                INIT="${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'systemd', 'sysvinit', d)}"
                HAS_X11="${@bb.utils.contains('DISTRO_FEATURES', 'x11', 'yes', 'no', d)}"
                PKG_NAME="${@d.getVar('PN')}"
                VERSION="${@d.getVar('PV')}"
                ./configure --prefix=/usr
            }
        "#;

        // Warm-up run
        let _ = preprocessor.preprocess(script).unwrap();

        // Benchmark
        let iterations = 100;
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = preprocessor.preprocess(script).unwrap();
        }
        let elapsed = start.elapsed();
        let avg_per_run = elapsed / iterations;

        println!("Preprocessing performance:");
        println!("  Total: {:?} for {} iterations", elapsed, iterations);
        println!("  Average: {:?} per script", avg_per_run);
        println!("  Throughput: {:.0} scripts/sec", 1000.0 / avg_per_run.as_millis() as f64);

        // Assert reasonable performance (< 50ms per script on average)
        // Note: This is environment-dependent and set conservatively for CI
        assert!(avg_per_run.as_millis() < 50, "Preprocessing too slow: {:?}", avg_per_run);
    }
}
