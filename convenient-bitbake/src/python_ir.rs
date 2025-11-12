// Flat IR (Intermediate Representation) for Python operations in BitBake
// Similar to RecipeGraph design: flat, ID-based, efficient
// Enables static analysis and optimization before execution

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// === ID Types (cheap to copy, use as keys) ===

/// Unique identifier for a Python operation
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct OpId(pub u32);

impl OpId {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// Unique identifier for a Python value (SSA-style)
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct ValueId(pub u32);

impl ValueId {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

// === Operation Types ===

/// Type of Python operation in BitBake context
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpKind {
    /// d.getVar('VAR') -> ValueId
    GetVar {
        var_name: String,
        expand: bool,
    },

    /// d.setVar('VAR', value)
    SetVar {
        var_name: String,
        value: ValueId,
    },

    /// d.appendVar('VAR', value)
    AppendVar {
        var_name: String,
        value: ValueId,
    },

    /// d.prependVar('VAR', value)
    PrependVar {
        var_name: String,
        value: ValueId,
    },

    /// d.delVar('VAR')
    DelVar {
        var_name: String,
    },

    /// bb.utils.contains(var, item, true_val, false_val, d)
    Contains {
        var_name: String,
        item: String,
        true_val: ValueId,
        false_val: ValueId,
    },

    /// String literal: "hello"
    StringLiteral {
        value: String,
    },

    /// String concatenation: a + b
    Concat {
        left: ValueId,
        right: ValueId,
    },

    /// String method call: value.startswith(arg)
    StringMethod {
        value: ValueId,
        method: StringMethodKind,
        args: Vec<ValueId>,
    },

    /// Conditional: true_val if condition else false_val
    Conditional {
        condition: ValueId,
        true_val: ValueId,
        false_val: ValueId,
    },

    /// Comparison: left == right
    Compare {
        left: ValueId,
        right: ValueId,
        op: CompareOp,
    },

    /// Logical operation: left and right
    Logical {
        left: ValueId,
        right: ValueId,
        op: LogicalOp,
    },

    /// List literal: ['a', 'b', 'c']
    ListLiteral {
        items: Vec<ValueId>,
    },

    /// List comprehension: [expr for var in source if condition]
    ListComp {
        expr: ValueId,
        var_name: String,
        source: ValueId,
        condition: Option<ValueId>,
    },

    /// For loop: for var in source: body
    ForLoop {
        var_name: String,
        source: ValueId,
        body: Vec<OpId>,
    },

    /// If statement: if condition: then_body else: else_body
    IfStmt {
        condition: ValueId,
        then_body: Vec<OpId>,
        else_body: Vec<OpId>,
    },

    /// Complex Python (fallback to RustPython)
    ComplexPython {
        code: String,
    },
}

/// String method types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringMethodKind {
    StartsWith,
    EndsWith,
    Find,
    RFind,
    Upper,
    Lower,
    Strip,
    LStrip,
    RStrip,
    Replace,
    Split,
    Join,
}

/// Comparison operators
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Eq,   // ==
    Ne,   // !=
    Lt,   // <
    Le,   // <=
    Gt,   // >
    Ge,   // >=
    In,   // in
}

/// Logical operators
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicalOp {
    And,
    Or,
    Not,
}

// === Operation Node ===

/// A single operation in the Python IR
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: OpId,
    pub kind: OpKind,
    /// Result value ID (for operations that produce values)
    pub result: Option<ValueId>,
    /// Dependencies: operations that must execute before this one
    pub depends_on: Vec<OpId>,
}

impl Operation {
    pub fn new(id: OpId, kind: OpKind) -> Self {
        Self {
            id,
            kind,
            result: None,
            depends_on: Vec::new(),
        }
    }

    pub fn with_result(mut self, result: ValueId) -> Self {
        self.result = Some(result);
        self
    }

    pub fn with_dependency(mut self, dep: OpId) -> Self {
        self.depends_on.push(dep);
        self
    }
}

// === Python IR Graph ===

/// Flat IR for Python code in BitBake
/// Similar to RecipeGraph: flat storage, ID-based references
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PythonIR {
    /// All operations (arena-style storage)
    operations: HashMap<OpId, Operation>,

    /// Execution order (topologically sorted based on dependencies)
    execution_order: Vec<OpId>,

    /// ID generators
    next_op_id: u32,
    next_value_id: u32,

    /// Analysis results
    /// Variables read (for dependency tracking)
    pub variables_read: Vec<String>,
    /// Variables written (for dependency extraction)
    pub variables_written: HashMap<String, ValueId>,

    /// Complexity score (0-100): determines if we need RustPython
    pub complexity_score: u32,
}

impl PythonIR {
    pub fn new() -> Self {
        Self::default()
    }

    // === Operation Building ===

    /// Add a new operation
    pub fn add_operation(&mut self, kind: OpKind) -> OpId {
        let id = OpId(self.next_op_id);
        self.next_op_id += 1;

        let operation = Operation::new(id, kind);
        self.operations.insert(id, operation);
        self.execution_order.push(id);

        id
    }

    /// Allocate a new value ID (SSA-style)
    pub fn new_value(&mut self) -> ValueId {
        let id = ValueId(self.next_value_id);
        self.next_value_id += 1;
        id
    }

    /// Add string literal operation
    pub fn add_string_literal(&mut self, value: impl Into<String>) -> ValueId {
        let value_id = self.new_value();
        let op_id = self.add_operation(OpKind::StringLiteral {
            value: value.into(),
        });

        if let Some(op) = self.operations.get_mut(&op_id) {
            op.result = Some(value_id);
        }

        value_id
    }

    /// Add getVar operation
    pub fn add_getvar(&mut self, var_name: impl Into<String>, expand: bool) -> ValueId {
        let var_name = var_name.into();
        self.variables_read.push(var_name.clone());

        let value_id = self.new_value();
        let op_id = self.add_operation(OpKind::GetVar {
            var_name,
            expand,
        });

        if let Some(op) = self.operations.get_mut(&op_id) {
            op.result = Some(value_id);
        }

        value_id
    }

    /// Add setVar operation
    pub fn add_setvar(&mut self, var_name: impl Into<String>, value: ValueId) -> OpId {
        let var_name = var_name.into();
        self.variables_written.insert(var_name.clone(), value);

        self.add_operation(OpKind::SetVar { var_name, value })
    }

    /// Add appendVar operation
    pub fn add_appendvar(&mut self, var_name: impl Into<String>, value: ValueId) -> OpId {
        let var_name = var_name.into();

        // Track as write (we're modifying the variable)
        if !self.variables_written.contains_key(&var_name) {
            self.variables_written.insert(var_name.clone(), value);
        }

        self.add_operation(OpKind::AppendVar { var_name, value })
    }

    /// Add prependVar operation
    pub fn add_prependvar(&mut self, var_name: impl Into<String>, value: ValueId) -> OpId {
        let var_name = var_name.into();

        if !self.variables_written.contains_key(&var_name) {
            self.variables_written.insert(var_name.clone(), value);
        }

        self.add_operation(OpKind::PrependVar { var_name, value })
    }

    /// Add conditional operation
    pub fn add_conditional(
        &mut self,
        condition: ValueId,
        true_val: ValueId,
        false_val: ValueId,
    ) -> ValueId {
        let value_id = self.new_value();
        let op_id = self.add_operation(OpKind::Conditional {
            condition,
            true_val,
            false_val,
        });

        if let Some(op) = self.operations.get_mut(&op_id) {
            op.result = Some(value_id);
        }

        value_id
    }

    /// Add bb.utils.contains operation
    pub fn add_contains(
        &mut self,
        var_name: impl Into<String>,
        item: impl Into<String>,
        true_val: ValueId,
        false_val: ValueId,
    ) -> ValueId {
        let var_name = var_name.into();
        self.variables_read.push(var_name.clone());

        let value_id = self.new_value();
        let op_id = self.add_operation(OpKind::Contains {
            var_name,
            item: item.into(),
            true_val,
            false_val,
        });

        if let Some(op) = self.operations.get_mut(&op_id) {
            op.result = Some(value_id);
        }

        value_id
    }

    // === Analysis ===

    /// Calculate complexity score (0-100)
    /// Higher score means more likely to need RustPython
    pub fn calculate_complexity(&mut self) {
        let mut score = 0u32;

        for op in self.operations.values() {
            score += match &op.kind {
                // Simple operations (low complexity, symbolic only)
                OpKind::StringLiteral { .. } => 0,
                OpKind::SetVar { .. } => 1,

                // GetVar depends on whether expansion is needed
                OpKind::GetVar { expand: true, .. } => 4,  // Needs variable expansion
                OpKind::GetVar { expand: false, .. } => 1,

                OpKind::AppendVar { .. } => 2,
                OpKind::PrependVar { .. } => 2,

                // Medium complexity (needs evaluation)
                OpKind::Contains { .. } => 5,  // Needs containment check
                OpKind::Concat { .. } => 2,
                OpKind::StringMethod { .. } => 4,
                OpKind::Conditional { .. } => 5,
                OpKind::Compare { .. } => 3,
                OpKind::Logical { .. } => 4,
                OpKind::ListLiteral { .. } => 3,

                // Higher complexity
                OpKind::ListComp { condition: Some(_), .. } => 8,
                OpKind::ListComp { .. } => 5,
                OpKind::ForLoop { .. } => 10,
                OpKind::IfStmt { .. } => 8,

                // Very high complexity (definitely need RustPython)
                OpKind::ComplexPython { .. } => 50,
                OpKind::DelVar { .. } => 5,
            };
        }

        self.complexity_score = score.min(100);
    }

    /// Determine execution strategy based on complexity
    pub fn execution_strategy(&self) -> ExecutionStrategy {
        match self.complexity_score {
            0..=3 => ExecutionStrategy::Static,        // Pure pattern matching (only literals)
            4..=50 => ExecutionStrategy::Hybrid,       // Pattern matching + simple execution
            _ => ExecutionStrategy::RustPython,        // Full RustPython VM
        }
    }

    // === Getters ===

    pub fn get_operation(&self, id: OpId) -> Option<&Operation> {
        self.operations.get(&id)
    }

    pub fn operations(&self) -> impl Iterator<Item = &Operation> {
        self.execution_order.iter().filter_map(|id| self.operations.get(id))
    }

    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }
}

/// Execution strategy for Python code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStrategy {
    /// Pure static analysis (no execution needed)
    Static,
    /// Hybrid: pattern matching with simple evaluation
    Hybrid,
    /// Full RustPython VM execution
    RustPython,
}

// === Builder Pattern ===

/// Builder for constructing Python IR
pub struct PythonIRBuilder {
    ir: PythonIR,
}

impl PythonIRBuilder {
    pub fn new() -> Self {
        Self {
            ir: PythonIR::new(),
        }
    }

    /// Add a string literal
    pub fn string_literal(&mut self, value: impl Into<String>) -> ValueId {
        self.ir.add_string_literal(value)
    }

    /// Add d.getVar() call
    pub fn getvar(&mut self, var_name: impl Into<String>) -> ValueId {
        self.ir.add_getvar(var_name, true)
    }

    /// Add d.getVar() call with explicit expand parameter
    pub fn getvar_with_expand(&mut self, var_name: impl Into<String>, expand: bool) -> ValueId {
        self.ir.add_getvar(var_name, expand)
    }

    /// Add d.setVar() call
    pub fn setvar(&mut self, var_name: impl Into<String>, value: ValueId) -> &mut Self {
        self.ir.add_setvar(var_name, value);
        self
    }

    /// Add d.appendVar() call
    pub fn appendvar(&mut self, var_name: impl Into<String>, value: ValueId) -> &mut Self {
        self.ir.add_appendvar(var_name, value);
        self
    }

    /// Add d.prependVar() call
    pub fn prependvar(&mut self, var_name: impl Into<String>, value: ValueId) -> &mut Self {
        self.ir.add_prependvar(var_name, value);
        self
    }

    /// Add conditional expression
    pub fn conditional(
        &mut self,
        condition: ValueId,
        true_val: ValueId,
        false_val: ValueId,
    ) -> ValueId {
        self.ir.add_conditional(condition, true_val, false_val)
    }

    /// Add bb.utils.contains() call
    pub fn contains(
        &mut self,
        var_name: impl Into<String>,
        item: impl Into<String>,
        true_val: ValueId,
        false_val: ValueId,
    ) -> ValueId {
        self.ir.add_contains(var_name, item, true_val, false_val)
    }

    /// Finish building and return the IR
    pub fn build(mut self) -> PythonIR {
        self.ir.calculate_complexity();
        self.ir
    }
}

impl Default for PythonIRBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_simple() {
        let mut builder = PythonIRBuilder::new();

        let value = builder.string_literal("test");
        builder.setvar("VAR", value);

        let ir = builder.build();

        assert_eq!(ir.operation_count(), 2); // StringLiteral + SetVar
        assert_eq!(ir.variables_written.len(), 1);
        assert!(ir.variables_written.contains_key("VAR"));
    }

    #[test]
    fn test_builder_getvar_setvar() {
        let mut builder = PythonIRBuilder::new();

        let workdir = builder.getvar("WORKDIR");
        builder.setvar("BUILD_DIR", workdir);

        let ir = builder.build();

        assert_eq!(ir.variables_read.len(), 1);
        assert_eq!(ir.variables_read[0], "WORKDIR");
        assert!(ir.variables_written.contains_key("BUILD_DIR"));
    }

    #[test]
    fn test_builder_conditional() {
        let mut builder = PythonIRBuilder::new();

        let true_val = builder.string_literal("yes");
        let false_val = builder.string_literal("no");
        let result = builder.contains("DISTRO_FEATURES", "systemd", true_val, false_val);
        builder.setvar("HAS_SYSTEMD", result);

        let ir = builder.build();

        assert!(ir.variables_read.contains(&"DISTRO_FEATURES".to_string()));
        assert!(ir.variables_written.contains_key("HAS_SYSTEMD"));
    }

    #[test]
    fn test_complexity_scoring() {
        // Simple IR (should be Static)
        let mut builder = PythonIRBuilder::new();
        let val = builder.string_literal("test");
        builder.setvar("VAR", val);
        let ir = builder.build();
        assert_eq!(ir.execution_strategy(), ExecutionStrategy::Static);

        // Medium complexity (should be Hybrid)
        let mut builder = PythonIRBuilder::new();
        let true_val = builder.string_literal("yes");
        let false_val = builder.string_literal("no");
        for _ in 0..5 {
            let result = builder.contains("FEATURES", "item", true_val, false_val);
            builder.setvar("RESULT", result);
        }
        let ir = builder.build();
        assert_eq!(ir.execution_strategy(), ExecutionStrategy::Hybrid);
    }

    #[test]
    fn test_appendvar() {
        let mut builder = PythonIRBuilder::new();

        let suffix = builder.string_literal(" extra");
        builder.appendvar("DEPENDS", suffix);

        let ir = builder.build();

        assert!(ir.variables_written.contains_key("DEPENDS"));
    }

    #[test]
    fn test_prependvar() {
        let mut builder = PythonIRBuilder::new();

        let prefix = builder.string_literal("prefix ");
        builder.prependvar("CFLAGS", prefix);

        let ir = builder.build();

        assert!(ir.variables_written.contains_key("CFLAGS"));
    }

    #[test]
    fn test_value_id_generation() {
        let mut builder = PythonIRBuilder::new();

        let val1 = builder.string_literal("a");
        let val2 = builder.string_literal("b");
        let val3 = builder.string_literal("c");

        assert_ne!(val1, val2);
        assert_ne!(val2, val3);
        assert_ne!(val1, val3);
    }

    #[test]
    fn test_operation_id_generation() {
        let mut ir = PythonIR::new();

        let op1 = ir.add_operation(OpKind::StringLiteral { value: "a".to_string() });
        let op2 = ir.add_operation(OpKind::StringLiteral { value: "b".to_string() });

        assert_ne!(op1, op2);
        assert_eq!(op1.as_u32() + 1, op2.as_u32());
    }
}
