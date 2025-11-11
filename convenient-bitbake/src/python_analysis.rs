// Python code analysis for BitBake recipes
// Extracts and analyzes Python blocks for variable operations

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Type of Python block in a BitBake recipe
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PythonBlockType {
    /// Anonymous python block: python() { ... }
    Anonymous,
    /// Named function: python do_configure() { ... }
    NamedFunction,
    /// Inline expression: ${@...}
    InlineExpression,
}

/// A Python code block extracted from a BitBake recipe
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PythonBlock {
    /// Type of Python block
    pub block_type: PythonBlockType,
    /// Python source code
    pub source: String,
    /// Function name (if named)
    pub name: Option<String>,
    /// Line number where block starts
    pub line_number: Option<usize>,
}

impl PythonBlock {
    pub fn new(block_type: PythonBlockType, source: String) -> Self {
        Self {
            block_type,
            source,
            name: None,
            line_number: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_line(mut self, line: usize) -> Self {
        self.line_number = Some(line);
        self
    }
}

/// Type of Python variable operation
#[derive(Debug, Clone, PartialEq)]
pub enum PythonOpType {
    /// d.setVar('VAR', value)
    SetVar,
    /// d.appendVar('VAR', value)
    AppendVar,
    /// d.prependVar('VAR', value)
    PrependVar,
    /// d.getVar('VAR')
    GetVar,
    /// d.delVar('VAR')
    DelVar,
}

/// A variable operation found in Python code
#[derive(Debug, Clone)]
pub struct PythonVariableOp {
    /// Type of operation
    pub operation: PythonOpType,
    /// Variable name
    pub var_name: String,
    /// Value (if it's a literal string)
    pub value: Option<String>,
    /// Whether the value is a literal (true) or computed (false)
    pub is_literal: bool,
}

/// Analyzer for Python code in BitBake recipes
pub struct PythonAnalyzer {
    /// Regex for d.setVar() calls
    setvar_regex: Regex,
    /// Regex for d.getVar() calls
    getvar_regex: Regex,
    /// Regex for d.appendVar() calls
    appendvar_regex: Regex,
    /// Regex for d.prependVar() calls
    prependvar_regex: Regex,
}

impl PythonAnalyzer {
    pub fn new() -> Self {
        // Match: d.setVar('VAR', 'literal') or d.setVar("VAR", "literal")
        // Captures: var_name, value (if literal)
        let setvar_regex = Regex::new(
            r#"d\.setVar\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]*)['"]\s*\)"#
        ).unwrap();

        // Match: d.getVar('VAR') or d.getVar("VAR")
        let getvar_regex = Regex::new(
            r#"d\.getVar\s*\(\s*['"]([^'"]+)['"]\s*\)"#
        ).unwrap();

        // Match: d.appendVar('VAR', 'literal')
        let appendvar_regex = Regex::new(
            r#"d\.appendVar\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]*)['"]\s*\)"#
        ).unwrap();

        // Match: d.prependVar('VAR', 'literal')
        let prependvar_regex = Regex::new(
            r#"d\.prependVar\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]*)['"]\s*\)"#
        ).unwrap();

        Self {
            setvar_regex,
            getvar_regex,
            appendvar_regex,
            prependvar_regex,
        }
    }

    /// Analyze a Python block and extract variable operations
    pub fn analyze_block(&self, block: &PythonBlock) -> Vec<PythonVariableOp> {
        let mut ops = Vec::new();

        // Extract setVar operations with literal values
        for cap in self.setvar_regex.captures_iter(&block.source) {
            if let (Some(var_name), Some(value)) = (cap.get(1), cap.get(2)) {
                ops.push(PythonVariableOp {
                    operation: PythonOpType::SetVar,
                    var_name: var_name.as_str().to_string(),
                    value: Some(value.as_str().to_string()),
                    is_literal: true,
                });
            }
        }

        // Extract appendVar operations
        for cap in self.appendvar_regex.captures_iter(&block.source) {
            if let (Some(var_name), Some(value)) = (cap.get(1), cap.get(2)) {
                ops.push(PythonVariableOp {
                    operation: PythonOpType::AppendVar,
                    var_name: var_name.as_str().to_string(),
                    value: Some(value.as_str().to_string()),
                    is_literal: true,
                });
            }
        }

        // Extract prependVar operations
        for cap in self.prependvar_regex.captures_iter(&block.source) {
            if let (Some(var_name), Some(value)) = (cap.get(1), cap.get(2)) {
                ops.push(PythonVariableOp {
                    operation: PythonOpType::PrependVar,
                    var_name: var_name.as_str().to_string(),
                    value: Some(value.as_str().to_string()),
                    is_literal: true,
                });
            }
        }

        // Extract getVar operations (reads)
        for cap in self.getvar_regex.captures_iter(&block.source) {
            if let Some(var_name) = cap.get(1) {
                ops.push(PythonVariableOp {
                    operation: PythonOpType::GetVar,
                    var_name: var_name.as_str().to_string(),
                    value: None,
                    is_literal: false,
                });
            }
        }

        // Detect non-literal setVar calls
        // Match: d.setVar('VAR', ...) where value is not a literal string
        let non_literal_setvar = Regex::new(
            r#"d\.setVar\s*\(\s*['"]([^'"]+)['"]\s*,\s*([^'"'\)][^\)]*)\)"#
        ).unwrap();

        for cap in non_literal_setvar.captures_iter(&block.source) {
            if let Some(var_name) = cap.get(1) {
                // Only add if we haven't already added this as a literal
                let var_name_str = var_name.as_str();
                if !ops.iter().any(|op| {
                    op.var_name == var_name_str &&
                    op.operation == PythonOpType::SetVar &&
                    op.is_literal
                }) {
                    ops.push(PythonVariableOp {
                        operation: PythonOpType::SetVar,
                        var_name: var_name_str.to_string(),
                        value: None,
                        is_literal: false,
                    });
                }
            }
        }

        ops
    }

    /// Analyze all Python blocks and build a summary
    pub fn analyze_blocks(&self, blocks: &[PythonBlock]) -> PythonAnalysisSummary {
        let mut summary = PythonAnalysisSummary::new();

        for block in blocks {
            let ops = self.analyze_block(block);

            for op in ops {
                match op.operation {
                    PythonOpType::SetVar | PythonOpType::AppendVar | PythonOpType::PrependVar => {
                        summary.variables_written.insert(op.var_name.clone());
                        if op.is_literal {
                            summary.literal_assignments.insert(op.var_name.clone(), op.value.unwrap_or_default());
                        } else {
                            summary.computed_assignments.insert(op.var_name.clone());
                        }
                    }
                    PythonOpType::GetVar => {
                        summary.variables_read.insert(op.var_name.clone());
                    }
                    _ => {}
                }
            }
        }

        summary
    }
}

impl Default for PythonAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of Python code analysis
#[derive(Debug, Clone)]
pub struct PythonAnalysisSummary {
    /// Variables that are written by Python code
    pub variables_written: std::collections::HashSet<String>,
    /// Variables that are read by Python code
    pub variables_read: std::collections::HashSet<String>,
    /// Literal assignments we can extract (var -> value)
    pub literal_assignments: HashMap<String, String>,
    /// Variables assigned with computed values (can't extract statically)
    pub computed_assignments: std::collections::HashSet<String>,
}

impl PythonAnalysisSummary {
    pub fn new() -> Self {
        Self {
            variables_written: std::collections::HashSet::new(),
            variables_read: std::collections::HashSet::new(),
            literal_assignments: HashMap::new(),
            computed_assignments: std::collections::HashSet::new(),
        }
    }

    /// Check if a variable may be modified by Python code
    pub fn is_modified_by_python(&self, var_name: &str) -> bool {
        self.variables_written.contains(var_name)
    }

    /// Check if a variable is read by Python code
    pub fn is_read_by_python(&self, var_name: &str) -> bool {
        self.variables_read.contains(var_name)
    }

    /// Get the literal value if available
    pub fn get_literal_value(&self, var_name: &str) -> Option<&str> {
        self.literal_assignments.get(var_name).map(|s| s.as_str())
    }
}

impl Default for PythonAnalysisSummary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_literal_setvar() {
        let analyzer = PythonAnalyzer::new();
        let block = PythonBlock::new(
            PythonBlockType::Anonymous,
            r#"d.setVar('FOO', 'bar')"#.to_string(),
        );

        let ops = analyzer.analyze_block(&block);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].var_name, "FOO");
        assert_eq!(ops[0].value, Some("bar".to_string()));
        assert!(ops[0].is_literal);
    }

    #[test]
    fn test_extract_computed_setvar() {
        let analyzer = PythonAnalyzer::new();
        let block = PythonBlock::new(
            PythonBlockType::Anonymous,
            r#"d.setVar('FOO', d.getVar('BAR') + '-suffix')"#.to_string(),
        );

        let ops = analyzer.analyze_block(&block);
        // Should find: setVar('FOO', ...) [non-literal] and getVar('BAR')
        assert!(ops.len() >= 2);

        let setvar_op = ops.iter().find(|op| op.operation == PythonOpType::SetVar).unwrap();
        assert_eq!(setvar_op.var_name, "FOO");
        assert!(!setvar_op.is_literal);

        let getvar_op = ops.iter().find(|op| op.operation == PythonOpType::GetVar).unwrap();
        assert_eq!(getvar_op.var_name, "BAR");
    }

    #[test]
    fn test_appendvar() {
        let analyzer = PythonAnalyzer::new();
        let block = PythonBlock::new(
            PythonBlockType::Anonymous,
            r#"d.appendVar('DEPENDS', ' systemd')"#.to_string(),
        );

        let ops = analyzer.analyze_block(&block);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].operation, PythonOpType::AppendVar);
        assert_eq!(ops[0].var_name, "DEPENDS");
        assert_eq!(ops[0].value, Some(" systemd".to_string()));
    }

    #[test]
    fn test_analysis_summary() {
        let analyzer = PythonAnalyzer::new();
        let blocks = vec![
            PythonBlock::new(
                PythonBlockType::Anonymous,
                r#"
                d.setVar('FOO', 'literal')
                d.setVar('BAR', d.getVar('BAZ'))
                d.getVar('MACHINE')
                "#.to_string(),
            ),
        ];

        let summary = analyzer.analyze_blocks(&blocks);

        assert!(summary.is_modified_by_python("FOO"));
        assert!(summary.is_modified_by_python("BAR"));
        assert!(summary.is_read_by_python("BAZ"));
        assert!(summary.is_read_by_python("MACHINE"));

        assert_eq!(summary.get_literal_value("FOO"), Some("literal"));
        assert!(summary.computed_assignments.contains("BAR"));
    }
}
