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
        if inner.contains("bb.utils.contains_any") {
            return self.eval_contains_any(inner);
        }

        if inner.contains("bb.utils.contains") {
            return self.eval_contains(inner);
        }

        // Handle bb.utils.filter
        if inner.contains("bb.utils.filter") {
            return self.eval_filter(inner);
        }

        // Handle oe.utils.conditional()
        if inner.contains("oe.utils.conditional") {
            return self.eval_conditional(inner);
        }

        // Handle bb.utils.to_boolean()
        if inner.contains("bb.utils.to_boolean") {
            return self.eval_to_boolean(inner);
        }

        // Handle oe.utils.any_distro_features()
        if inner.contains("oe.utils.any_distro_features") {
            return self.eval_any_distro_features(inner);
        }

        // Handle oe.utils.all_distro_features()
        if inner.contains("oe.utils.all_distro_features") {
            return self.eval_all_distro_features(inner);
        }

        // Handle inline Python conditionals (ternary): value1 if condition else value2
        // IMPORTANT: Check this BEFORE d.getVar() to avoid false matches
        if inner.contains(" if ") && inner.contains(" else ") {
            return self.eval_inline_conditional(inner);
        }

        // Handle d.getVar()
        if inner.contains("d.getVar") {
            return self.eval_getvar(inner);
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

    /// Evaluate bb.utils.contains_any(var, items, true_val, false_val, d)
    /// Returns true_val if ANY item from items is in var, false_val otherwise
    fn eval_contains_any(&self, expr: &str) -> Option<String> {
        debug!("Evaluating bb.utils.contains_any: {}", expr);

        // Parse: bb.utils.contains_any('VAR', 'item1 item2', 'true_value', 'false_value', d)
        let args = self.parse_function_args(expr, "bb.utils.contains_any")?;

        if args.len() < 4 {
            debug!("bb.utils.contains_any: Expected 4+ args, got {}", args.len());
            return None;
        }

        let var_name = &args[0];
        let search_items = &args[1];
        let true_value = &args[2];
        let false_value = &args[3];

        debug!(
            "  var={}, items={}, true={}, false={}",
            var_name, search_items, true_value, false_value
        );

        // Look up the variable
        let var_value = self.variables.get(var_name)?;

        // Check if ANY search item is in var_value
        let var_items: Vec<&str> = var_value.split_whitespace().collect();
        let contains_any = search_items
            .split_whitespace()
            .any(|item| var_items.contains(&item));

        let result = if contains_any {
            true_value.clone()
        } else {
            false_value.clone()
        };

        debug!("  var_value={}, contains_any={}, result={}", var_value, contains_any, result);

        Some(result)
    }

    /// Evaluate d.getVar('VAR') or d.getVar('VAR', True/False)
    /// Returns the value of the variable from our context
    fn eval_getvar(&self, expr: &str) -> Option<String> {
        debug!("Evaluating d.getVar: {}", expr);

        // Try to extract variable name from d.getVar('VAR') or d.getVar("VAR")
        // Handle both forms: d.getVar('VAR') and d.getVar('VAR', True)

        // Find d.getVar
        let start = expr.find("d.getVar")?;
        let after = &expr[start + 8..]; // Skip "d.getVar"

        // Find opening paren
        let open_paren = after.find('(')?;
        let after_open = &after[open_paren + 1..];

        // Find first quoted string (the variable name)
        let quote_start = after_open.find(|c| c == '\'' || c == '"')?;
        let quote_char = after_open.chars().nth(quote_start)?;
        let after_quote = &after_open[quote_start + 1..];

        // Find matching close quote
        let quote_end = after_quote.find(quote_char)?;
        let var_name = &after_quote[..quote_end];

        debug!("  Extracting variable: {}", var_name);

        // Look up the variable
        let result = self.variables.get(var_name)?;
        debug!("  Result: {}", result);

        Some(result.clone())
    }

    /// Evaluate oe.utils.conditional(var, value, true_val, false_val, d)
    /// Returns true_val if var == value, false_val otherwise
    fn eval_conditional(&self, expr: &str) -> Option<String> {
        debug!("Evaluating oe.utils.conditional: {}", expr);

        // Parse: oe.utils.conditional('VAR', 'value', 'true_value', 'false_value', d)
        let args = self.parse_function_args(expr, "oe.utils.conditional")?;

        if args.len() < 4 {
            debug!("oe.utils.conditional: Expected 4+ args, got {}", args.len());
            return None;
        }

        let var_name = &args[0];
        let expected_value = &args[1];
        let true_value = &args[2];
        let false_value = &args[3];

        debug!(
            "  var={}, expected={}, true={}, false={}",
            var_name, expected_value, true_value, false_value
        );

        // Look up the variable
        let var_value = self.variables.get(var_name)?;

        // Check if variable equals expected value
        let matches = var_value == expected_value;

        let result = if matches {
            true_value.clone()
        } else {
            false_value.clone()
        };

        debug!("  var_value={}, matches={}, result={}", var_value, matches, result);

        Some(result)
    }

    /// Evaluate inline Python conditional (ternary): value1 if condition else value2
    /// Examples:
    ///   - 'qemu-native' if d.getVar('TOOLCHAIN_TEST_TARGET') == 'user' else ''
    ///   - 'yes' if 'systemd' in d.getVar('DISTRO_FEATURES').split() else 'no'
    ///   - 'enabled' if d.getVar('FLAG') else 'disabled'
    fn eval_inline_conditional(&self, expr: &str) -> Option<String> {
        debug!("Evaluating inline conditional: {}", expr);

        // Parse pattern: true_value if condition else false_value
        // Find ' if ' and ' else ' keywords (with spaces to avoid false matches)
        let if_pos = expr.find(" if ")?;
        let else_pos = expr.rfind(" else ")?;

        // Ensure else comes after if
        if else_pos <= if_pos {
            debug!("  Invalid structure: else before if");
            return None;
        }

        // Extract the three parts
        let true_value_part = expr[..if_pos].trim();
        let condition_part = expr[if_pos + 4..else_pos].trim(); // Skip " if "
        let false_value_part = expr[else_pos + 6..].trim(); // Skip " else "

        debug!("  true_value: {}", true_value_part);
        debug!("  condition: {}", condition_part);
        debug!("  false_value: {}", false_value_part);

        // Evaluate the condition
        let condition_result = self.eval_condition(condition_part)?;

        debug!("  condition_result: {}", condition_result);

        // Return appropriate value based on condition
        let result = if condition_result {
            self.extract_string_literal(true_value_part)
        } else {
            self.extract_string_literal(false_value_part)
        };

        debug!("  final_result: {:?}", result);
        result
    }

    /// Evaluate a condition to boolean
    /// Handles:
    ///   - d.getVar('VAR') == 'value'
    ///   - d.getVar('VAR') != 'value'  ///   - 'item' in d.getVar('VAR').split()
    ///   - d.getVar('VAR') (truthiness check)
    fn eval_condition(&self, condition: &str) -> Option<bool> {
        debug!("Evaluating condition: {}", condition);

        let trimmed = condition.trim();

        // Handle: d.getVar('VAR') == 'value'
        if let Some(eq_pos) = trimmed.find(" == ") {
            let left = trimmed[..eq_pos].trim();
            let right = trimmed[eq_pos + 4..].trim();

            debug!("  Equality check: {} == {}", left, right);

            // Evaluate left side (usually d.getVar)
            let left_val = if left.contains("d.getVar") {
                self.eval_getvar(left)?
            } else {
                self.extract_string_literal(left)?
            };

            // Evaluate right side (usually a string literal)
            let right_val = self.extract_string_literal(right)?;

            debug!("  left_val={}, right_val={}", left_val, right_val);
            return Some(left_val == right_val);
        }

        // Handle: d.getVar('VAR') != 'value'
        if let Some(neq_pos) = trimmed.find(" != ") {
            let left = trimmed[..neq_pos].trim();
            let right = trimmed[neq_pos + 4..].trim();

            debug!("  Inequality check: {} != {}", left, right);

            let left_val = if left.contains("d.getVar") {
                self.eval_getvar(left)?
            } else {
                self.extract_string_literal(left)?
            };

            let right_val = self.extract_string_literal(right)?;

            debug!("  left_val={}, right_val={}", left_val, right_val);
            return Some(left_val != right_val);
        }

        // Handle: 'item' in d.getVar('VAR').split()
        if let Some(in_pos) = trimmed.find(" in ") {
            let item_part = trimmed[..in_pos].trim();
            let container_part = trimmed[in_pos + 4..].trim();

            debug!("  Membership check: {} in {}", item_part, container_part);

            // Extract item (usually a string literal)
            let item = self.extract_string_literal(item_part)?;

            // Evaluate container - handle d.getVar('VAR').split()
            let container_str = if container_part.contains("d.getVar") && container_part.contains(".split()") {
                // Extract variable value and split
                let var_value = self.eval_getvar(container_part)?;
                var_value
            } else if container_part.contains("d.getVar") {
                // Just variable value (treat as space-separated)
                self.eval_getvar(container_part)?
            } else {
                debug!("  Can't evaluate container: {}", container_part);
                return None;
            };

            // Check if item is in container (space-separated)
            let contains = container_str.split_whitespace().any(|s| s == item);
            debug!("  container={}, contains={}", container_str, contains);
            return Some(contains);
        }

        // Handle: d.getVar('VAR') (truthiness check - non-empty string is true)
        if trimmed.contains("d.getVar") {
            debug!("  Truthiness check");
            if let Some(value) = self.eval_getvar(trimmed) {
                // Non-empty string is truthy
                let is_truthy = !value.is_empty() && value != "0" && value.to_lowercase() != "false";
                debug!("  value={}, is_truthy={}", value, is_truthy);
                return Some(is_truthy);
            }
        }

        debug!("  Can't evaluate condition: {}", trimmed);
        None
    }

    /// Extract a string literal from quotes, or return the trimmed value if no quotes
    fn extract_string_literal(&self, s: &str) -> Option<String> {
        let trimmed = s.trim();

        // Handle quoted strings: 'value' or "value"
        if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
            || (trimmed.starts_with('"') && trimmed.ends_with('"'))
        {
            if trimmed.len() < 2 {
                return Some(String::new());
            }
            return Some(trimmed[1..trimmed.len() - 1].to_string());
        }

        // If it's a variable reference, try to evaluate it
        if trimmed.contains("d.getVar") {
            return self.eval_getvar(trimmed);
        }

        // Otherwise, return as-is (might be empty string or other literal)
        Some(trimmed.to_string())
    }

    /// Evaluate bb.utils.to_boolean(value, d)
    /// Convert string to boolean ("yes"/"true"/"1" -> true, others -> false)
    fn eval_to_boolean(&self, expr: &str) -> Option<String> {
        debug!("Evaluating bb.utils.to_boolean: {}", expr);

        let args = self.parse_function_args(expr, "bb.utils.to_boolean")?;

        if args.is_empty() {
            debug!("bb.utils.to_boolean: Expected 1+ args, got 0");
            return None;
        }

        let value = &args[0];
        debug!("  value={}", value);

        // Try to get variable value if it's a variable reference
        let actual_value = if value.is_empty() {
            value.clone()
        } else {
            self.variables.get(value).cloned().unwrap_or_else(|| value.clone())
        };

        // Check if value is truthy
        let is_true = matches!(actual_value.to_lowercase().as_str(),
            "yes" | "true" | "1" | "y" | "t" | "on" | "enable" | "enabled");

        let result = if is_true { "true" } else { "false" };
        debug!("  actual_value={}, result={}", actual_value, result);

        Some(result.to_string())
    }

    /// Evaluate oe.utils.any_distro_features(d, features, *args)
    /// Returns True if ANY of the features are in DISTRO_FEATURES
    fn eval_any_distro_features(&self, expr: &str) -> Option<String> {
        debug!("Evaluating oe.utils.any_distro_features: {}", expr);

        // Parse: oe.utils.any_distro_features(d, 'feature1 feature2', true_val, false_val)
        let args = self.parse_function_args(expr, "oe.utils.any_distro_features")?;

        if args.len() < 2 {
            debug!("oe.utils.any_distro_features: Expected 2+ args, got {}", args.len());
            return None;
        }

        // First arg is 'd', second is features string
        let features = &args[1];
        let true_value = if args.len() > 2 { &args[2] } else { "1" };
        let false_value = if args.len() > 3 { &args[3] } else { "" };

        debug!("  features={}, true={}, false={}", features, true_value, false_value);

        // Look up DISTRO_FEATURES
        let distro_features = self.variables.get("DISTRO_FEATURES")?;

        // Check if ANY feature is in DISTRO_FEATURES
        let distro_items: Vec<&str> = distro_features.split_whitespace().collect();
        let has_any = features
            .split_whitespace()
            .any(|feature| distro_items.contains(&feature));

        let result = if has_any {
            true_value.to_string()
        } else {
            false_value.to_string()
        };

        debug!("  distro_features={}, has_any={}, result={}", distro_features, has_any, result);

        Some(result)
    }

    /// Evaluate oe.utils.all_distro_features(d, features, *args)
    /// Returns True if ALL of the features are in DISTRO_FEATURES
    fn eval_all_distro_features(&self, expr: &str) -> Option<String> {
        debug!("Evaluating oe.utils.all_distro_features: {}", expr);

        // Parse: oe.utils.all_distro_features(d, 'feature1 feature2', true_val, false_val)
        let args = self.parse_function_args(expr, "oe.utils.all_distro_features")?;

        if args.len() < 2 {
            debug!("oe.utils.all_distro_features: Expected 2+ args, got {}", args.len());
            return None;
        }

        // First arg is 'd', second is features string
        let features = &args[1];
        let true_value = if args.len() > 2 { &args[2] } else { "1" };
        let false_value = if args.len() > 3 { &args[3] } else { "" };

        debug!("  features={}, true={}, false={}", features, true_value, false_value);

        // Look up DISTRO_FEATURES
        let distro_features = self.variables.get("DISTRO_FEATURES")?;

        // Check if ALL features are in DISTRO_FEATURES
        let distro_items: Vec<&str> = distro_features.split_whitespace().collect();
        let has_all = features
            .split_whitespace()
            .all(|feature| distro_items.contains(&feature));

        let result = if has_all {
            true_value.to_string()
        } else {
            false_value.to_string()
        };

        debug!("  distro_features={}, has_all={}, result={}", distro_features, has_all, result);

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

    #[test]
    fn test_contains_any_match() {
        let eval = create_test_evaluator();

        // At least one item matches
        let result = eval.evaluate(
            "${@bb.utils.contains_any('DISTRO_FEATURES', 'systemd bluetooth', 'yes', 'no', d)}"
        );
        assert_eq!(result, Some("yes".to_string()));
    }

    #[test]
    fn test_contains_any_no_match() {
        let eval = create_test_evaluator();

        // No items match
        let result = eval.evaluate(
            "${@bb.utils.contains_any('DISTRO_FEATURES', 'bluetooth selinux', 'yes', 'no', d)}"
        );
        assert_eq!(result, Some("no".to_string()));
    }

    #[test]
    fn test_getvar() {
        let eval = create_test_evaluator();

        // Test d.getVar('VAR') in conditional
        let result = eval.evaluate("${@'qemu-native' if d.getVar('TOOLCHAIN_TEST_TARGET') == 'user' else ''}");
        // TOOLCHAIN_TEST_TARGET not in our vars, so should return None (can't evaluate)
        assert_eq!(result, None);

        // Add the variable with matching value
        let mut vars = HashMap::new();
        vars.insert("TOOLCHAIN_TEST_TARGET".to_string(), "user".to_string());
        let eval2 = SimplePythonEvaluator::new(vars);

        // Now it should evaluate to 'qemu-native'
        let result = eval2.evaluate("${@'qemu-native' if d.getVar('TOOLCHAIN_TEST_TARGET') == 'user' else ''}");
        assert_eq!(result, Some("qemu-native".to_string()));

        // Test with non-matching value
        let mut vars3 = HashMap::new();
        vars3.insert("TOOLCHAIN_TEST_TARGET".to_string(), "qemu".to_string());
        let eval3 = SimplePythonEvaluator::new(vars3);

        let result = eval3.evaluate("${@'qemu-native' if d.getVar('TOOLCHAIN_TEST_TARGET') == 'user' else ''}");
        assert_eq!(result, Some("".to_string()));
    }

    #[test]
    fn test_conditional_match() {
        let eval = create_test_evaluator();

        // Add a test variable
        let mut vars = HashMap::new();
        vars.insert("MACHINE".to_string(), "qemux86".to_string());
        let eval2 = SimplePythonEvaluator::new(vars);

        let result = eval2.evaluate(
            "${@oe.utils.conditional('MACHINE', 'qemux86', 'yes', 'no', d)}"
        );
        assert_eq!(result, Some("yes".to_string()));
    }

    #[test]
    fn test_conditional_no_match() {
        let eval = create_test_evaluator();

        // Add a test variable
        let mut vars = HashMap::new();
        vars.insert("MACHINE".to_string(), "qemux86".to_string());
        let eval2 = SimplePythonEvaluator::new(vars);

        let result = eval2.evaluate(
            "${@oe.utils.conditional('MACHINE', 'qemuarm', 'yes', 'no', d)}"
        );
        assert_eq!(result, Some("no".to_string()));
    }

    #[test]
    fn test_inline_conditional_equality() {
        let mut vars = HashMap::new();
        vars.insert("TARGET_ARCH".to_string(), "arm".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test == operator
        let result = eval.evaluate("${@'arm-dep' if d.getVar('TARGET_ARCH') == 'arm' else 'x86-dep'}");
        assert_eq!(result, Some("arm-dep".to_string()));

        let result = eval.evaluate("${@'arm-dep' if d.getVar('TARGET_ARCH') == 'x86' else 'x86-dep'}");
        assert_eq!(result, Some("x86-dep".to_string()));
    }

    #[test]
    fn test_inline_conditional_inequality() {
        let mut vars = HashMap::new();
        vars.insert("MACHINE".to_string(), "qemux86".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test != operator
        let result = eval.evaluate("${@'special' if d.getVar('MACHINE') != 'host' else 'standard'}");
        assert_eq!(result, Some("special".to_string()));
    }

    #[test]
    fn test_inline_conditional_membership() {
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam ipv6".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test 'in' operator with .split()
        let result = eval.evaluate("${@'systemd-dep' if 'systemd' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("systemd-dep".to_string()));

        let result = eval.evaluate("${@'bluetooth-dep' if 'bluetooth' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("".to_string()));
    }

    #[test]
    fn test_inline_conditional_truthiness() {
        let mut vars = HashMap::new();
        vars.insert("ENABLE_FEATURE".to_string(), "1".to_string());
        vars.insert("DISABLE_FEATURE".to_string(), "".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test truthiness (non-empty is true)
        let result = eval.evaluate("${@'enabled' if d.getVar('ENABLE_FEATURE') else 'disabled'}");
        assert_eq!(result, Some("enabled".to_string()));

        let result = eval.evaluate("${@'enabled' if d.getVar('DISABLE_FEATURE') else 'disabled'}");
        assert_eq!(result, Some("disabled".to_string()));
    }

    #[test]
    fn test_inline_conditional_real_world_patterns() {
        // Pattern 1: From crosssdk recipe
        let mut vars = HashMap::new();
        vars.insert("TOOLCHAIN_TEST_TARGET".to_string(), "user".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        let result = eval.evaluate("${@'qemu-native' if d.getVar('TOOLCHAIN_TEST_TARGET') == 'user' else ''}");
        assert_eq!(result, Some("qemu-native".to_string()));

        // Pattern 2: From systemd-dependent recipes
        let mut vars2 = HashMap::new();
        vars2.insert("DISTRO_FEATURES".to_string(), "systemd wayland".to_string());
        let eval2 = SimplePythonEvaluator::new(vars2);

        let result = eval2.evaluate("${@'libsystemd' if 'systemd' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("libsystemd".to_string()));

        // Pattern 3: Architecture-specific dependencies
        let mut vars3 = HashMap::new();
        vars3.insert("TARGET_ARCH".to_string(), "aarch64".to_string());
        let eval3 = SimplePythonEvaluator::new(vars3);

        let result = eval3.evaluate("${@'arm-toolchain' if d.getVar('TARGET_ARCH') != 'x86_64' else 'x86-toolchain'}");
        assert_eq!(result, Some("arm-toolchain".to_string()));
    }
}

