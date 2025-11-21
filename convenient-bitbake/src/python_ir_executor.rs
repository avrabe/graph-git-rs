// Executor for Python IR - runs flat operations efficiently
// Much faster than RustPython for simple operations

use crate::python_ir::{OpKind, Operation, PythonIR, ValueId, ExecutionStrategy};
use std::collections::HashMap;

use crate::python_executor::PythonExecutor;

/// Result of IR execution
#[derive(Debug, Clone)]
pub struct IRExecutionResult {
    /// Variables that were modified
    pub variables_set: HashMap<String, String>,
    /// Variables that were read
    pub variables_read: Vec<String>,
    /// Success flag
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Execution strategy used
    pub strategy_used: ExecutionStrategy,
}

impl IRExecutionResult {
    pub fn success(
        variables_set: HashMap<String, String>,
        variables_read: Vec<String>,
        strategy: ExecutionStrategy,
    ) -> Self {
        Self {
            variables_set,
            variables_read,
            success: true,
            error: None,
            strategy_used: strategy,
        }
    }

    pub fn failure(error: String, strategy: ExecutionStrategy) -> Self {
        Self {
            variables_set: HashMap::new(),
            variables_read: Vec::new(),
            success: false,
            error: Some(error),
            strategy_used: strategy,
        }
    }
}

/// Executor for Python IR
pub struct IRExecutor {
    /// Initial variable context
    initial_vars: HashMap<String, String>,
    /// Current variable state during execution
    current_vars: HashMap<String, String>,
    /// Value store (SSA-style values)
    values: HashMap<ValueId, String>,
    /// Variables read during execution
    variables_read: Vec<String>,
}

impl IRExecutor {
    pub fn new(initial_vars: HashMap<String, String>) -> Self {
        Self {
            current_vars: initial_vars.clone(),
            initial_vars,
            values: HashMap::new(),
            variables_read: Vec::new(),
        }
    }

    /// Expand ${VAR} references in a string
    fn expand_var_references(&self, s: &str) -> String {
        let mut result = s.to_string();

        // Simple expansion of ${VAR} patterns
        let var_pattern = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();

        loop {
            let mut changed = false;
            result = var_pattern.replace_all(&result, |caps: &regex::Captures| {
                let var_name = &caps[1];
                if let Some(value) = self.current_vars.get(var_name) {
                    changed = true;
                    value.clone()
                } else {
                    // Keep unexpanded if variable not found
                    caps[0].to_string()
                }
            }).to_string();

            if !changed {
                break;
            }
        }

        result
    }

    /// Execute Python IR
    pub fn execute(&mut self, ir: &PythonIR) -> IRExecutionResult {
        // Determine execution strategy
        let strategy = ir.execution_strategy();

        match strategy {
            ExecutionStrategy::Static => self.execute_static(ir),
            ExecutionStrategy::Hybrid => self.execute_hybrid(ir),
            ExecutionStrategy::RustPython => self.execute_rustpython(ir),
        }
    }

    /// Execute with pure static analysis (no evaluation needed)
    fn execute_static(&mut self, ir: &PythonIR) -> IRExecutionResult {
        // For static execution, we just extract the operations
        // Variables are tracked, but values are kept symbolic

        for operation in ir.operations() {
            if let Err(e) = self.execute_operation_static(operation) {
                return IRExecutionResult::failure(e, ExecutionStrategy::Static);
            }
        }

        IRExecutionResult::success(
            self.current_vars.clone(),
            self.variables_read.clone(),
            ExecutionStrategy::Static,
        )
    }

    /// Execute with hybrid approach (simple evaluation)
    fn execute_hybrid(&mut self, ir: &PythonIR) -> IRExecutionResult {
        for operation in ir.operations() {
            if let Err(e) = self.execute_operation_hybrid(operation) {
                return IRExecutionResult::failure(e, ExecutionStrategy::Hybrid);
            }
        }

        IRExecutionResult::success(
            self.current_vars.clone(),
            self.variables_read.clone(),
            ExecutionStrategy::Hybrid,
        )
    }

    /// Execute using RustPython fallback
    fn execute_rustpython(&self, ir: &PythonIR) -> IRExecutionResult {
        // Find ComplexPython operation and extract code
        for operation in ir.operations() {
            if let OpKind::ComplexPython { code } = &operation.kind {
                let executor = PythonExecutor::new();
                let result = executor.execute(&code, &self.initial_vars);

                return if result.success {
                    IRExecutionResult::success(
                        result.variables_set,
                        result.variables_read,
                        ExecutionStrategy::RustPython,
                    )
                } else {
                    IRExecutionResult::failure(
                        result.error.unwrap_or_else(|| "Unknown error".to_string()),
                        ExecutionStrategy::RustPython,
                    )
                };
            }
        }

        IRExecutionResult::failure(
            "No ComplexPython operation found for RustPython execution".to_string(),
            ExecutionStrategy::RustPython,
        )
    }

    /// Execute single operation (static mode)
    fn execute_operation_static(&mut self, op: &Operation) -> Result<(), String> {
        match &op.kind {
            OpKind::StringLiteral { value } => {
                if let Some(result_id) = op.result {
                    self.values.insert(result_id, value.clone());
                }
                Ok(())
            }

            OpKind::GetVar { var_name, .. } => {
                self.variables_read.push(var_name.clone());
                if let Some(result_id) = op.result {
                    // Store the value if available
                    if let Some(value) = self.current_vars.get(var_name) {
                        self.values.insert(result_id, value.clone());
                    } else {
                        // Store empty for unknown variables
                        self.values.insert(result_id, String::new());
                    }
                }
                Ok(())
            }

            OpKind::SetVar { var_name, value } => {
                // Expand any ${...} references in the variable name
                let expanded_var_name = self.expand_var_references(var_name);

                if let Some(value_str) = self.values.get(value) {
                    self.current_vars.insert(expanded_var_name, value_str.clone());
                }
                Ok(())
            }

            OpKind::AppendVar { var_name, value } => {
                // Expand any ${...} references in the variable name
                let expanded_var_name = self.expand_var_references(var_name);

                if let Some(value_str) = self.values.get(value) {
                    let current = self.current_vars.get(&expanded_var_name).cloned().unwrap_or_default();
                    self.current_vars.insert(expanded_var_name, format!("{}{}", current, value_str));
                }
                Ok(())
            }

            OpKind::PrependVar { var_name, value } => {
                // Expand any ${...} references in the variable name
                let expanded_var_name = self.expand_var_references(var_name);

                if let Some(value_str) = self.values.get(value) {
                    let current = self.current_vars.get(&expanded_var_name).cloned().unwrap_or_default();
                    self.current_vars.insert(expanded_var_name, format!("{}{}", value_str, current));
                }
                Ok(())
            }

            _ => {
                // Other operations not supported in static mode
                Ok(())
            }
        }
    }

    /// Execute single operation (hybrid mode with evaluation)
    fn execute_operation_hybrid(&mut self, op: &Operation) -> Result<(), String> {
        match &op.kind {
            OpKind::StringLiteral { value } => {
                if let Some(result_id) = op.result {
                    self.values.insert(result_id, value.clone());
                }
                Ok(())
            }

            OpKind::GetVar { var_name, expand } => {
                self.variables_read.push(var_name.clone());
                if let Some(result_id) = op.result {
                    if let Some(value) = self.current_vars.get(var_name) {
                        let final_value = if *expand {
                            self.expand_value(value)
                        } else {
                            value.clone()
                        };
                        self.values.insert(result_id, final_value);
                    } else {
                        self.values.insert(result_id, String::new());
                    }
                }
                Ok(())
            }

            OpKind::SetVar { var_name, value } => {
                // Expand any ${...} references in the variable name
                let expanded_var_name = self.expand_var_references(var_name);

                if let Some(value_str) = self.values.get(value) {
                    self.current_vars.insert(expanded_var_name, value_str.clone());
                }
                Ok(())
            }

            OpKind::AppendVar { var_name, value } => {
                // Expand any ${...} references in the variable name
                let expanded_var_name = self.expand_var_references(var_name);

                if let Some(value_str) = self.values.get(value) {
                    let current = self.current_vars.get(&expanded_var_name).cloned().unwrap_or_default();
                    self.current_vars.insert(expanded_var_name, format!("{}{}", current, value_str));
                }
                Ok(())
            }

            OpKind::PrependVar { var_name, value } => {
                // Expand any ${...} references in the variable name
                let expanded_var_name = self.expand_var_references(var_name);

                if let Some(value_str) = self.values.get(value) {
                    let current = self.current_vars.get(&expanded_var_name).cloned().unwrap_or_default();
                    self.current_vars.insert(expanded_var_name, format!("{}{}", value_str, current));
                }
                Ok(())
            }

            OpKind::Concat { left, right } => {
                if let Some(result_id) = op.result {
                    let left_val = self.values.get(left).cloned().unwrap_or_default();
                    let right_val = self.values.get(right).cloned().unwrap_or_default();
                    self.values.insert(result_id, format!("{}{}", left_val, right_val));
                }
                Ok(())
            }

            OpKind::Conditional { condition, true_val, false_val } => {
                if let Some(result_id) = op.result {
                    // Evaluate condition
                    let cond_str = self.values.get(condition).cloned().unwrap_or_default();
                    let is_true = self.evaluate_condition(&cond_str);

                    let result = if is_true {
                        self.values.get(true_val).cloned().unwrap_or_default()
                    } else {
                        self.values.get(false_val).cloned().unwrap_or_default()
                    };

                    self.values.insert(result_id, result);
                }
                Ok(())
            }

            OpKind::Contains { var_name, item, true_val, false_val } => {
                self.variables_read.push(var_name.clone());

                if let Some(result_id) = op.result {
                    let var_value = self.current_vars.get(var_name).cloned().unwrap_or_default();
                    let contains = var_value.split_whitespace().any(|s| s == item);

                    let result = if contains {
                        self.values.get(true_val).cloned().unwrap_or_default()
                    } else {
                        self.values.get(false_val).cloned().unwrap_or_default()
                    };

                    self.values.insert(result_id, result);
                }
                Ok(())
            }

            OpKind::StringMethod { value, method, args } => {
                if let Some(result_id) = op.result {
                    let value_str = self.values.get(value).cloned().unwrap_or_default();
                    let result = self.apply_string_method(&value_str, method, args)?;
                    self.values.insert(result_id, result);
                }
                Ok(())
            }

            OpKind::Compare { left, right, op: cmp_op } => {
                if let Some(result_id) = op.result {
                    let left_val = self.values.get(left).cloned().unwrap_or_default();
                    let right_val = self.values.get(right).cloned().unwrap_or_default();
                    let result = self.compare(&left_val, &right_val, cmp_op);
                    self.values.insert(result_id, if result { "True".to_string() } else { "False".to_string() });
                }
                Ok(())
            }

            OpKind::Logical { left, right, op: log_op } => {
                if let Some(result_id) = op.result {
                    let left_val = self.values.get(left).cloned().unwrap_or_default();
                    let right_val = self.values.get(right).cloned().unwrap_or_default();
                    let result = self.logical(&left_val, &right_val, log_op);
                    self.values.insert(result_id, if result { "True".to_string() } else { "False".to_string() });
                }
                Ok(())
            }

            OpKind::ListLiteral { items } => {
                if let Some(result_id) = op.result {
                    let values: Vec<String> = items.iter()
                        .filter_map(|id| self.values.get(id).cloned())
                        .collect();
                    self.values.insert(result_id, values.join(" "));
                }
                Ok(())
            }

            OpKind::DelVar { var_name } => {
                self.current_vars.remove(var_name);
                Ok(())
            }

            _ => {
                // Complex operations (ForLoop, IfStmt, ListComp) not supported in hybrid
                Err(format!("Operation {:?} requires RustPython execution", op.kind))
            }
        }
    }

    /// Simple variable expansion: ${VAR} -> value
    fn expand_value(&self, value: &str) -> String {
        let mut result = value.to_string();

        loop {
            if let Some(start) = result.find("${") {
                if let Some(end) = result[start..].find('}') {
                    let var_name = &result[start + 2..start + end];
                    let replacement = self.current_vars.get(var_name).cloned().unwrap_or_default();
                    result = format!("{}{}{}", &result[..start], replacement, &result[start + end + 1..]);
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        result
    }

    /// Evaluate condition string to boolean
    fn evaluate_condition(&self, condition: &str) -> bool {
        match condition {
            "True" | "true" | "1" | "yes" => true,
            "False" | "false" | "0" | "no" | "" => false,
            _ => !condition.is_empty(),
        }
    }

    /// Apply string method
    fn apply_string_method(
        &self,
        value: &str,
        method: &crate::python_ir::StringMethodKind,
        args: &[ValueId],
    ) -> Result<String, String> {
        use crate::python_ir::StringMethodKind;

        match method {
            StringMethodKind::Upper => Ok(value.to_uppercase()),
            StringMethodKind::Lower => Ok(value.to_lowercase()),
            StringMethodKind::Strip => Ok(value.trim().to_string()),
            StringMethodKind::LStrip => Ok(value.trim_start().to_string()),
            StringMethodKind::RStrip => Ok(value.trim_end().to_string()),

            StringMethodKind::StartsWith => {
                if let Some(arg_id) = args.first() {
                    let arg = self.values.get(arg_id).cloned().unwrap_or_default();
                    Ok(if value.starts_with(&arg) { "True".to_string() } else { "False".to_string() })
                } else {
                    Err("startswith requires 1 argument".to_string())
                }
            }

            StringMethodKind::EndsWith => {
                if let Some(arg_id) = args.first() {
                    let arg = self.values.get(arg_id).cloned().unwrap_or_default();
                    Ok(if value.ends_with(&arg) { "True".to_string() } else { "False".to_string() })
                } else {
                    Err("endswith requires 1 argument".to_string())
                }
            }

            StringMethodKind::Find => {
                if let Some(arg_id) = args.first() {
                    let arg = self.values.get(arg_id).cloned().unwrap_or_default();
                    Ok(value.find(&arg).map(|i| i.to_string()).unwrap_or_else(|| "-1".to_string()))
                } else {
                    Err("find requires 1 argument".to_string())
                }
            }

            StringMethodKind::RFind => {
                if let Some(arg_id) = args.first() {
                    let arg = self.values.get(arg_id).cloned().unwrap_or_default();
                    Ok(value.rfind(&arg).map(|i| i.to_string()).unwrap_or_else(|| "-1".to_string()))
                } else {
                    Err("rfind requires 1 argument".to_string())
                }
            }

            StringMethodKind::Replace => {
                if args.len() >= 2 {
                    let old = self.values.get(&args[0]).cloned().unwrap_or_default();
                    let new = self.values.get(&args[1]).cloned().unwrap_or_default();
                    Ok(value.replace(&old, &new))
                } else {
                    Err("replace requires 2 arguments".to_string())
                }
            }

            StringMethodKind::Split => {
                // For now, split by whitespace
                Ok(value.split_whitespace().collect::<Vec<_>>().join(" "))
            }

            StringMethodKind::Join => {
                if let Some(arg_id) = args.first() {
                    let arg = self.values.get(arg_id).cloned().unwrap_or_default();
                    let items: Vec<&str> = arg.split_whitespace().collect();
                    Ok(items.join(value))
                } else {
                    Err("join requires 1 argument".to_string())
                }
            }
        }
    }

    /// Compare two values
    fn compare(&self, left: &str, right: &str, op: &crate::python_ir::CompareOp) -> bool {
        use crate::python_ir::CompareOp;

        match op {
            CompareOp::Eq => left == right,
            CompareOp::Ne => left != right,
            CompareOp::Lt => left < right,
            CompareOp::Le => left <= right,
            CompareOp::Gt => left > right,
            CompareOp::Ge => left >= right,
            CompareOp::In => right.split_whitespace().any(|s| s == left),
        }
    }

    /// Logical operation
    fn logical(&self, left: &str, right: &str, op: &crate::python_ir::LogicalOp) -> bool {
        use crate::python_ir::LogicalOp;

        let left_bool = self.evaluate_condition(left);
        let right_bool = self.evaluate_condition(right);

        match op {
            LogicalOp::And => left_bool && right_bool,
            LogicalOp::Or => left_bool || right_bool,
            LogicalOp::Not => !left_bool, // Only uses left
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::python_ir::PythonIRBuilder;

    #[test]
    fn test_execute_simple() {
        let mut builder = PythonIRBuilder::new();
        let value = builder.string_literal("test_value");
        builder.setvar("TEST_VAR", value);
        let ir = builder.build();

        let mut executor = IRExecutor::new(HashMap::new());
        let result = executor.execute(&ir);

        assert!(result.success);
        assert_eq!(result.variables_set.get("TEST_VAR"), Some(&"test_value".to_string()));
    }

    #[test]
    fn test_execute_getvar_setvar() {
        let mut builder = PythonIRBuilder::new();
        let workdir = builder.getvar("WORKDIR");
        builder.setvar("BUILD_DIR", workdir);
        let ir = builder.build();

        let mut initial = HashMap::new();
        initial.insert("WORKDIR".to_string(), "/tmp/work".to_string());

        let mut executor = IRExecutor::new(initial);
        let result = executor.execute(&ir);

        assert!(result.success);
        assert_eq!(result.variables_set.get("BUILD_DIR"), Some(&"/tmp/work".to_string()));
        assert!(result.variables_read.contains(&"WORKDIR".to_string()));
    }

    #[test]
    fn test_execute_appendvar() {
        let mut builder = PythonIRBuilder::new();
        let suffix = builder.string_literal(" extra");
        builder.appendvar("DEPENDS", suffix);
        let ir = builder.build();

        let mut initial = HashMap::new();
        initial.insert("DEPENDS".to_string(), "base".to_string());

        let mut executor = IRExecutor::new(initial);
        let result = executor.execute(&ir);

        assert!(result.success);
        assert_eq!(result.variables_set.get("DEPENDS"), Some(&"base extra".to_string()));
    }

    #[test]
    fn test_execute_contains() {
        let mut builder = PythonIRBuilder::new();
        let true_val = builder.string_literal("yes");
        let false_val = builder.string_literal("no");
        let result_val = builder.contains("FEATURES", "systemd", true_val, false_val);
        builder.setvar("HAS_SYSTEMD", result_val);
        let ir = builder.build();

        let mut initial = HashMap::new();
        initial.insert("FEATURES".to_string(), "systemd x11 wayland".to_string());

        let mut executor = IRExecutor::new(initial);
        let result = executor.execute(&ir);

        assert!(result.success);
        assert_eq!(result.variables_set.get("HAS_SYSTEMD"), Some(&"yes".to_string()));
    }

    #[test]
    fn test_variable_expansion() {
        let mut builder = PythonIRBuilder::new();
        let base = builder.getvar_with_expand("BASE", true);
        builder.setvar("INSTALL_DIR", base);
        let ir = builder.build();

        let mut initial = HashMap::new();
        initial.insert("BASE".to_string(), "${PREFIX}/app".to_string());
        initial.insert("PREFIX".to_string(), "/usr/local".to_string());

        let mut executor = IRExecutor::new(initial);
        let result = executor.execute(&ir);

        assert!(result.success);
        assert_eq!(result.variables_set.get("INSTALL_DIR"), Some(&"/usr/local/app".to_string()));
    }

    #[test]
    fn test_execution_strategy() {
        // Simple operations should use Static
        let mut builder = PythonIRBuilder::new();
        let val = builder.string_literal("test");
        builder.setvar("VAR", val);
        let ir = builder.build();
        assert_eq!(ir.execution_strategy(), ExecutionStrategy::Static);

        // Moderate complexity should use Hybrid
        let mut builder = PythonIRBuilder::new();
        for _ in 0..10 {
            let val = builder.string_literal("item");
            builder.appendvar("LIST", val);
        }
        let ir = builder.build();
        assert_eq!(ir.execution_strategy(), ExecutionStrategy::Hybrid);
    }
}
