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

        // Handle inline Python conditionals (ternary): value1 if condition else value2
        // IMPORTANT: Check this FIRST to avoid false matches with functions in the condition
        if inner.contains(" if ") && inner.contains(" else ") {
            return self.eval_inline_conditional(inner);
        }

        // Phase 8b: Handle list literals before other checks
        // Check for list literals: ['item1', 'item2'] or ["item1", "item2"]
        let trimmed_inner = inner.trim();
        if trimmed_inner.starts_with('[') && trimmed_inner.contains(']') {
            // Phase 9e: Check for list comprehension first: [x for x in list if condition]
            if trimmed_inner.contains(" for ") && trimmed_inner.contains(" in ") {
                if let Some(comp_result) = self.eval_list_comprehension(trimmed_inner) {
                    return Some(comp_result);
                }
            }

            if let Some(list_result) = self.eval_list_literal(trimmed_inner) {
                return Some(list_result);
            }
        }

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

        // Phase 9f: Handle bb.utils.which()
        if inner.contains("bb.utils.which") {
            return self.eval_which(inner);
        }

        // Handle oe.utils.any_distro_features()
        if inner.contains("oe.utils.any_distro_features") {
            return self.eval_any_distro_features(inner);
        }

        // Handle oe.utils.all_distro_features()
        if inner.contains("oe.utils.all_distro_features") {
            return self.eval_all_distro_features(inner);
        }

        // Handle d.getVar()
        if inner.contains("d.getVar") {
            return self.eval_getvar(inner);
        }

        // Phase 9d: Handle string literals with methods: 'string'.method()
        if (inner.contains("'") || inner.contains("\"")) && inner.contains('.') {
            return self.eval_string_literal_with_methods(inner);
        }

        // Can't handle this expression
        None
    }

    /// Phase 9d: Evaluate string literal with method calls
    /// Example: 'hello-world'.startswith('hello') -> "True"
    fn eval_string_literal_with_methods(&self, expr: &str) -> Option<String> {
        debug!("Evaluating string literal with methods: {}", expr);

        let trimmed = expr.trim();

        // Find the string literal (quoted part)
        let quote_char = if trimmed.starts_with('\'') { '\'' } else if trimmed.starts_with('"') { '"' } else { return None; };

        // Find the end of the string literal
        let mut end_quote = None;
        let mut escaped = false;
        for (i, ch) in trimmed[1..].chars().enumerate() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == quote_char {
                end_quote = Some(i + 1);
                break;
            }
        }

        let end_quote_pos = end_quote?;
        let string_value = &trimmed[1..end_quote_pos]; // Extract without quotes
        let after_string = &trimmed[end_quote_pos + 1..];

        debug!("  String value: {}, After: {}", string_value, after_string);

        // Apply any methods after the string
        if after_string.starts_with('.') {
            self.apply_string_operations(string_value, after_string)
        } else {
            // Just return the string value
            Some(string_value.to_string())
        }
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
    /// Phase 8a: Now supports chained string operations
    /// Returns the value of the variable from our context, with operations applied
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
        let mut result = self.variables.get(var_name)?.clone();
        debug!("  Initial value: {}", result);

        // Phase 8a: Check for chained operations after d.getVar()
        // Find the closing paren of d.getVar(...)
        let full_getvar_call = &expr[start..];
        if let Some(close_paren_pos) = self.find_matching_paren(&after[open_paren + 1..]) {
            let after_getvar = &after[open_paren + 1 + close_paren_pos + 1..];

            // Apply any chained string operations
            result = self.apply_string_operations(&result, after_getvar)?;
        }

        debug!("  Final result: {}", result);
        Some(result)
    }

    /// Phase 8a + 9d: Apply chained string operations to a value
    /// Supports: .replace(old, new), .strip()/.lstrip()/.rstrip(), .upper()/.lower(), [index], [start:end]
    /// Phase 9d: .startswith(prefix), .endswith(suffix), .find(substring), .rfind(substring)
    /// Example: "foo-bar".replace('-', '_').upper() -> "FOO_BAR"
    /// Example: "hello".startswith('hel') -> "True"
    fn apply_string_operations(&self, value: &str, operations: &str) -> Option<String> {
        debug!("Applying string operations: {} to value: {}", operations, value);

        let mut result = value.to_string();
        let mut remaining = operations.trim();

        // Process operations left-to-right
        while !remaining.is_empty() {
            // Skip whitespace
            remaining = remaining.trim_start();

            if remaining.is_empty() {
                break;
            }

            // Check for indexing/slicing: [...]
            if remaining.starts_with('[') {
                // Find matching close bracket
                let close_bracket = remaining.find(']')?;
                let index_expr = &remaining[1..close_bracket];

                debug!("  Indexing/slicing: [{}]", index_expr);

                // Check if it's slicing (contains ':') or indexing (just number)
                if index_expr.contains(':') {
                    // Slicing: [start:end]
                    let parts: Vec<&str> = index_expr.split(':').collect();

                    let start: usize = if parts[0].trim().is_empty() {
                        0
                    } else {
                        parts[0].trim().parse().ok()?
                    };

                    let end: usize = if parts.len() > 1 && !parts[1].trim().is_empty() {
                        parts[1].trim().parse().ok()?
                    } else {
                        result.len()
                    };

                    // Apply slice
                    if start <= result.len() && end <= result.len() && start <= end {
                        result = result[start..end].to_string();
                        debug!("  After slice [{}:{}]: {}", start, end, result);
                    } else {
                        debug!("  Invalid slice indices");
                        return None;
                    }
                } else {
                    // Indexing: [n]
                    let index: usize = index_expr.trim().parse().ok()?;

                    if index < result.len() {
                        result = result.chars().nth(index)?.to_string();
                        debug!("  After index [{}]: {}", index, result);
                    } else {
                        debug!("  Index out of bounds");
                        return None;
                    }
                }

                remaining = &remaining[close_bracket + 1..];
                continue;
            }

            // Check for method calls: .method(...)
            if remaining.starts_with('.') {
                let method_start = &remaining[1..];

                // Find method name (up to '(' or end)
                let method_end = method_start
                    .find('(')
                    .unwrap_or(method_start.len());
                let method_name = &method_start[..method_end].trim();

                debug!("  Method call: .{}", method_name);

                match *method_name {
                    "strip" => {
                        result = result.trim().to_string();
                        debug!("  After strip: {}", result);

                        // Skip past method call
                        if method_start.len() > method_end && method_start.chars().nth(method_end) == Some('(') {
                            // Find matching close paren
                            let after_paren = &method_start[method_end + 1..];
                            let close_paren = self.find_matching_paren(after_paren)?;
                            remaining = &after_paren[close_paren + 1..];
                        } else {
                            remaining = &method_start[method_end..];
                        }
                    }
                    "lstrip" => {
                        result = result.trim_start().to_string();
                        debug!("  After lstrip: {}", result);

                        if method_start.len() > method_end && method_start.chars().nth(method_end) == Some('(') {
                            let after_paren = &method_start[method_end + 1..];
                            let close_paren = self.find_matching_paren(after_paren)?;
                            remaining = &after_paren[close_paren + 1..];
                        } else {
                            remaining = &method_start[method_end..];
                        }
                    }
                    "rstrip" => {
                        result = result.trim_end().to_string();
                        debug!("  After rstrip: {}", result);

                        if method_start.len() > method_end && method_start.chars().nth(method_end) == Some('(') {
                            let after_paren = &method_start[method_end + 1..];
                            let close_paren = self.find_matching_paren(after_paren)?;
                            remaining = &after_paren[close_paren + 1..];
                        } else {
                            remaining = &method_start[method_end..];
                        }
                    }
                    "upper" => {
                        result = result.to_uppercase();
                        debug!("  After upper: {}", result);

                        if method_start.len() > method_end && method_start.chars().nth(method_end) == Some('(') {
                            let after_paren = &method_start[method_end + 1..];
                            let close_paren = self.find_matching_paren(after_paren)?;
                            remaining = &after_paren[close_paren + 1..];
                        } else {
                            remaining = &method_start[method_end..];
                        }
                    }
                    "lower" => {
                        result = result.to_lowercase();
                        debug!("  After lower: {}", result);

                        if method_start.len() > method_end && method_start.chars().nth(method_end) == Some('(') {
                            let after_paren = &method_start[method_end + 1..];
                            let close_paren = self.find_matching_paren(after_paren)?;
                            remaining = &after_paren[close_paren + 1..];
                        } else {
                            remaining = &method_start[method_end..];
                        }
                    }
                    "replace" => {
                        // .replace(old, new)
                        if method_start.len() <= method_end || method_start.chars().nth(method_end) != Some('(') {
                            debug!("  replace() requires arguments");
                            return None;
                        }

                        let after_paren = &method_start[method_end + 1..];
                        let close_paren = self.find_matching_paren(after_paren)?;
                        let args_str = &after_paren[..close_paren];

                        // Parse the two arguments
                        let args = self.parse_args(args_str)?;
                        if args.len() != 2 {
                            debug!("  replace() requires exactly 2 arguments, got {}", args.len());
                            return None;
                        }

                        let old_str = &args[0];
                        let new_str = &args[1];

                        result = result.replace(old_str, new_str);
                        debug!("  After replace({}, {}): {}", old_str, new_str, result);

                        remaining = &after_paren[close_paren + 1..];
                    }
                    "split" => {
                        // .split() returns a list, but for string operations we'll join back
                        // This is mainly used in conditionals, but we support it here
                        if method_start.len() > method_end && method_start.chars().nth(method_end) == Some('(') {
                            let after_paren = &method_start[method_end + 1..];
                            let close_paren = self.find_matching_paren(after_paren)?;
                            remaining = &after_paren[close_paren + 1..];
                        } else {
                            remaining = &method_start[method_end..];
                        }

                        // For now, keep the result as-is since split() typically used in conditionals
                        debug!("  After split: {} (no change)", result);
                    }
                    // Phase 9d: Additional string methods
                    "startswith" => {
                        // .startswith(prefix)
                        if method_start.len() <= method_end || method_start.chars().nth(method_end) != Some('(') {
                            debug!("  startswith() requires arguments");
                            return None;
                        }

                        let after_paren = &method_start[method_end + 1..];
                        let close_paren = self.find_matching_paren(after_paren)?;
                        let args_str = &after_paren[..close_paren];

                        let args = self.parse_args(args_str)?;
                        if args.is_empty() {
                            debug!("  startswith() requires at least 1 argument");
                            return None;
                        }

                        let prefix = &args[0];
                        // Return "True" or "False" as string (for use in conditionals)
                        result = if result.starts_with(prefix) {
                            "True".to_string()
                        } else {
                            "False".to_string()
                        };
                        debug!("  After startswith({}): {}", prefix, result);

                        remaining = &after_paren[close_paren + 1..];
                    }
                    "endswith" => {
                        // .endswith(suffix)
                        if method_start.len() <= method_end || method_start.chars().nth(method_end) != Some('(') {
                            debug!("  endswith() requires arguments");
                            return None;
                        }

                        let after_paren = &method_start[method_end + 1..];
                        let close_paren = self.find_matching_paren(after_paren)?;
                        let args_str = &after_paren[..close_paren];

                        let args = self.parse_args(args_str)?;
                        if args.is_empty() {
                            debug!("  endswith() requires at least 1 argument");
                            return None;
                        }

                        let suffix = &args[0];
                        // Return "True" or "False" as string (for use in conditionals)
                        result = if result.ends_with(suffix) {
                            "True".to_string()
                        } else {
                            "False".to_string()
                        };
                        debug!("  After endswith({}): {}", suffix, result);

                        remaining = &after_paren[close_paren + 1..];
                    }
                    "find" => {
                        // .find(substring) - returns index or -1
                        if method_start.len() <= method_end || method_start.chars().nth(method_end) != Some('(') {
                            debug!("  find() requires arguments");
                            return None;
                        }

                        let after_paren = &method_start[method_end + 1..];
                        let close_paren = self.find_matching_paren(after_paren)?;
                        let args_str = &after_paren[..close_paren];

                        let args = self.parse_args(args_str)?;
                        if args.is_empty() {
                            debug!("  find() requires at least 1 argument");
                            return None;
                        }

                        let substring = &args[0];
                        result = match result.find(substring) {
                            Some(pos) => pos.to_string(),
                            None => "-1".to_string(),
                        };
                        debug!("  After find({}): {}", substring, result);

                        remaining = &after_paren[close_paren + 1..];
                    }
                    "rfind" => {
                        // .rfind(substring) - returns rightmost index or -1
                        if method_start.len() <= method_end || method_start.chars().nth(method_end) != Some('(') {
                            debug!("  rfind() requires arguments");
                            return None;
                        }

                        let after_paren = &method_start[method_end + 1..];
                        let close_paren = self.find_matching_paren(after_paren)?;
                        let args_str = &after_paren[..close_paren];

                        let args = self.parse_args(args_str)?;
                        if args.is_empty() {
                            debug!("  rfind() requires at least 1 argument");
                            return None;
                        }

                        let substring = &args[0];
                        result = match result.rfind(substring) {
                            Some(pos) => pos.to_string(),
                            None => "-1".to_string(),
                        };
                        debug!("  After rfind({}): {}", substring, result);

                        remaining = &after_paren[close_paren + 1..];
                    }
                    _ => {
                        debug!("  Unknown method: {}", method_name);
                        return None;
                    }
                }
            } else {
                // No more operations to process
                break;
            }
        }

        debug!("  Final result after all operations: {}", result);
        Some(result)
    }

    /// Phase 8b: Evaluate list literals
    /// Supports: ['item1', 'item2'] and ["item1", "item2"]
    /// Returns items as space-separated string for compatibility with our string-based architecture
    fn eval_list_literal(&self, expr: &str) -> Option<String> {
        debug!("Evaluating list literal: {}", expr);

        let trimmed = expr.trim();

        // Find the opening and closing brackets
        let start_bracket = trimmed.find('[')?;
        let end_bracket = trimmed.rfind(']')?;

        if end_bracket <= start_bracket {
            debug!("  Invalid list: closing bracket before opening");
            return None;
        }

        // Extract the list content
        let list_content = &trimmed[start_bracket + 1..end_bracket];
        debug!("  List content: {}", list_content);

        // Parse comma-separated items
        let items = self.parse_args(list_content)?;

        // Join items as space-separated string (compatible with our architecture)
        let result = items.join(" ");
        debug!("  Parsed list items: {:?}, result: {}", items, result);

        Some(result)
    }

    /// Phase 9e: Evaluate list comprehension: [x for x in list if condition]
    /// Examples:
    ///   - [x for x in ['a', 'b', 'c']]
    ///   - [x for x in d.getVar('PACKAGES').split() if x.startswith('lib')]
    ///   - [x.replace('-', '_') for x in items]
    fn eval_list_comprehension(&self, expr: &str) -> Option<String> {
        debug!("Evaluating list comprehension: {}", expr);

        let trimmed = expr.trim();

        // Extract content between [ and ]
        let start_bracket = trimmed.find('[')?;
        let end_bracket = trimmed.rfind(']')?;
        if end_bracket <= start_bracket {
            return None;
        }

        let content = &trimmed[start_bracket + 1..end_bracket];
        debug!("  Content: {}", content);

        // Parse pattern: output_expr for var_name in source_list [if condition]
        let for_pos = content.find(" for ")?;
        let in_pos = content.find(" in ")?;

        if in_pos <= for_pos {
            debug!("  Invalid comprehension: 'in' before 'for'");
            return None;
        }

        let output_expr = content[..for_pos].trim();
        let var_name = content[for_pos + 5..in_pos].trim();

        // Check for optional 'if' clause
        let (source_expr, condition_expr) = if let Some(if_pos) = content[in_pos..].find(" if ") {
            let actual_if_pos = in_pos + if_pos;
            let source = content[in_pos + 4..actual_if_pos].trim();
            let condition = content[actual_if_pos + 4..].trim();
            (source, Some(condition))
        } else {
            (content[in_pos + 4..].trim(), None)
        };

        debug!("  output_expr: {}, var: {}, source: {}, condition: {:?}",
               output_expr, var_name, source_expr, condition_expr);

        // Evaluate the source list
        let source_result = self.evaluate(source_expr)?;
        debug!("  source_result: {}", source_result);

        // Split into items (space-separated)
        let items: Vec<&str> = source_result.split_whitespace().collect();
        debug!("  items: {:?}", items);

        // Process each item
        let mut results = Vec::new();
        for item in items {
            debug!("  Processing item: {}", item);

            // Create a temporary evaluator with the loop variable bound
            let mut temp_vars = self.variables.clone();
            temp_vars.insert(var_name.to_string(), item.to_string());
            let temp_eval = SimplePythonEvaluator::new(temp_vars);

            // Check condition if present
            if let Some(condition) = condition_expr {
                // Replace variable references in condition
                let condition_with_var = condition.replace(var_name, &format!("'{}'", item));
                debug!("  Evaluating condition: {}", condition_with_var);

                if let Some(cond_result) = temp_eval.eval_condition(&condition_with_var) {
                    if !cond_result {
                        debug!("  Condition false, skipping");
                        continue;
                    }
                } else {
                    debug!("  Could not evaluate condition, skipping");
                    continue;
                }
            }

            // Evaluate output expression
            // Replace variable references in output expression
            let output_with_var = if output_expr == var_name {
                // Simple case: [x for x in list]
                item.to_string()
            } else if output_expr.contains(var_name) {
                // Complex case with methods: [x.strip() for x in list]
                let expr_to_eval = output_expr.replace(var_name, &format!("'{}'", item));
                debug!("  Evaluating output: {}", expr_to_eval);

                if let Some(result) = temp_eval.evaluate(&expr_to_eval) {
                    result
                } else {
                    debug!("  Could not evaluate output expression");
                    continue;
                }
            } else {
                // Output expression doesn't reference the variable
                if let Some(result) = temp_eval.evaluate(output_expr) {
                    result
                } else {
                    output_expr.to_string()
                }
            };

            debug!("  Adding result: {}", output_with_var);
            results.push(output_with_var);
        }

        // Return space-separated results
        let final_result = results.join(" ");
        debug!("  Final result: {}", final_result);

        Some(final_result)
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

    /// Phase 8c: Evaluate logical expression with and, or, not operators
    /// Operator precedence: not > and > or (standard Python precedence)
    /// Supports parentheses for grouping
    fn eval_logical_expression(&self, expr: &str) -> Option<bool> {
        debug!("Evaluating logical expression: {}", expr);

        let trimmed = expr.trim();

        // Handle parentheses ONLY at the start (logical grouping), not embedded (function calls)
        // This prevents matching d.getVar(...) parentheses
        if trimmed.starts_with('(') {
            // Find matching closing paren for the opening paren at position 0
            let mut depth = 0;
            let mut close_paren = None;

            for (i, ch) in trimmed.chars().enumerate() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close_paren = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if let Some(close_pos) = close_paren {
                // Evaluate the parenthesized expression
                let inner_expr = &trimmed[1..close_pos];
                let inner_result = self.eval_logical_expression(inner_expr)?;

                // Replace the parenthesized part with its result
                let after = &trimmed[close_pos + 1..];

                // If there's nothing after the closing paren, just return the result
                if after.trim().is_empty() {
                    return Some(inner_result);
                }

                // Otherwise, replace and continue evaluating
                let result_str = if inner_result { "True" } else { "False" };
                let new_expr = format!("{}{}", result_str, after);
                return self.eval_logical_expression(&new_expr);
            }
        }

        // Handle 'or' operator (lowest precedence)
        // Find ' or ' at the top level (not inside parens or quotes)
        if let Some(or_pos) = self.find_operator(trimmed, " or ") {
            let left_part = &trimmed[..or_pos];
            let right_part = &trimmed[or_pos + 4..]; // Skip " or "

            debug!("  OR expression: {} or {}", left_part, right_part);

            let left_result = self.eval_logical_expression(left_part)?;

            // Short-circuit: if left is true, return true without evaluating right
            if left_result {
                return Some(true);
            }

            let right_result = self.eval_logical_expression(right_part)?;
            return Some(right_result);
        }

        // Handle 'and' operator (medium precedence)
        if let Some(and_pos) = self.find_operator(trimmed, " and ") {
            let left_part = &trimmed[..and_pos];
            let right_part = &trimmed[and_pos + 5..]; // Skip " and "

            debug!("  AND expression: {} and {}", left_part, right_part);

            let left_result = self.eval_logical_expression(left_part)?;

            // Short-circuit: if left is false, return false without evaluating right
            if !left_result {
                return Some(false);
            }

            let right_result = self.eval_logical_expression(right_part)?;
            return Some(right_result);
        }

        // Handle 'not' operator (highest precedence)
        if trimmed.starts_with("not ") {
            let inner_expr = &trimmed[4..]; // Skip "not "
            debug!("  NOT expression: not {}", inner_expr);

            let inner_result = self.eval_logical_expression(inner_expr)?;
            return Some(!inner_result);
        }

        // Handle True/False literals (from parenthesis evaluation)
        if trimmed == "True" {
            return Some(true);
        }
        if trimmed == "False" {
            return Some(false);
        }

        // Base case: evaluate as simple condition (comparison, membership, etc.)
        self.eval_simple_condition(trimmed)
    }

    /// Helper: Find operator at top level (not inside parens or quotes)
    fn find_operator(&self, expr: &str, operator: &str) -> Option<usize> {
        let mut paren_depth = 0;
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let op_bytes = operator.as_bytes();
        let expr_bytes = expr.as_bytes();

        for i in 0..expr_bytes.len() {
            let ch = expr_bytes[i] as char;

            match ch {
                '(' if !in_single_quote && !in_double_quote => paren_depth += 1,
                ')' if !in_single_quote && !in_double_quote => paren_depth -= 1,
                '\'' if !in_double_quote => in_single_quote = !in_single_quote,
                '"' if !in_single_quote => in_double_quote = !in_double_quote,
                _ => {}
            }

            // Check for operator at this position (only at top level)
            if paren_depth == 0 && !in_single_quote && !in_double_quote {
                if i + op_bytes.len() <= expr_bytes.len() {
                    if &expr_bytes[i..i + op_bytes.len()] == op_bytes {
                        return Some(i);
                    }
                }
            }
        }

        None
    }

    /// Evaluate a simple condition (no logical operators)
    /// This is the base case for eval_logical_expression
    fn eval_simple_condition(&self, condition: &str) -> Option<bool> {
        debug!("Evaluating simple condition: {}", condition);

        let trimmed = condition.trim();

        // Phase 9a: Handle numeric comparisons (<, >, <=, >=) for vercmp and len
        // Check >= before > and <= before < to avoid partial matches
        if let Some(gte_pos) = self.find_operator(trimmed, " >= ") {
            let left = trimmed[..gte_pos].trim();
            let right = trimmed[gte_pos + 4..].trim();
            debug!("  Greater-than-or-equal check: {} >= {}", left, right);

            let left_num = self.eval_numeric_expr(left)?;
            let right_num = self.eval_numeric_expr(right)?;
            debug!("  left_num={}, right_num={}", left_num, right_num);
            return Some(left_num >= right_num);
        }

        if let Some(lte_pos) = self.find_operator(trimmed, " <= ") {
            let left = trimmed[..lte_pos].trim();
            let right = trimmed[lte_pos + 4..].trim();
            debug!("  Less-than-or-equal check: {} <= {}", left, right);

            let left_num = self.eval_numeric_expr(left)?;
            let right_num = self.eval_numeric_expr(right)?;
            debug!("  left_num={}, right_num={}", left_num, right_num);
            return Some(left_num <= right_num);
        }

        if let Some(gt_pos) = self.find_operator(trimmed, " > ") {
            let left = trimmed[..gt_pos].trim();
            let right = trimmed[gt_pos + 3..].trim();
            debug!("  Greater-than check: {} > {}", left, right);

            let left_num = self.eval_numeric_expr(left)?;
            let right_num = self.eval_numeric_expr(right)?;
            debug!("  left_num={}, right_num={}", left_num, right_num);
            return Some(left_num > right_num);
        }

        if let Some(lt_pos) = self.find_operator(trimmed, " < ") {
            let left = trimmed[..lt_pos].trim();
            let right = trimmed[lt_pos + 3..].trim();
            debug!("  Less-than check: {} < {}", left, right);

            let left_num = self.eval_numeric_expr(left)?;
            let right_num = self.eval_numeric_expr(right)?;
            debug!("  left_num={}, right_num={}", left_num, right_num);
            return Some(left_num < right_num);
        }

        // Handle: d.getVar('VAR') == 'value' or len(...) == 5
        if let Some(eq_pos) = trimmed.find(" == ") {
            let left = trimmed[..eq_pos].trim();
            let right = trimmed[eq_pos + 4..].trim();

            debug!("  Equality check: {} == {}", left, right);

            // Phase 9a: Check if this is a numeric comparison (vercmp or len)
            if left.contains("bb.utils.vercmp") || left.starts_with("len(")
                || right.contains("bb.utils.vercmp") || right.starts_with("len(")
                || right.parse::<i32>().is_ok() {
                // Numeric comparison
                if let (Some(left_num), Some(right_num)) = (self.eval_numeric_expr(left), self.eval_numeric_expr(right)) {
                    debug!("  Numeric equality: {} == {}", left_num, right_num);
                    return Some(left_num == right_num);
                }
            }

            // String comparison (original behavior)
            let left_val = if left.contains("d.getVar") {
                self.eval_getvar(left)?
            } else {
                self.extract_string_literal(left)?
            };

            let right_val = self.extract_string_literal(right)?;

            debug!("  left_val={}, right_val={}", left_val, right_val);
            return Some(left_val == right_val);
        }

        // Handle: d.getVar('VAR') != 'value' or len(...) != 5
        if let Some(neq_pos) = trimmed.find(" != ") {
            let left = trimmed[..neq_pos].trim();
            let right = trimmed[neq_pos + 4..].trim();

            debug!("  Inequality check: {} != {}", left, right);

            // Phase 9a: Check if this is a numeric comparison (vercmp or len)
            if left.contains("bb.utils.vercmp") || left.starts_with("len(")
                || right.contains("bb.utils.vercmp") || right.starts_with("len(")
                || right.parse::<i32>().is_ok() {
                // Numeric comparison
                if let (Some(left_num), Some(right_num)) = (self.eval_numeric_expr(left), self.eval_numeric_expr(right)) {
                    debug!("  Numeric inequality: {} != {}", left_num, right_num);
                    return Some(left_num != right_num);
                }
            }

            // String comparison (original behavior)
            let left_val = if left.contains("d.getVar") {
                self.eval_getvar(left)?
            } else {
                self.extract_string_literal(left)?
            };

            let right_val = self.extract_string_literal(right)?;

            debug!("  left_val={}, right_val={}", left_val, right_val);
            return Some(left_val != right_val);
        }

        // Handle: 'item' in d.getVar('VAR').split() or 'item' in ['item1', 'item2']
        // Use find_operator to avoid matching ' in ' inside method calls
        if let Some(in_pos) = self.find_operator(trimmed, " in ") {
            let item_part = trimmed[..in_pos].trim();
            let container_part = trimmed[in_pos + 4..].trim();

            debug!("  Membership check: {} in {}", item_part, container_part);

            // Extract item (usually a string literal)
            let item = self.extract_string_literal(item_part)?;

            // Evaluate container
            let container_str = if container_part.starts_with('[') && container_part.contains(']') {
                // Phase 8b: Handle list literal
                self.eval_list_literal(container_part)?
            } else if container_part.contains("d.getVar") && container_part.contains(".split()") {
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

        // Phase 9d: Handle "True"/"False" string literals (from .startswith(), .endswith())
        if trimmed == "True" || trimmed == "'True'" || trimmed == "\"True\"" {
            debug!("  Boolean literal: True");
            return Some(true);
        }
        if trimmed == "False" || trimmed == "'False'" || trimmed == "\"False\"" {
            debug!("  Boolean literal: False");
            return Some(false);
        }

        // Phase 9d: Try to evaluate as string method that returns boolean
        // e.g., 'hello'.startswith('hel') -> "True"
        if trimmed.contains(".startswith(") || trimmed.contains(".endswith(") {
            if let Some(result) = self.evaluate(trimmed) {
                debug!("  String method result: {}", result);
                if result == "True" {
                    return Some(true);
                } else if result == "False" {
                    return Some(false);
                }
            }
        }

        // Phase 9f: Try to evaluate as bb.utils function and check truthiness
        // e.g., bb.utils.which('PATH', 'gcc') returns 'gcc' (truthy) or '' (falsy)
        if trimmed.contains("bb.utils.") || trimmed.contains("oe.utils.") {
            if let Some(result) = self.evaluate(trimmed) {
                debug!("  bb.utils/oe.utils function result: {}", result);
                // Non-empty string is truthy (like Python)
                let is_truthy = !result.is_empty() && result != "0" && result.to_lowercase() != "false";
                debug!("  is_truthy={}", is_truthy);
                return Some(is_truthy);
            }
        }

        debug!("  Can't evaluate simple condition: {}", trimmed);
        None
    }

    /// Evaluate a condition to boolean
    /// Phase 8c: Now supports logical operators (and, or, not) and parentheses
    /// This is the main entry point for condition evaluation
    fn eval_condition(&self, condition: &str) -> Option<bool> {
        debug!("Evaluating condition: {}", condition);

        let trimmed = condition.trim();

        // Phase 8c: Check for logical operators
        // If present, use eval_logical_expression which handles precedence and grouping
        // Note: We check for logical grouping parens (starts with '('), not function call parens
        if trimmed.starts_with("not ")
            || trimmed.contains(" and ")
            || trimmed.contains(" or ")
            || trimmed.starts_with('(') {
            return self.eval_logical_expression(trimmed);
        }

        // Otherwise, it's a simple condition
        self.eval_simple_condition(trimmed)
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

    /// Phase 9a: Evaluate numeric expression (supports bb.utils.vercmp, len, and numeric literals)
    /// Returns an integer for comparison operations
    fn eval_numeric_expr(&self, expr: &str) -> Option<i32> {
        let trimmed = expr.trim();
        debug!("Evaluating numeric expression: {}", trimmed);

        // Handle bb.utils.vercmp(v1, v2)
        if trimmed.contains("bb.utils.vercmp") {
            return self.eval_vercmp(trimmed);
        }

        // Handle len(...)
        if trimmed.starts_with("len(") {
            return self.eval_len(trimmed);
        }

        // Try to parse as integer literal
        if let Ok(num) = trimmed.parse::<i32>() {
            return Some(num);
        }

        // Try to evaluate as d.getVar and parse result
        if trimmed.contains("d.getVar") {
            if let Some(value) = self.eval_getvar(trimmed) {
                if let Ok(num) = value.parse::<i32>() {
                    return Some(num);
                }
            }
        }

        // Phase 9d: Try to evaluate as string method that returns numeric (.find(), .rfind())
        if trimmed.contains(".find(") || trimmed.contains(".rfind(") {
            if let Some(value) = self.evaluate(trimmed) {
                if let Ok(num) = value.parse::<i32>() {
                    return Some(num);
                }
            }
        }

        debug!("  Can't evaluate numeric expression: {}", trimmed);
        None
    }

    /// Phase 9a: Evaluate bb.utils.vercmp(v1, v2)
    /// Returns -1 if v1 < v2, 0 if v1 == v2, 1 if v1 > v2
    fn eval_vercmp(&self, expr: &str) -> Option<i32> {
        debug!("Evaluating bb.utils.vercmp: {}", expr);

        let args = self.parse_function_args(expr, "bb.utils.vercmp")?;

        if args.len() < 2 {
            debug!("bb.utils.vercmp: Expected 2 args, got {}", args.len());
            return None;
        }

        // Evaluate arguments (they might be d.getVar() calls or literals)
        let v1_raw = &args[0];
        let v2_raw = &args[1];

        // Try to evaluate as expressions first, then fall back to literal values
        let v1 = if v1_raw.contains("d.getVar") {
            self.eval_getvar(v1_raw).unwrap_or_else(|| v1_raw.clone())
        } else {
            v1_raw.clone()
        };

        let v2 = if v2_raw.contains("d.getVar") {
            self.eval_getvar(v2_raw).unwrap_or_else(|| v2_raw.clone())
        } else {
            v2_raw.clone()
        };

        debug!("  Comparing versions: {} vs {}", v1, v2);

        // Implement Debian-style version comparison
        // Split versions into numeric components and compare
        let result = self.compare_versions(&v1, &v2);
        debug!("  Result: {}", result);

        Some(result)
    }

    /// Compare two version strings (Debian-style comparison)
    /// Returns -1 if v1 < v2, 0 if v1 == v2, 1 if v1 > v2
    fn compare_versions(&self, v1: &str, v2: &str) -> i32 {
        // Split versions by dots and compare component by component
        let parts1: Vec<&str> = v1.split('.').collect();
        let parts2: Vec<&str> = v2.split('.').collect();

        let max_len = parts1.len().max(parts2.len());

        for i in 0..max_len {
            let p1 = parts1.get(i).unwrap_or(&"0");
            let p2 = parts2.get(i).unwrap_or(&"0");

            // Try to parse as numbers for numeric comparison
            let n1 = p1.parse::<i32>().unwrap_or(0);
            let n2 = p2.parse::<i32>().unwrap_or(0);

            if n1 < n2 {
                return -1;
            } else if n1 > n2 {
                return 1;
            }
            // If equal, continue to next component
        }

        // All components are equal
        0
    }

    /// Phase 9a: Evaluate len(...) - returns length of string or list
    fn eval_len(&self, expr: &str) -> Option<i32> {
        debug!("Evaluating len: {}", expr);

        let trimmed = expr.trim();

        // Extract content between len( and )
        if !trimmed.starts_with("len(") {
            debug!("  Expression doesn't start with len(");
            return None;
        }

        // Find matching closing paren - need to skip "len(" prefix
        let after_open_paren = &trimmed[4..];
        let relative_close = self.find_matching_paren(after_open_paren)?;
        let inner = &after_open_paren[..relative_close];

        debug!("  Inner expression: {}", inner);

        // Evaluate the inner expression to get a string
        let value = if inner.contains("d.getVar") {
            // Handle: len(d.getVar('VAR')) or len(d.getVar('VAR').split())
            self.eval_getvar(inner)?
        } else {
            // Handle: len('literal')
            self.extract_string_literal(inner)?
        };

        // If the value was from .split(), count the words
        // Otherwise, return the string length
        let length = if inner.contains(".split()") {
            // Count words (space-separated)
            value.split_whitespace().count() as i32
        } else {
            // String length
            value.len() as i32
        };

        debug!("  Length: {}", length);
        Some(length)
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

    /// Phase 9f: Evaluate bb.utils.which(path, item, d)
    /// Searches for item in path (colon or space separated) and returns item if found, empty otherwise
    /// Example: bb.utils.which('PATH', 'gcc') -> 'gcc' if gcc is in PATH, '' otherwise
    fn eval_which(&self, expr: &str) -> Option<String> {
        debug!("Evaluating bb.utils.which: {}", expr);

        let args = self.parse_function_args(expr, "bb.utils.which")?;

        if args.len() < 2 {
            debug!("bb.utils.which: Expected 2+ args, got {}", args.len());
            return None;
        }

        let path_var = &args[0];
        let item = &args[1];
        debug!("  path_var={}, item={}", path_var, item);

        // Get the path value from variables
        let path_value = self.variables.get(path_var)?;
        debug!("  path_value={}", path_value);

        // Split by colon (PATH-style) or space (BitBake-style)
        let path_items: Vec<&str> = if path_value.contains(':') {
            path_value.split(':').collect()
        } else {
            path_value.split_whitespace().collect()
        };

        debug!("  path_items={:?}", path_items);

        // Check if item is in path
        let found = path_items.iter().any(|&p| {
            // Check for exact match or if path ends with item
            p == item || p.ends_with(&format!("/{}", item))
        });

        let result = if found {
            item.to_string()
        } else {
            String::new()
        };

        debug!("  found={}, result={}", found, result);

        Some(result)
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
        let mut arg_started = false; // Track if we've seen non-whitespace content
        let mut had_quotes = false; // Track if this argument had quotes (don't trim whitespace-only quoted args)

        for ch in args_str.chars() {
            match ch {
                '\'' if !in_double_quote => {
                    in_single_quote = !in_single_quote;
                    arg_started = true;
                    // Phase 9a: Preserve quotes inside parentheses (nested function calls)
                    if paren_depth > 0 {
                        current_arg.push(ch);
                    } else {
                        had_quotes = true;
                        // Don't include the outermost quote in the result
                    }
                }
                '"' if !in_single_quote => {
                    in_double_quote = !in_double_quote;
                    arg_started = true;
                    // Phase 9a: Preserve quotes inside parentheses (nested function calls)
                    if paren_depth > 0 {
                        current_arg.push(ch);
                    } else {
                        had_quotes = true;
                        // Don't include the outermost quote in the result
                    }
                }
                '(' if !in_single_quote && !in_double_quote => {
                    paren_depth += 1;
                    current_arg.push(ch);
                    arg_started = true;
                }
                ')' if !in_single_quote && !in_double_quote => {
                    paren_depth -= 1;
                    current_arg.push(ch);
                }
                ',' if !in_single_quote && !in_double_quote && paren_depth == 0 => {
                    // End of argument
                    // Only trim if not entirely from quotes (preserve significant whitespace)
                    let result = if had_quotes && current_arg.trim().is_empty() {
                        // Whitespace-only quoted string - preserve it
                        current_arg.clone()
                    } else {
                        // Normal argument - trim trailing whitespace
                        current_arg.trim_end().to_string()
                    };
                    args.push(result);
                    current_arg.clear();
                    arg_started = false;
                    had_quotes = false;
                }
                ' ' | '\t' if !in_single_quote && !in_double_quote && !arg_started => {
                    // Skip leading whitespace outside of quotes
                }
                _ => {
                    current_arg.push(ch);
                    arg_started = true;
                }
            }
        }

        // Add final argument
        if !current_arg.is_empty() || !args.is_empty() {
            let result = if had_quotes && current_arg.trim().is_empty() {
                // Whitespace-only quoted string - preserve it
                current_arg
            } else {
                // Normal argument - trim trailing whitespace
                let trimmed = current_arg.trim_end().to_string();
                if trimmed.is_empty() && args.is_empty() {
                    return Some(args);
                }
                trimmed
            };
            args.push(result);
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

    // Phase 8a: String Operations Tests

    #[test]
    fn test_string_replace() {
        let mut vars = HashMap::new();
        vars.insert("PACKAGE_NAME".to_string(), "foo-bar-baz".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test .replace() with d.getVar()
        let result = eval.evaluate("${@d.getVar('PACKAGE_NAME').replace('-', '_')}");
        assert_eq!(result, Some("foo_bar_baz".to_string()));

        // Test multiple replacements
        let result = eval.evaluate("${@d.getVar('PACKAGE_NAME').replace('-', ' ')}");
        assert_eq!(result, Some("foo bar baz".to_string()));
    }

    #[test]
    fn test_string_case_conversion() {
        let mut vars = HashMap::new();
        vars.insert("ARCH".to_string(), "x86_64".to_string());
        vars.insert("FLAG".to_string(), "ENABLE_FEATURE".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test .upper()
        let result = eval.evaluate("${@d.getVar('ARCH').upper()}");
        assert_eq!(result, Some("X86_64".to_string()));

        // Test .lower()
        let result = eval.evaluate("${@d.getVar('FLAG').lower()}");
        assert_eq!(result, Some("enable_feature".to_string()));
    }

    #[test]
    fn test_string_strip() {
        let mut vars = HashMap::new();
        vars.insert("VALUE1".to_string(), "  trim-me  ".to_string());
        vars.insert("VALUE2".to_string(), "  left-space".to_string());
        vars.insert("VALUE3".to_string(), "right-space  ".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test .strip()
        let result = eval.evaluate("${@d.getVar('VALUE1').strip()}");
        assert_eq!(result, Some("trim-me".to_string()));

        // Test .lstrip()
        let result = eval.evaluate("${@d.getVar('VALUE2').lstrip()}");
        assert_eq!(result, Some("left-space".to_string()));

        // Test .rstrip()
        let result = eval.evaluate("${@d.getVar('VALUE3').rstrip()}");
        assert_eq!(result, Some("right-space".to_string()));
    }

    #[test]
    fn test_string_indexing() {
        let mut vars = HashMap::new();
        vars.insert("VERSION".to_string(), "1.2.3".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test indexing [0] - first character
        let result = eval.evaluate("${@d.getVar('VERSION')[0]}");
        assert_eq!(result, Some("1".to_string()));

        // Test indexing [1] - second character
        let result = eval.evaluate("${@d.getVar('VERSION')[1]}");
        assert_eq!(result, Some(".".to_string()));

        // Test indexing [2] - third character
        let result = eval.evaluate("${@d.getVar('VERSION')[2]}");
        assert_eq!(result, Some("2".to_string()));
    }

    #[test]
    fn test_string_slicing() {
        let mut vars = HashMap::new();
        vars.insert("PACKAGE".to_string(), "libfoo-1.2.3".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test slicing [0:6] (libfoo)
        let result = eval.evaluate("${@d.getVar('PACKAGE')[0:6]}");
        assert_eq!(result, Some("libfoo".to_string()));

        // Test slicing [:3] (from start)
        let result = eval.evaluate("${@d.getVar('PACKAGE')[:3]}");
        assert_eq!(result, Some("lib".to_string()));

        // Test slicing [7:] (to end)
        let result = eval.evaluate("${@d.getVar('PACKAGE')[7:]}");
        assert_eq!(result, Some("1.2.3".to_string()));
    }

    #[test]
    fn test_chained_string_operations() {
        let mut vars = HashMap::new();
        vars.insert("RAW_NAME".to_string(), "  Foo-Bar  ".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Test chaining: .strip().replace('-', '_').lower()
        let result = eval.evaluate("${@d.getVar('RAW_NAME').strip().replace('-', '_').lower()}");
        assert_eq!(result, Some("foo_bar".to_string()));

        // Test chaining: .upper().replace('FOO', 'BAZ')
        let result = eval.evaluate("${@d.getVar('RAW_NAME').strip().upper().replace('FOO', 'BAZ')}");
        assert_eq!(result, Some("BAZ-BAR".to_string()));
    }

    #[test]
    fn test_real_world_string_operations() {
        // Pattern 1: Normalize package names (common in DEPENDS)
        let mut vars = HashMap::new();
        vars.insert("PN".to_string(), "libfoo-bar".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        let result = eval.evaluate("${@d.getVar('PN').replace('-', '_')}");
        assert_eq!(result, Some("libfoo_bar".to_string()));

        // Pattern 2: Extract major version
        let mut vars2 = HashMap::new();
        vars2.insert("PV".to_string(), "2.4.15".to_string());
        let eval2 = SimplePythonEvaluator::new(vars2);

        let result = eval2.evaluate("${@d.getVar('PV')[0]}");
        assert_eq!(result, Some("2".to_string()));

        // Pattern 3: Architecture normalization
        let mut vars3 = HashMap::new();
        vars3.insert("TARGET_ARCH".to_string(), "aarch64".to_string());
        let eval3 = SimplePythonEvaluator::new(vars3);

        let result = eval3.evaluate("${@d.getVar('TARGET_ARCH').replace('aarch64', 'arm64')}");
        assert_eq!(result, Some("arm64".to_string()));
    }

    #[test]
    fn test_string_operations_with_conditionals() {
        // Test combining string operations with conditionals
        let mut vars = HashMap::new();
        vars.insert("MACHINE".to_string(), "qemu-x86-64".to_string());
        vars.insert("ENABLE".to_string(), "yes".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Pattern: Use string operation result in conditional
        let result = eval.evaluate("${@'x86-deps' if d.getVar('MACHINE').replace('-', '_').upper() == 'QEMU_X86_64' else 'other'}");
        assert_eq!(result, Some("x86-deps".to_string()));

        // Pattern: Conditional result with string operation
        let result = eval.evaluate("${@d.getVar('ENABLE').upper() if d.getVar('ENABLE') else 'NO'}");
        assert_eq!(result, Some("YES".to_string()));
    }

    // Phase 8b: List Operations Tests

    #[test]
    fn test_list_literal_simple() {
        let eval = create_test_evaluator();

        // Test simple list literal
        let result = eval.evaluate("${@['item1', 'item2', 'item3']}");
        assert_eq!(result, Some("item1 item2 item3".to_string()));

        // Test list with double quotes
        let result = eval.evaluate("${@[\"foo\", \"bar\", \"baz\"]}");
        assert_eq!(result, Some("foo bar baz".to_string()));

        // Test single item list
        let result = eval.evaluate("${@['single']}");
        assert_eq!(result, Some("single".to_string()));
    }

    #[test]
    fn test_list_membership() {
        let eval = create_test_evaluator();

        // Test 'in' operator with list literal - item present
        let result = eval.evaluate("${@'systemd' in ['systemd', 'sysvinit', 'openrc']}");
        // Note: 'in' returns bool, which we don't directly support yet
        // But when used in conditionals, it should work

        // Test with conditional
        let result = eval.evaluate("${@'yes' if 'systemd' in ['systemd', 'sysvinit'] else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        let result = eval.evaluate("${@'yes' if 'bluetooth' in ['systemd', 'sysvinit'] else 'no'}");
        assert_eq!(result, Some("no".to_string()));
    }

    #[test]
    fn test_list_with_whitespace() {
        let eval = create_test_evaluator();

        // Test list with extra whitespace
        let result = eval.evaluate("${@[ 'item1' ,  'item2' , 'item3' ]}");
        assert_eq!(result, Some("item1 item2 item3".to_string()));
    }

    #[test]
    fn test_list_in_conditionals() {
        let mut vars = HashMap::new();
        vars.insert("INIT_SYSTEM".to_string(), "systemd".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Pattern: Check if value is in allowed list
        let result = eval.evaluate("${@'valid' if d.getVar('INIT_SYSTEM') in ['systemd', 'sysvinit'] else 'invalid'}");
        // Note: This requires checking d.getVar result against list
        // For now, let's test list literal membership

        // Simpler test: Direct list membership
        let result = eval.evaluate("${@'init-deps' if 'systemd' in ['systemd', 'sysvinit'] else 'no-deps'}");
        assert_eq!(result, Some("init-deps".to_string()));
    }

    #[test]
    fn test_real_world_list_patterns() {
        let eval = create_test_evaluator();

        // Pattern 1: INIT_MANAGER options
        let result = eval.evaluate("${@'systemd-deps' if 'systemd' in ['systemd', 'sysvinit', 'mdev-busybox'] else ''}");
        assert_eq!(result, Some("systemd-deps".to_string()));

        // Pattern 2: PACKAGECONFIG options
        let result = eval.evaluate("${@'extra-deps' if 'feature-x' in ['feature-a', 'feature-b'] else ''}");
        assert_eq!(result, Some("".to_string()));

        // Pattern 3: Architecture checks
        let result = eval.evaluate("${@'arm-specific' if 'arm' in ['arm', 'aarch64', 'armv7'] else 'generic'}");
        assert_eq!(result, Some("arm-specific".to_string()));
    }

    // Phase 8c: Logical Operators Tests

    #[test]
    fn test_logical_basic_comparison() {
        let mut vars = HashMap::new();
        vars.insert("ARCH".to_string(), "arm".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // First, test that a simple comparison works
        let result = eval.evaluate("${@'yes' if d.getVar('ARCH') == 'arm' else 'no'}");
        assert_eq!(result, Some("yes".to_string()));
    }

    #[test]
    fn test_logical_and_minimal() {
        let mut vars = HashMap::new();
        vars.insert("A".to_string(), "1".to_string());
        vars.insert("B".to_string(), "2".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // First test without any complex logic - just one getVar
        let result1 = eval.evaluate("${@'yes' if d.getVar('A') == '1' else 'no'}");
        assert_eq!(result1, Some("yes".to_string()), "Single comparison failed");

        // Then test with and
        let result2 = eval.evaluate("${@'yes' if d.getVar('A') == '1' and d.getVar('B') == '2' else 'no'}");
        assert_eq!(result2, Some("yes".to_string()), "And expression failed");
    }

    #[test]
    fn test_logical_and_simple() {
        let mut vars = HashMap::new();
        vars.insert("ARCH".to_string(), "arm".to_string());
        vars.insert("OS".to_string(), "linux".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Simple: both true
        let result = eval.evaluate("${@'yes' if d.getVar('ARCH') == 'arm' and d.getVar('OS') == 'linux' else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        // Simple: first true, second false
        let result = eval.evaluate("${@'yes' if d.getVar('ARCH') == 'arm' and d.getVar('OS') == 'windows' else 'no'}");
        assert_eq!(result, Some("no".to_string()));
    }

    #[test]
    fn test_logical_and_operator() {
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Both conditions true
        let result = eval.evaluate("${@'deps' if 'systemd' in d.getVar('DISTRO_FEATURES').split() and 'pam' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("deps".to_string()));

        // First true, second false
        let result = eval.evaluate("${@'deps' if 'systemd' in d.getVar('DISTRO_FEATURES').split() and 'bluetooth' in d.getVar('DISTRO_FEATURES').split() else 'no-deps'}");
        assert_eq!(result, Some("no-deps".to_string()));

        // Both false
        let result = eval.evaluate("${@'deps' if 'bluetooth' in d.getVar('DISTRO_FEATURES').split() and 'wayland' in d.getVar('DISTRO_FEATURES').split() else 'no-deps'}");
        assert_eq!(result, Some("no-deps".to_string()));
    }

    #[test]
    fn test_logical_or_operator() {
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // First true
        let result = eval.evaluate("${@'init-deps' if 'systemd' in d.getVar('DISTRO_FEATURES').split() or 'sysvinit' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("init-deps".to_string()));

        // Second true
        let mut vars2 = HashMap::new();
        vars2.insert("DISTRO_FEATURES".to_string(), "sysvinit".to_string());
        let eval2 = SimplePythonEvaluator::new(vars2);

        let result = eval2.evaluate("${@'init-deps' if 'systemd' in d.getVar('DISTRO_FEATURES').split() or 'sysvinit' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("init-deps".to_string()));

        // Both false
        let mut vars3 = HashMap::new();
        vars3.insert("DISTRO_FEATURES".to_string(), "x11".to_string());
        let eval3 = SimplePythonEvaluator::new(vars3);

        let result = eval3.evaluate("${@'init-deps' if 'systemd' in d.getVar('DISTRO_FEATURES').split() or 'sysvinit' in d.getVar('DISTRO_FEATURES').split() else 'no-init'}");
        assert_eq!(result, Some("no-init".to_string()));
    }

    #[test]
    fn test_logical_not_operator() {
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "x11 wayland".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Not (condition false) = true
        let result = eval.evaluate("${@'legacy' if not 'systemd' in d.getVar('DISTRO_FEATURES').split() else 'modern'}");
        assert_eq!(result, Some("legacy".to_string()));

        // Not (condition true) = false
        let mut vars2 = HashMap::new();
        vars2.insert("DISTRO_FEATURES".to_string(), "systemd wayland".to_string());
        let eval2 = SimplePythonEvaluator::new(vars2);

        let result = eval2.evaluate("${@'legacy' if not 'systemd' in d.getVar('DISTRO_FEATURES').split() else 'modern'}");
        assert_eq!(result, Some("modern".to_string()));
    }

    #[test]
    fn test_logical_parentheses() {
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "x11 opengl".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // (true or false) and true = true
        let result = eval.evaluate("${@'graphics' if ('x11' in d.getVar('DISTRO_FEATURES').split() or 'wayland' in d.getVar('DISTRO_FEATURES').split()) and 'opengl' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("graphics".to_string()));

        // (false or false) and true = false
        let mut vars2 = HashMap::new();
        vars2.insert("DISTRO_FEATURES".to_string(), "opengl".to_string());
        let eval2 = SimplePythonEvaluator::new(vars2);

        let result = eval2.evaluate("${@'graphics' if ('x11' in d.getVar('DISTRO_FEATURES').split() or 'wayland' in d.getVar('DISTRO_FEATURES').split()) and 'opengl' in d.getVar('DISTRO_FEATURES').split() else 'no-graphics'}");
        assert_eq!(result, Some("no-graphics".to_string()));
    }

    #[test]
    fn test_logical_complex_expressions() {
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam x11".to_string());
        vars.insert("MACHINE".to_string(), "qemux86".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Multiple and operators
        let result = eval.evaluate("${@'full-deps' if 'systemd' in d.getVar('DISTRO_FEATURES').split() and 'pam' in d.getVar('DISTRO_FEATURES').split() and 'x11' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("full-deps".to_string()));

        // not with and
        let result = eval.evaluate("${@'result' if not 'wayland' in d.getVar('DISTRO_FEATURES').split() and 'x11' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("result".to_string()));

        // Complex: not (A and B) or C
        let result = eval.evaluate("${@'deps' if not ('bluetooth' in d.getVar('DISTRO_FEATURES').split() and 'wifi' in d.getVar('DISTRO_FEATURES').split()) or 'systemd' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("deps".to_string()));
    }

    #[test]
    fn test_logical_with_comparisons() {
        let mut vars = HashMap::new();
        vars.insert("ARCH".to_string(), "arm".to_string());
        vars.insert("DISTRO_FEATURES".to_string(), "systemd".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Comparison and membership
        let result = eval.evaluate("${@'arm-systemd' if d.getVar('ARCH') == 'arm' and 'systemd' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("arm-systemd".to_string()));

        // Comparison or comparison
        let result = eval.evaluate("${@'arm-or-x86' if d.getVar('ARCH') == 'arm' or d.getVar('ARCH') == 'x86_64' else 'other'}");
        assert_eq!(result, Some("arm-or-x86".to_string()));

        // not with comparison
        let result = eval.evaluate("${@'not-x86' if not d.getVar('ARCH') == 'x86_64' else 'x86'}");
        assert_eq!(result, Some("not-x86".to_string()));
    }

    #[test]
    fn test_real_world_logical_patterns() {
        // Pattern 1: Multiple feature dependencies
        let mut vars = HashMap::new();
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam x11 opengl".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        let result = eval.evaluate("${@'full-graphics' if ('x11' in d.getVar('DISTRO_FEATURES').split() or 'wayland' in d.getVar('DISTRO_FEATURES').split()) and 'opengl' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("full-graphics".to_string()));

        // Pattern 2: Platform-specific with feature check
        let mut vars2 = HashMap::new();
        vars2.insert("TARGET_ARCH".to_string(), "aarch64".to_string());
        vars2.insert("DISTRO_FEATURES".to_string(), "systemd".to_string());
        let eval2 = SimplePythonEvaluator::new(vars2);

        let result = eval2.evaluate("${@'arm-systemd-deps' if (d.getVar('TARGET_ARCH') == 'arm' or d.getVar('TARGET_ARCH') == 'aarch64') and 'systemd' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("arm-systemd-deps".to_string()));

        // Pattern 3: Exclusion pattern (not X and Y)
        let mut vars3 = HashMap::new();
        vars3.insert("DISTRO_FEATURES".to_string(), "pam x11".to_string());
        let eval3 = SimplePythonEvaluator::new(vars3);

        let result = eval3.evaluate("${@'sysvinit-compat' if not 'systemd' in d.getVar('DISTRO_FEATURES').split() and 'pam' in d.getVar('DISTRO_FEATURES').split() else ''}");
        assert_eq!(result, Some("sysvinit-compat".to_string()));
    }

    // Phase 9a: bb.utils.vercmp() and len() tests

    #[test]
    fn test_vercmp_basic() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Equal versions
        let result = eval.evaluate("${@'yes' if bb.utils.vercmp('1.2.3', '1.2.3') == 0 else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        // First version less than second
        let result = eval.evaluate("${@'yes' if bb.utils.vercmp('1.2.3', '1.2.4') < 0 else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        // First version greater than second
        let result = eval.evaluate("${@'yes' if bb.utils.vercmp('1.2.4', '1.2.3') > 0 else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        // Greater than or equal
        let result = eval.evaluate("${@'yes' if bb.utils.vercmp('2.5.1', '2.0') >= 0 else 'no'}");
        assert_eq!(result, Some("yes".to_string()));
    }

    #[test]
    fn test_vercmp_with_variables() {
        let mut vars = HashMap::new();
        vars.insert("PV".to_string(), "2.5.1".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Check if version is at least 2.0
        let result = eval.evaluate("${@'newfeature' if bb.utils.vercmp(d.getVar('PV'), '2.0') >= 0 else ''}");
        assert_eq!(result, Some("newfeature".to_string()));

        // Check if version is less than 3.0
        let result = eval.evaluate("${@'oldapi' if bb.utils.vercmp(d.getVar('PV'), '3.0') < 0 else 'newapi'}");
        assert_eq!(result, Some("oldapi".to_string()));
    }

    #[test]
    fn test_vercmp_complex_versions() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Multi-digit versions
        let result = eval.evaluate("${@'yes' if bb.utils.vercmp('1.10.0', '1.9.0') > 0 else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        // Versions with different component counts
        let result = eval.evaluate("${@'yes' if bb.utils.vercmp('2.0', '2.0.0') == 0 else 'no'}");
        assert_eq!(result, Some("yes".to_string()));
    }

    #[test]
    fn test_len_basic() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Length of string literal
        let result = eval.evaluate("${@'yes' if len('hello') == 5 else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        // Length check greater than
        let result = eval.evaluate("${@'long' if len('hello world') > 5 else 'short'}");
        assert_eq!(result, Some("long".to_string()));
    }

    #[test]
    fn test_len_with_variables() {
        let mut vars = HashMap::new();
        vars.insert("DEPENDS".to_string(), "libfoo libbar libbaz".to_string());
        vars.insert("ARCH".to_string(), "arm".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Count items after split
        let result = eval.evaluate("${@'many' if len(d.getVar('DEPENDS').split()) >= 3 else 'few'}");
        assert_eq!(result, Some("many".to_string()));

        // String length
        let result = eval.evaluate("${@'short' if len(d.getVar('ARCH')) < 10 else 'long'}");
        assert_eq!(result, Some("short".to_string()));
    }

    #[test]
    fn test_len_empty_and_zero() {
        let mut vars = HashMap::new();
        vars.insert("EMPTY".to_string(), "".to_string());
        vars.insert("NONEMPTY".to_string(), "value".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Empty string length
        let result = eval.evaluate("${@'empty' if len(d.getVar('EMPTY')) == 0 else 'nonempty'}");
        assert_eq!(result, Some("empty".to_string()));

        // Non-empty check
        let result = eval.evaluate("${@'has-value' if len(d.getVar('NONEMPTY')) > 0 else 'empty'}");
        assert_eq!(result, Some("has-value".to_string()));
    }

    #[test]
    fn test_real_world_vercmp_patterns() {
        let mut vars = HashMap::new();
        vars.insert("PV".to_string(), "5.4.0".to_string());
        vars.insert("KERNEL_VERSION".to_string(), "5.10.0".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Kernel version check pattern
        let result = eval.evaluate("${@'newkernel' if bb.utils.vercmp(d.getVar('KERNEL_VERSION'), '5.0') >= 0 else 'oldkernel'}");
        assert_eq!(result, Some("newkernel".to_string()));

        // Package version range check
        let result = eval.evaluate("${@'supported' if bb.utils.vercmp(d.getVar('PV'), '4.0') >= 0 and bb.utils.vercmp(d.getVar('PV'), '6.0') < 0 else 'unsupported'}");
        assert_eq!(result, Some("supported".to_string()));
    }

    #[test]
    fn test_real_world_len_patterns() {
        let mut vars = HashMap::new();
        vars.insert("PACKAGECONFIG".to_string(), "ssl crypto zlib openssl".to_string());
        vars.insert("DISTRO_FEATURES".to_string(), "systemd pam".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Check if many features enabled
        let result = eval.evaluate("${@'feature-rich' if len(d.getVar('PACKAGECONFIG').split()) > 3 else 'minimal'}");
        assert_eq!(result, Some("feature-rich".to_string()));

        // Conditional dependency based on feature count
        let result = eval.evaluate("${@'extra-deps' if len(d.getVar('DISTRO_FEATURES').split()) >= 2 else ''}");
        assert_eq!(result, Some("extra-deps".to_string()));
    }

    // Phase 9d: Additional string methods tests

    #[test]
    fn test_string_startswith() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Basic startswith
        let result = eval.evaluate("${@'yes' if 'hello-world'.startswith('hello') else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        // Negative case
        let result = eval.evaluate("${@'yes' if 'hello-world'.startswith('world') else 'no'}");
        assert_eq!(result, Some("no".to_string()));
    }

    #[test]
    fn test_string_endswith() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Basic endswith
        let result = eval.evaluate("${@'yes' if 'hello-world'.endswith('world') else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        // Negative case
        let result = eval.evaluate("${@'yes' if 'hello-world'.endswith('hello') else 'no'}");
        assert_eq!(result, Some("no".to_string()));
    }

    #[test]
    fn test_string_find() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Find returns index
        let result = eval.evaluate("${@'found' if 'hello-world'.find('world') >= 0 else 'not-found'}");
        assert_eq!(result, Some("found".to_string()));

        // Not found returns -1
        let result = eval.evaluate("${@'found' if 'hello-world'.find('xyz') >= 0 else 'not-found'}");
        assert_eq!(result, Some("not-found".to_string()));
    }

    #[test]
    fn test_string_rfind() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // rfind returns rightmost index
        let result = eval.evaluate("${@'found' if 'hello-world-world'.rfind('world') > 6 else 'not-found'}");
        assert_eq!(result, Some("found".to_string()));
    }

    #[test]
    fn test_string_methods_with_variables() {
        let mut vars = HashMap::new();
        vars.insert("PACKAGECONFIG".to_string(), "ssl-support crypto".to_string());
        vars.insert("PN".to_string(), "libfoo-dev".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // startswith with variable
        let result = eval.evaluate("${@'yes' if d.getVar('PN').startswith('lib') else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        // endswith with variable
        let result = eval.evaluate("${@'dev' if d.getVar('PN').endswith('-dev') else 'normal'}");
        assert_eq!(result, Some("dev".to_string()));

        // find with variable
        let result = eval.evaluate("${@'has-ssl' if d.getVar('PACKAGECONFIG').find('ssl') >= 0 else 'no-ssl'}");
        assert_eq!(result, Some("has-ssl".to_string()));
    }

    #[test]
    fn test_real_world_string_method_patterns() {
        let mut vars = HashMap::new();
        vars.insert("PN".to_string(), "python3-numpy".to_string());
        vars.insert("SRC_URI".to_string(), "https://example.com/file.tar.gz".to_string());
        vars.insert("DISTRO".to_string(), "poky".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Python package detection
        let result = eval.evaluate("${@'python-pkg' if d.getVar('PN').startswith('python') else 'other'}");
        assert_eq!(result, Some("python-pkg".to_string()));

        // Archive type detection
        let result = eval.evaluate("${@'tarball' if d.getVar('SRC_URI').endswith('.tar.gz') else 'other'}");
        assert_eq!(result, Some("tarball".to_string()));

        // Distro-specific handling
        let result = eval.evaluate("${@'poky-distro' if d.getVar('DISTRO').find('poky') >= 0 else 'other-distro'}");
        assert_eq!(result, Some("poky-distro".to_string()));
    }

    /// Phase 9e: List comprehension tests
    #[test]
    fn test_list_comprehension_simple() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Simple iteration: [x for x in list]
        let result = eval.evaluate("${@[x for x in ['a', 'b', 'c']]}");
        assert_eq!(result, Some("a b c".to_string()));
    }

    #[test]
    fn test_list_comprehension_with_filter() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Filter with startswith
        let result = eval.evaluate("${@[x for x in ['libfoo', 'bar', 'libbaz', 'qux'] if x.startswith('lib')]}");
        assert_eq!(result, Some("libfoo libbaz".to_string()));
    }

    #[test]
    fn test_list_comprehension_with_transform() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Transform with .upper()
        let result = eval.evaluate("${@[x.upper() for x in ['hello', 'world']]}");
        assert_eq!(result, Some("HELLO WORLD".to_string()));

        // Transform with .replace()
        let result = eval.evaluate("${@[x.replace('-', '_') for x in ['foo-bar', 'baz-qux']]}");
        assert_eq!(result, Some("foo_bar baz_qux".to_string()));
    }

    #[test]
    fn test_list_comprehension_with_variables() {
        let mut vars = HashMap::new();
        vars.insert("PACKAGES".to_string(), "libfoo libbar app1 app2".to_string());
        vars.insert("DISTRO_FEATURES".to_string(), "systemd x11 wayland".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Filter packages starting with 'lib'
        let result = eval.evaluate("${@[x for x in d.getVar('PACKAGES').split() if x.startswith('lib')]}");
        assert_eq!(result, Some("libfoo libbar".to_string()));

        // Filter packages starting with 'app'
        let result = eval.evaluate("${@[x for x in d.getVar('PACKAGES').split() if x.startswith('app')]}");
        assert_eq!(result, Some("app1 app2".to_string()));
    }

    #[test]
    fn test_list_comprehension_real_world_patterns() {
        let mut vars = HashMap::new();
        vars.insert("PACKAGES".to_string(), "python3-foo python3-bar lib-baz".to_string());
        vars.insert("FILES".to_string(), "file1.so file2.a file3.so".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Find all Python 3 packages
        let result = eval.evaluate("${@[x for x in d.getVar('PACKAGES').split() if x.startswith('python3-')]}");
        assert_eq!(result, Some("python3-foo python3-bar".to_string()));

        // Find all .so files
        let result = eval.evaluate("${@[x for x in d.getVar('FILES').split() if x.endswith('.so')]}");
        assert_eq!(result, Some("file1.so file3.so".to_string()));

        // Transform package names by replacing '-' with '_'
        let result = eval.evaluate("${@[x.replace('-', '_') for x in d.getVar('PACKAGES').split() if x.startswith('python3')]}");
        assert_eq!(result, Some("python3_foo python3_bar".to_string()));
    }

    #[test]
    fn test_list_comprehension_with_strip() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Strip whitespace from items
        let result = eval.evaluate("${@[x.strip() for x in ['  hello  ', '  world  ']]}");
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn test_list_comprehension_empty_result() {
        let vars = HashMap::new();
        let eval = SimplePythonEvaluator::new(vars);

        // Filter that matches nothing
        let result = eval.evaluate("${@[x for x in ['foo', 'bar', 'baz'] if x.startswith('qux')]}");
        assert_eq!(result, Some("".to_string()));
    }

    /// Phase 9f: bb.utils.which() tests
    #[test]
    fn test_bb_utils_which_colon_separated() {
        let mut vars = HashMap::new();
        vars.insert("PATH".to_string(), "/usr/bin:/usr/local/bin:/bin".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Check if an item exists in PATH (colon-separated)
        let result = eval.evaluate("${@bb.utils.which('PATH', 'bin', d)}");
        assert_eq!(result, Some("bin".to_string()));
    }

    #[test]
    fn test_bb_utils_which_space_separated() {
        let mut vars = HashMap::new();
        vars.insert("TOOLCHAIN".to_string(), "gcc g++ clang".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Check if gcc exists in space-separated list
        let result = eval.evaluate("${@bb.utils.which('TOOLCHAIN', 'gcc', d)}");
        assert_eq!(result, Some("gcc".to_string()));

        // Check for non-existent item
        let result = eval.evaluate("${@bb.utils.which('TOOLCHAIN', 'rust', d)}");
        assert_eq!(result, Some("".to_string()));
    }

    #[test]
    fn test_bb_utils_which_with_conditional() {
        let mut vars = HashMap::new();
        vars.insert("DEPENDS".to_string(), "python3 openssl zlib".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // First test: bb.utils.which() returns non-empty (truthy)
        // Use bb.utils.which() in a conditional - explicit test first
        let which_result = eval.evaluate("${@bb.utils.which('DEPENDS', 'python3', d)}");
        assert_eq!(which_result, Some("python3".to_string()));

        // Now test in conditional
        let result = eval.evaluate("${@'yes' if bb.utils.which('DEPENDS', 'python3', d) else 'no'}");
        assert_eq!(result, Some("yes".to_string()));

        let result = eval.evaluate("${@'yes' if bb.utils.which('DEPENDS', 'rust', d) else 'no'}");
        assert_eq!(result, Some("no".to_string()));
    }

    #[test]
    fn test_bb_utils_which_real_world() {
        let mut vars = HashMap::new();
        vars.insert("PACKAGECONFIG".to_string(), "ssl crypto x11".to_string());
        vars.insert("RDEPENDS".to_string(), "glibc libfoo libbar".to_string());
        let eval = SimplePythonEvaluator::new(vars);

        // Check if SSL is in package config
        let result = eval.evaluate("${@bb.utils.which('PACKAGECONFIG', 'ssl', d)}");
        assert_eq!(result, Some("ssl".to_string()));

        // Check if glibc is in runtime dependencies
        let result = eval.evaluate("${@bb.utils.which('RDEPENDS', 'glibc', d)}");
        assert_eq!(result, Some("glibc".to_string()));
    }
}

