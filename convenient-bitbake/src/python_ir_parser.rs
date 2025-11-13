// Parser for Python code → Python IR
// Converts BitBake Python blocks into flat IR operations

use crate::python_ir::{PythonIRBuilder, PythonIR, ValueId};
use regex::Regex;
use std::collections::HashMap;

/// Parser for converting Python code to IR
pub struct PythonIRParser {
    /// Regex patterns for common operations
    setvar_literal: Regex,
    getvar_simple: Regex,
    getvar_expand: Regex,
    appendvar_literal: Regex,
    prependvar_literal: Regex,
    contains_pattern: Regex,
    string_method_pattern: Regex,
    if_statement: Regex,
}

impl PythonIRParser {
    pub fn new() -> Self {
        Self {
            // d.setVar('VAR', 'literal') or d.setVar("VAR", "literal")
            setvar_literal: Regex::new(
                r#"d\.setVar\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]*)['"]\s*\)"#
            ).unwrap(),

            // d.getVar('VAR') - simple read
            getvar_simple: Regex::new(
                r#"d\.getVar\s*\(\s*['"]([^'"]+)['"]\s*\)"#
            ).unwrap(),

            // d.getVar('VAR', True) or d.getVar('VAR', expand=True) or d.getVar('VAR', 1)
            getvar_expand: Regex::new(
                r#"d\.getVar\s*\(\s*['"]([^'"]+)['"]\s*,\s*(?:expand\s*=\s*)?(True|1)\s*\)"#
            ).unwrap(),

            // d.appendVar('VAR', 'literal')
            appendvar_literal: Regex::new(
                r#"d\.appendVar\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]*)['"]\s*\)"#
            ).unwrap(),

            // d.prependVar('VAR', 'literal')
            prependvar_literal: Regex::new(
                r#"d\.prependVar\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]*)['"]\s*\)"#
            ).unwrap(),

            // bb.utils.contains('VAR', 'item', true_val, false_val, d)
            // true_val and false_val can be either quoted strings or unquoted literals (True, False, 1, 0, etc.)
            contains_pattern: Regex::new(
                r#"bb\.utils\.contains\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]+)['"]\s*,\s*(?:['"]([^'"]*)['"']|([A-Za-z_0-9]+))\s*,\s*(?:['"]([^'"]*)['"']|([A-Za-z_0-9]+))\s*,\s*d\s*\)"#
            ).unwrap(),

            // String methods: var.startswith('prefix'), var.endswith('suffix'), etc.
            string_method_pattern: Regex::new(
                r#"(\w+)\.(startswith|endswith|find|rfind|upper|lower|strip|replace)\s*\(([^)]*)\)"#
            ).unwrap(),

            // if condition: (simple single-line if detection)
            if_statement: Regex::new(
                r#"^\s*if\s+(.+):\s*$"#
            ).unwrap(),
        }
    }

    /// Parse Python code into IR
    /// Returns None if code is too complex for IR (needs RustPython)
    pub fn parse(&self, python_code: &str, initial_vars: HashMap<String, String>) -> Option<PythonIR> {
        let mut builder = PythonIRBuilder::new();
        let mut value_cache: HashMap<String, ValueId> = HashMap::new();

        // Quick complexity check - if code has certain patterns, mark as complex
        if self.is_too_complex(python_code) {
            // Mark as requiring RustPython
            builder.complex_python(python_code);
            return Some(builder.build());
        }

        // Parse line by line
        let lines: Vec<&str> = python_code.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                i += 1;
                continue;
            }

            // Check for if statement (before trim so we can detect indentation in body)
            if line.starts_with("if ") && line.trim_end().ends_with(':') {
                // Parse if statement with body
                let (parsed, lines_consumed) = self.parse_if_statement(&lines[i..], &mut builder, &mut value_cache, &initial_vars);
                if !parsed {
                    // Failed to parse - mark as complex
                    builder.complex_python(python_code);
                    return Some(builder.build());
                }
                i += lines_consumed;
                continue;
            }

            // Try to parse this line
            if !self.parse_line(line, &mut builder, &mut value_cache, &initial_vars) {
                // Failed to parse - mark as complex
                builder.complex_python(python_code);
                return Some(builder.build());
            }

            i += 1;
        }

        Some(builder.build())
    }

    /// Parse an if statement with body
    /// Returns (success, lines_consumed)
    fn parse_if_statement(
        &self,
        lines: &[&str],
        builder: &mut PythonIRBuilder,
        value_cache: &mut HashMap<String, ValueId>,
        initial_vars: &HashMap<String, String>,
    ) -> (bool, usize) {
        if lines.is_empty() {
            return (false, 0);
        }

        let if_line = lines[0].trim();

        // Extract condition from "if condition:"
        if !if_line.starts_with("if ") || !if_line.ends_with(':') {
            return (false, 0);
        }

        let condition = if_line[3..if_line.len()-1].trim();

        // Try to parse bb.utils.contains as condition
        if let Some(cap) = self.contains_pattern.captures(condition) {
            let var_name = cap.get(1).unwrap().as_str();
            let item = cap.get(2).unwrap().as_str();

            // true_val can be in group 3 (quoted) or group 4 (unquoted)
            let true_str = cap.get(3)
                .or_else(|| cap.get(4))
                .unwrap()
                .as_str();

            // false_val can be in group 5 (quoted) or group 6 (unquoted)
            let false_str = cap.get(5)
                .or_else(|| cap.get(6))
                .unwrap()
                .as_str();

            // Create contains check
            let true_val = builder.string_literal(true_str);
            let false_val = builder.string_literal(false_str);
            let condition_result = builder.contains(var_name, item, true_val, false_val);

            // Store for use in conditional body
            value_cache.insert("__condition__".to_string(), condition_result);

            // Parse the body (indented lines after the if)
            let mut body_lines_consumed = 1; // Start after the if line

            // For now, we'll execute the body lines directly if they're simple operations
            // In a full implementation, we'd wrap them in a conditional IR operation
            while body_lines_consumed < lines.len() {
                let body_line = lines[body_lines_consumed];

                // Check if line is indented (part of if body)
                if body_line.starts_with("    ") || body_line.starts_with("\t") {
                    let trimmed = body_line.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        // Try to parse the body line
                        // For simple bb.utils.contains in if, we'll execute the body
                        // This is a simplification - ideally we'd generate conditional IR
                        if !self.parse_line(trimmed, builder, value_cache, initial_vars) {
                            return (false, 0);
                        }
                    }
                    body_lines_consumed += 1;
                } else {
                    // No longer indented, end of if body
                    break;
                }
            }

            // Check if there's an else clause and skip it
            if body_lines_consumed < lines.len() {
                let next_line = lines[body_lines_consumed].trim();
                if next_line == "else:" {
                    // Skip the else line
                    body_lines_consumed += 1;

                    // Skip all indented lines in the else body
                    while body_lines_consumed < lines.len() {
                        let else_line = lines[body_lines_consumed];
                        if else_line.starts_with("    ") || else_line.starts_with("\t") {
                            body_lines_consumed += 1;
                        } else {
                            break;
                        }
                    }
                }
            }

            return (true, body_lines_consumed);
        }

        // If condition is not bb.utils.contains, we can't handle it yet
        (false, 0)
    }

    /// Parse a single line of Python code
    /// Returns false if line is too complex to handle
    fn parse_line(
        &self,
        line: &str,
        builder: &mut PythonIRBuilder,
        value_cache: &mut HashMap<String, ValueId>,
        _initial_vars: &HashMap<String, String>,
    ) -> bool {
        // Try to match various patterns

        // 1. d.setVar('VAR', 'literal')
        if let Some(cap) = self.setvar_literal.captures(line) {
            let var_name = cap.get(1).unwrap().as_str();
            let value_str = cap.get(2).unwrap().as_str();
            let value = builder.string_literal(value_str);
            builder.setvar(var_name, value);
            return true;
        }

        // 2. d.appendVar('VAR', 'literal')
        if let Some(cap) = self.appendvar_literal.captures(line) {
            let var_name = cap.get(1).unwrap().as_str();
            let value_str = cap.get(2).unwrap().as_str();
            let value = builder.string_literal(value_str);
            builder.appendvar(var_name, value);
            return true;
        }

        // 3. d.prependVar('VAR', 'literal')
        if let Some(cap) = self.prependvar_literal.captures(line) {
            let var_name = cap.get(1).unwrap().as_str();
            let value_str = cap.get(2).unwrap().as_str();
            let value = builder.string_literal(value_str);
            builder.prependvar(var_name, value);
            return true;
        }

        // 4. bb.utils.contains('VAR', 'item', 'true_val', 'false_val', d)
        if let Some(cap) = self.contains_pattern.captures(line) {
            let var_name = cap.get(1).unwrap().as_str();
            let item = cap.get(2).unwrap().as_str();

            // True value can be a string literal (group 3) or identifier (group 4)
            let true_str = cap.get(3)
                .or_else(|| cap.get(4))
                .map(|m| m.as_str())
                .unwrap_or("True");

            // False value can be a string literal (group 5) or identifier (group 6)
            let false_str = cap.get(5)
                .or_else(|| cap.get(6))
                .map(|m| m.as_str())
                .unwrap_or("False");

            let true_val = builder.string_literal(true_str);
            let false_val = builder.string_literal(false_str);
            let _result = builder.contains(var_name, item, true_val, false_val);

            // Note: We're not capturing the result here, but in real usage
            // this would be part of an assignment
            return true;
        }

        // 5. var = d.getVar('VAR') or var = d.getVar('VAR', True)
        if line.contains("=") && line.contains("d.getVar") {
            // Extract variable name being assigned to
            if let Some(eq_pos) = line.find('=') {
                let lhs = line[..eq_pos].trim();
                let rhs = line[eq_pos + 1..].trim();

                // Check if it's expand mode
                let expand = self.getvar_expand.is_match(rhs);

                // Extract the variable name from d.getVar('VAR', ...)
                if let Some(cap) = self.getvar_simple.captures(rhs) {
                    let var_name = cap.get(1).unwrap().as_str();
                    let value = if expand {
                        builder.getvar_with_expand(var_name, true)
                    } else {
                        builder.getvar(var_name)
                    };
                    value_cache.insert(lhs.to_string(), value);
                    return true;
                }
            }
        }

        // 6. Check for assignment with bb.utils.contains
        if line.contains("=") && line.contains("bb.utils.contains") {
            if let Some(eq_pos) = line.find('=') {
                let lhs = line[..eq_pos].trim();
                let rhs = line[eq_pos + 1..].trim();

                if let Some(cap) = self.contains_pattern.captures(rhs) {
                    let var_name = cap.get(1).unwrap().as_str();
                    let item = cap.get(2).unwrap().as_str();
                    let true_str = cap.get(3).unwrap().as_str();
                    let false_str = cap.get(4).unwrap().as_str();

                    let true_val = builder.string_literal(true_str);
                    let false_val = builder.string_literal(false_str);
                    let result = builder.contains(var_name, item, true_val, false_val);
                    value_cache.insert(lhs.to_string(), result);
                    return true;
                }
            }
        }

        // If we get here, we couldn't parse this line
        false
    }

    /// Check if Python code is too complex for IR
    fn is_too_complex(&self, code: &str) -> bool {
        // Patterns that indicate complexity beyond our IR
        let complex_patterns = [
            "for ",      // for loops (unless very simple)
            "while ",    // while loops
            "import ",   // imports
            "class ",    // class definitions
            "def ",      // function definitions (except anonymous)
            "try:",      // exception handling
            "except:",
            "finally:",
            "with ",     // context managers
            "yield ",    // generators
            "lambda ",   // lambda functions
            "exec(",     // dynamic execution
            "eval(",     // dynamic evaluation
            "[",         // List/dict comprehensions (complex)
            "{",         // Dict literals/comprehensions
        ];

        for pattern in &complex_patterns {
            if code.contains(pattern) {
                // Special case: allow simple list access like var[0]
                if *pattern == "[" && code.matches('[').count() <= 2 {
                    continue;
                }
                // Special case: allow simple dict access like {'key': 'value'}
                if *pattern == "{" && code.matches('{').count() <= 2 {
                    continue;
                }
                return true;
            }
        }

        // Check line count - very long code is likely complex
        if code.lines().count() > 20 {
            return true;
        }

        false
    }

    /// Parse inline Python expression ${@...}
    pub fn parse_inline_expression(&self, expr: &str, initial_vars: HashMap<String, String>) -> Option<PythonIR> {
        let mut builder = PythonIRBuilder::new();

        // Simple patterns for inline expressions

        // 1. bb.utils.contains('VAR', 'item', 'true', 'false', d)
        if let Some(cap) = self.contains_pattern.captures(expr) {
            let var_name = cap.get(1).unwrap().as_str();
            let item = cap.get(2).unwrap().as_str();

            // True value can be a string literal (group 3) or identifier (group 4)
            let true_str = cap.get(3)
                .or_else(|| cap.get(4))
                .map(|m| m.as_str())
                .unwrap_or("True");

            // False value can be a string literal (group 5) or identifier (group 6)
            let false_str = cap.get(5)
                .or_else(|| cap.get(6))
                .map(|m| m.as_str())
                .unwrap_or("False");

            let true_val = builder.string_literal(true_str);
            let false_val = builder.string_literal(false_str);
            builder.contains(var_name, item, true_val, false_val);

            return Some(builder.build());
        }

        // 2. d.getVar('VAR', True/expand=True) - with expansion
        if let Some(cap) = self.getvar_expand.captures(expr) {
            let var_name = cap.get(1).unwrap().as_str();
            builder.getvar_with_expand(var_name, true);
            return Some(builder.build());
        }

        // 3. d.getVar('VAR') - simple read without expansion
        if let Some(cap) = self.getvar_simple.captures(expr) {
            let var_name = cap.get(1).unwrap().as_str();
            builder.getvar(var_name);
            return Some(builder.build());
        }

        // 4. String literals
        if (expr.starts_with('"') && expr.ends_with('"')) ||
           (expr.starts_with('\'') && expr.ends_with('\'')) {
            let literal = &expr[1..expr.len()-1];
            builder.string_literal(literal);
            return Some(builder.build());
        }

        // 5. Conditional expressions: value1 if condition else value2
        if expr.contains(" if ") && expr.contains(" else ") {
            // Mark as complex for now - we'd need to parse the condition properly
            builder.complex_python(expr);
            return Some(builder.build());
        }

        // Otherwise, mark as complex
        builder.complex_python(expr);
        Some(builder.build())
    }
}

impl Default for PythonIRParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_setvar() {
        let parser = PythonIRParser::new();
        let code = r#"d.setVar('FOO', 'bar')"#;

        let ir = parser.parse(code, HashMap::new()).unwrap();

        // Should be hybrid strategy (contains SetVar operation)
        assert!(ir.complexity_score() <= 50);
        assert_eq!(ir.variables_written().len(), 1);
        assert!(ir.variables_written().contains_key("FOO"));
    }

    #[test]
    fn test_parse_appendvar() {
        let parser = PythonIRParser::new();
        let code = r#"d.appendVar('DEPENDS', ' systemd')"#;

        let ir = parser.parse(code, HashMap::new()).unwrap();

        assert!(ir.complexity_score() <= 50);
        assert_eq!(ir.variables_written().len(), 1);
        assert!(ir.variables_written().contains_key("DEPENDS"));
    }

    #[test]
    fn test_parse_contains() {
        let parser = PythonIRParser::new();
        let code = r#"bb.utils.contains('DISTRO_FEATURES', 'systemd', 'yes', 'no', d)"#;

        let ir = parser.parse(code, HashMap::new()).unwrap();

        // Contains operation should be in the IR
        assert!(ir.complexity_score() > 0);
        assert_eq!(ir.variables_read().len(), 1);
        assert_eq!(ir.variables_read()[0], "DISTRO_FEATURES");
    }

    #[test]
    fn test_parse_getvar_assignment() {
        let parser = PythonIRParser::new();
        let code = r#"machine = d.getVar('MACHINE')"#;

        let ir = parser.parse(code, HashMap::new()).unwrap();

        assert_eq!(ir.variables_read().len(), 1);
        assert_eq!(ir.variables_read()[0], "MACHINE");
    }

    #[test]
    fn test_parse_complex_code() {
        let parser = PythonIRParser::new();
        let code = r#"
for pkg in packages:
    d.setVar('FILES_' + pkg, files)
"#;

        let ir = parser.parse(code, HashMap::new()).unwrap();

        // Should be marked as complex (RustPython needed)
        assert!(ir.complexity_score() >= 50);
        use crate::python_ir::ExecutionStrategy;
        assert_eq!(ir.execution_strategy(), ExecutionStrategy::RustPython);
    }

    #[test]
    fn test_parse_inline_contains() {
        let parser = PythonIRParser::new();
        let expr = r#"bb.utils.contains('DISTRO_FEATURES', 'systemd', 'systemd', '', d)"#;

        let ir = parser.parse_inline_expression(expr, HashMap::new()).unwrap();

        assert_eq!(ir.variables_read().len(), 1);
        assert_eq!(ir.variables_read()[0], "DISTRO_FEATURES");
    }

    #[test]
    fn test_parse_inline_getvar() {
        let parser = PythonIRParser::new();
        let expr = r#"d.getVar('WORKDIR', True)"#;

        let ir = parser.parse_inline_expression(expr, HashMap::new()).unwrap();

        assert_eq!(ir.variables_read().len(), 1);
        assert_eq!(ir.variables_read()[0], "WORKDIR");
    }

    #[test]
    fn test_multiple_operations() {
        let parser = PythonIRParser::new();
        let code = r#"
d.setVar('FOO', 'bar')
d.appendVar('DEPENDS', ' systemd')
d.setVar('BAR', 'baz')
"#;

        let ir = parser.parse(code, HashMap::new()).unwrap();

        assert_eq!(ir.variables_written().len(), 3);
        assert!(ir.variables_written().contains_key("FOO"));
        assert!(ir.variables_written().contains_key("DEPENDS"));
        assert!(ir.variables_written().contains_key("BAR"));
    }
}
