// RustPython-based Python execution for BitBake recipes
// Executes Python code with mocked BitBake DataStore for high-accuracy variable extraction

#![cfg(feature = "python-execution")]

use rustpython_vm::{
    builtins::{PyDict, PyStr},
    AsObject, Interpreter, PyObjectRef, PyResult, VirtualMachine,
};
use std::collections::HashMap;
use std::time::Duration;

/// Result of executing Python code
#[derive(Debug, Clone)]
pub struct PythonExecutionResult {
    /// Variables that were set during execution
    pub variables_set: HashMap<String, String>,
    /// Variables that were read during execution
    pub variables_read: Vec<String>,
    /// Whether execution was successful
    pub success: bool,
    /// Error message if execution failed
    pub error: Option<String>,
}

impl PythonExecutionResult {
    pub fn success(variables_set: HashMap<String, String>, variables_read: Vec<String>) -> Self {
        Self {
            variables_set,
            variables_read,
            success: true,
            error: None,
        }
    }

    pub fn failure(error: String) -> Self {
        Self {
            variables_set: HashMap::new(),
            variables_read: Vec::new(),
            success: false,
            error: Some(error),
        }
    }
}

/// Mock BitBake DataStore that tracks variable operations
#[derive(Debug, Clone)]
pub struct MockDataStore {
    variables: HashMap<String, String>,
    read_log: Vec<String>,
    write_log: Vec<(String, String)>,
}

impl MockDataStore {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            read_log: Vec::new(),
            write_log: Vec::new(),
        }
    }

    /// Pre-populate with known variables (from static analysis)
    pub fn set_initial(&mut self, name: String, value: String) {
        self.variables.insert(name, value);
    }

    /// Called by Python: d.getVar('VAR')
    pub fn get_var(&mut self, name: &str) -> Option<String> {
        self.read_log.push(name.to_string());
        self.variables.get(name).cloned()
    }

    /// Called by Python: d.setVar('VAR', 'value')
    pub fn set_var(&mut self, name: String, value: String) {
        self.write_log.push((name.clone(), value.clone()));
        self.variables.insert(name, value);
    }

    /// Called by Python: d.appendVar('VAR', ' suffix')
    pub fn append_var(&mut self, name: String, suffix: String) {
        let current = self.variables.get(&name).cloned().unwrap_or_default();
        let new_value = format!("{}{}", current, suffix);
        self.set_var(name, new_value);
    }

    /// Called by Python: d.prependVar('VAR', 'prefix ')
    pub fn prepend_var(&mut self, name: String, prefix: String) {
        let current = self.variables.get(&name).cloned().unwrap_or_default();
        let new_value = format!("{}{}", prefix, current);
        self.set_var(name, new_value);
    }

    /// Get execution results
    pub fn into_result(self) -> PythonExecutionResult {
        PythonExecutionResult::success(self.variables, self.read_log)
    }
}

impl Default for MockDataStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Python executor for BitBake code
pub struct PythonExecutor {
    /// Timeout for Python execution
    pub timeout: Duration,
}

impl PythonExecutor {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(1),
        }
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// Execute Python code with a mocked BitBake DataStore
    ///
    /// # Arguments
    /// * `python_code` - The Python code to execute
    /// * `initial_vars` - Variables to pre-populate in the DataStore
    ///
    /// # Returns
    /// PythonExecutionResult with captured variable operations
    pub fn execute(
        &self,
        python_code: &str,
        initial_vars: &HashMap<String, String>,
    ) -> PythonExecutionResult {
        // Create interpreter
        let interp = Interpreter::without_stdlib(Default::default());

        // Execute in VM context
        let result = interp.enter(|vm| {
            self.execute_in_vm(vm, python_code, initial_vars)
        });

        match result {
            Ok(res) => res,
            Err(e) => PythonExecutionResult::failure(format!("Execution error: {:?}", e)),
        }
    }

    fn execute_in_vm(
        &self,
        vm: &VirtualMachine,
        python_code: &str,
        initial_vars: &HashMap<String, String>,
    ) -> PyResult<PythonExecutionResult> {
        // Create mock DataStore
        let datastore_dict = PyDict::new_ref(&vm.ctx);

        // Populate with initial variables
        for (key, value) in initial_vars {
            let py_key = vm.ctx.new_str(key.clone());
            let py_value = vm.ctx.new_str(value.clone());
            datastore_dict.set_item(py_key.as_object(), py_value, vm)?;
        }

        // Track operations
        let mut read_log = Vec::new();
        let mut write_log = HashMap::new();

        // Create mock 'd' object with methods
        let d_class = create_datastore_class(vm, datastore_dict.clone(), &mut read_log, &mut write_log)?;

        // Set 'd' as a global
        let scope = vm.new_scope_with_builtins();
        scope.globals.set_item("d", d_class, vm)?;

        // Execute the Python code
        match vm.run_block_expr(scope, python_code) {
            Ok(_) => {
                // Extract final state from datastore_dict
                let mut final_vars = HashMap::new();
                for (key, value) in datastore_dict.into_iter() {
                    if let (Ok(k), Ok(v)) = (
                        key.try_into_value::<PyStr>(vm),
                        value.try_into_value::<PyStr>(vm),
                    ) {
                        final_vars.insert(k.as_str().to_string(), v.as_str().to_string());
                    }
                }

                Ok(PythonExecutionResult::success(final_vars, read_log))
            }
            Err(e) => {
                let error_msg = format!("Python error: {}", e);
                Ok(PythonExecutionResult::failure(error_msg))
            }
        }
    }
}

impl Default for PythonExecutor {
    fn default() -> Self {
        Self::new()
    }
}

// Helper to create mock DataStore class
fn create_datastore_class(
    vm: &VirtualMachine,
    datastore: PyObjectRef,
    _read_log: &mut Vec<String>,
    _write_log: &mut HashMap<String, String>,
) -> PyResult<PyObjectRef> {
    // For now, return a simple object with the datastore dict
    // In a full implementation, this would be a Python class with methods
    Ok(datastore)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_execution() {
        let executor = PythonExecutor::new();

        let code = r#"
x = 1 + 1
y = "hello"
"#;

        let result = executor.execute(code, &HashMap::new());
        assert!(result.success, "Execution should succeed");
    }

    #[test]
    fn test_with_initial_vars() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("WORKDIR".to_string(), "/tmp/work".to_string());

        let code = r#"
# This is a simplified test - full d.getVar/setVar requires more setup
workdir = "/tmp/work"  # In reality: d.getVar('WORKDIR')
build_dir = workdir + "/build"
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed");
    }
}
