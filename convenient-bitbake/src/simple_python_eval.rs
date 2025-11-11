// Simple Python expression evaluator for common BitBake patterns
// This handles bb.utils.contains() and bb.utils.filter() without requiring Python execution

use std::collections::HashMap;
use tracing::debug;

/// Simple Python expression evaluator for BitBake ${@...} expressions
#[derive(Debug, Clone)]
pub struct SimplePythonEvaluator {
    /// Variable context for evaluation
    variables: HashMap<String, String>,
}

impl SimplePythonEvaluator {
    /// Create a new evaluator with the given variable context
    pub fn new(variables: HashMap<String, String>) -> Self {
        Self { variables }
    }

    /// Evaluate a Python expression, returning Some(result) if we can handle it,
    /// or None if we should keep the original expression
    pub fn evaluate(&self, expr: &str) -> Option<String> {
        let trimmed = expr.trim();

        // Strip ${@ and } if present
        let inner = if trimmed.starts_with("${@") && trimmed.ends_with('}') {
            &trimmed[3..trimmed.len() - 1]
        } else if trimmed.starts_with("@") {
            &trimmed[1..]
        } else {
            trimmed
        };

        // Handle bb.utils.contains
        if inner.contains("bb.utils.contains") {
            return self.eval_contains(inner);
        }

        // Handle bb.utils.filter
        if inner.contains("bb.utils.filter") {
            return self.eval_filter(inner);
        }

        // Can't handle this expression
        None
    }

    /// Evaluate bb.utils.contains(var, item, true_val, false_val, d)
    /// Returns true_val if item is in var (space-separated), false_val otherwise
    fn eval_contains(&self, expr: &str) -> Option<String> {
        debug!("Evaluating bb.utils.contains: {}", expr);

        // Parse: bb.utils.contains('VAR', 'item', 'true_value', 'false_value', d)
        // We need to extract the four string arguments
        let args = self.parse_function_args(expr, "bb.utils.contains")?;

        if args.len() < 4 {
            debug!("bb.utils.contains: Expected 4+ args, got {}", args.len());
            return None;
        }

        let var_name = &args[0];
        let search_item = &args[1];
        let true_value = &args[2];
        let false_value = &args[3];

        debug!(
            "  var={}, item={}, true={}, false={}",
            var_name, search_item, true_value, false_value
        );

        // Look up the variable
        let var_value = self.variables.get(var_name)?;

        // Check if search_item is in var_value (space-separated)
        let contains = var_value
            .split_whitespace()
            .any(|item| item == search_item);

        let result = if contains {
            true_value.clone()
        } else {
            false_value.clone()
        };

        debug!("  var_value={}, contains={}, result={}", var_value, contains, result);

        Some(result)
    }

    /// Evaluate bb.utils.filter(var, items, d)
    /// Returns items that exist in var (space-separated)
    fn eval_filter(&self, expr: &str) -> Option<String> {
        debug!("Evaluating bb.utils.filter: {}", expr);

        // Parse: bb.utils.filter('VAR', 'item1 item2 item3', d)
        let args = self.parse_function_args(expr, "bb.utils.filter")?;

        if args.len() < 2 {
            debug!("bb.utils.filter: Expected 2+ args, got {}", args.len());
            return None;
        }

        let var_name = &args[0];
        let items = &args[1];

        debug!("  var={}, items={}", var_name, items);

        // Look up the variable
        let var_value = self.variables.get(var_name)?;

        // Filter items that exist in var_value
        let var_items: Vec<&str> = var_value.split_whitespace().collect();
        let filtered: Vec<&str> = items
            .split_whitespace()
            .filter(|item| var_items.contains(item))
            .collect();

        let result = filtered.join(" ");
        debug!("  var_value={}, filtered={}", var_value, result);

        Some(result)
    }

    /// Parse function arguments from a function call
    /// Returns a vector of string arguments (quotes already stripped)
    fn parse_function_args(&self, expr: &str, func_name: &str) -> Option<Vec<String>> {
        // Find the function call: func_name(...)
        let start = expr.find(func_name)?;
        let after_func = &expr[start + func_name.len()..];

        // Find opening paren
        let open_paren = after_func.find('(')?;
        let after_open = &after_func[open_paren + 1..];

        // Find matching closing paren
        let close_paren = self.find_matching_paren(after_open)?;
        let args_str = &after_open[..close_paren];

        // Parse arguments (handle quoted strings with commas)
        let args = self.parse_args(args_str)?;

        Some(args)
    }

    /// Find the matching closing parenthesis
    fn find_matching_paren(&self, s: &str) -> Option<usize> {
        let mut depth = 1;
        let mut in_single_quote = false;
        let mut in_double_quote = false;

        for (i, ch) in s.chars().enumerate() {
            match ch {
                '\'' if !in_double_quote => in_single_quote = !in_single_quote,
                '"' if !in_single_quote => in_double_quote = !in_double_quote,
                '(' if !in_single_quote && !in_double_quote => depth += 1,
                ')' if !in_single_quote && !in_double_quote => {
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

    /// Parse comma-separated arguments, handling quoted strings
    fn parse_args(&self, args_str: &str) -> Option<Vec<String>> {
        let mut args = Vec::new();
        let mut current_arg = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut paren_depth = 0;

        for ch in args_str.chars() {
            match ch {
                '\'' if !in_double_quote => {
                    in_single_quote = !in_single_quote;
                    // Don't include the quote in the result
                }
                '"' if !in_single_quote => {
                    in_double_quote = !in_double_quote;
                    // Don't include the quote in the result
                }
                '(' if !in_single_quote && !in_double_quote => {
                    paren_depth += 1;
                    current_arg.push(ch);
                }
                ')' if !in_single_quote && !in_double_quote => {
                    paren_depth -= 1;
                    current_arg.push(ch);
                }
                ',' if !in_single_quote && !in_double_quote && paren_depth == 0 => {
                    // End of argument
                    args.push(current_arg.trim().to_string());
                    current_arg.clear();
                }
                _ => {
                    current_arg.push(ch);
                }
            }
        }

        // Add final argument
        if !current_arg.trim().is_empty() || !args.is_empty() {
            args.push(current_arg.trim().to_string());
        }

        Some(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_evaluator() -> SimplePythonEvaluator {
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam ipv6".to_string());
        vars.insert("PACKAGECONFIG".to_string(), "udev openssl".to_string());
        vars.insert("TUNE_FEATURES".to_string(), "arm neon vfp".to_string());
        SimplePythonEvaluator::new(vars)
    }

    #[test]
    fn test_eval_contains_found() {
        let eval = create_test_evaluator();

        // Test with ${@...} wrapper
        let result = eval.evaluate("${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'yes', 'no', d)}");
        assert_eq!(result, Some("yes".to_string()));

        // Test without wrapper
        let result = eval.evaluate("bb.utils.contains('DISTRO_FEATURES', 'pam', 'found', 'missing', d)");
        assert_eq!(result, Some("found".to_string()));
    }

    #[test]
    fn test_eval_contains_not_found() {
        let eval = create_test_evaluator();

        let result = eval.evaluate("${@bb.utils.contains('DISTRO_FEATURES', 'bluetooth', 'yes', 'no', d)}");
        assert_eq!(result, Some("no".to_string()));
    }

    #[test]
    fn test_eval_contains_empty_false_value() {
        let eval = create_test_evaluator();

        // Common pattern: empty string for false value
        let result = eval.evaluate("${@bb.utils.contains('DISTRO_FEATURES', 'bluetooth', 'bluez5', '', d)}");
        assert_eq!(result, Some("".to_string()));

        let result = eval.evaluate("${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'hwdb', '', d)}");
        assert_eq!(result, Some("hwdb".to_string()));
    }

    #[test]
    fn test_eval_filter_some_match() {
        let eval = create_test_evaluator();

        let result = eval.evaluate("${@bb.utils.filter('DISTRO_FEATURES', 'systemd ipv6 bluetooth', d)}");
        assert_eq!(result, Some("systemd ipv6".to_string()));
    }

    #[test]
    fn test_eval_filter_no_match() {
        let eval = create_test_evaluator();

        let result = eval.evaluate("${@bb.utils.filter('DISTRO_FEATURES', 'bluetooth selinux', d)}");
        assert_eq!(result, Some("".to_string()));
    }

    #[test]
    fn test_eval_filter_all_match() {
        let eval = create_test_evaluator();

        let result = eval.evaluate("${@bb.utils.filter('DISTRO_FEATURES', 'systemd pam', d)}");
        assert_eq!(result, Some("systemd pam".to_string()));
    }

    #[test]
    fn test_unknown_variable() {
        let eval = create_test_evaluator();

        // Unknown variable should return None (can't evaluate)
        let result = eval.evaluate("${@bb.utils.contains('UNKNOWN_VAR', 'value', 'yes', 'no', d)}");
        assert_eq!(result, None);
    }

    #[test]
    fn test_unknown_function() {
        let eval = create_test_evaluator();

        // Unknown function should return None
        let result = eval.evaluate("${@bb.utils.unknown_func('arg1', 'arg2', d)}");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_args() {
        let eval = create_test_evaluator();

        let args = eval.parse_args("'VAR', 'item', 'true', 'false', d").unwrap();
        assert_eq!(args, vec!["VAR", "item", "true", "false", "d"]);

        let args = eval.parse_args("'DISTRO_FEATURES', 'systemd', 'hwdb', '', d").unwrap();
        assert_eq!(args, vec!["DISTRO_FEATURES", "systemd", "hwdb", "", "d"]);
    }

    #[test]
    fn test_complex_true_value() {
        let eval = create_test_evaluator();

        // Test with complex true value (like a dependency list)
        let result = eval.evaluate(
            "${@bb.utils.contains('DISTRO_FEATURES', 'pam', 'libpam libpam-runtime', '', d)}"
        );
        assert_eq!(result, Some("libpam libpam-runtime".to_string()));
    }

    #[test]
    fn test_packageconfig_pattern() {
        let eval = create_test_evaluator();

        // Common PACKAGECONFIG pattern
        let result = eval.evaluate(
            "${@bb.utils.contains('PACKAGECONFIG', 'openssl', 'yes', 'no', d)}"
        );
        assert_eq!(result, Some("yes".to_string()));

        let result = eval.evaluate(
            "${@bb.utils.contains('PACKAGECONFIG', 'gnutls', 'yes', 'no', d)}"
        );
        assert_eq!(result, Some("no".to_string()));
    }

    #[test]
    fn test_real_world_patterns() {
        let eval = create_test_evaluator();

        // From pciutils
        let result = eval.evaluate(
            "${@bb.utils.contains('DISTRO_FEATURES', 'systemd', 'hwdb', '', d)}"
        );
        assert_eq!(result, Some("hwdb".to_string()));

        // From inetutils
        let result = eval.evaluate(
            "${@bb.utils.filter('DISTRO_FEATURES', 'pam', d)}"
        );
        assert_eq!(result, Some("pam".to_string()));

        // From at.bb
        let result = eval.evaluate(
            "${@bb.utils.contains('DISTRO_FEATURES', 'pam', 'libpam', '', d)}"
        );
        assert_eq!(result, Some("libpam".to_string()));
    }
}
