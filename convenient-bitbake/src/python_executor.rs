// RustPython-based Python execution for BitBake recipes
// Executes Python code with mocked BitBake DataStore for high-accuracy variable extraction

#![cfg(feature = "python-execution")]

use rustpython::{
    vm::{builtins::PyStrRef, pyclass, pymodule, PyObjectRef, PyPayload, PyResult, VirtualMachine},
    InterpreterConfig,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Module containing the DataStore class
#[pymodule]
mod bitbake_internal {
    use super::*;

    #[pyattr]
    #[pyclass(module = "bitbake_internal", name = "DataStore")]
    #[derive(Debug, Clone, PyPayload)]
    pub(super) struct DataStore {
        pub(super) inner: Arc<Mutex<DataStoreInner>>,
    }

    #[pyclass]
    impl DataStore {
        #[pymethod]
        fn getVar(&self, name: PyStrRef, expand: Option<bool>, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
            let name_str = name.as_str();
            let expand_val = expand.unwrap_or(true);

            let result = self.inner.lock().unwrap().get_var(name_str, expand_val);

            match result {
                Some(value) => Ok(vm.ctx.new_str(value).into()),
                None => Ok(vm.ctx.none()),
            }
        }

        #[pymethod]
        fn setVar(&self, name: PyStrRef, value: PyStrRef, _vm: &VirtualMachine) -> PyResult<()> {
            self.inner.lock().unwrap().set_var(name.as_str().to_string(), value.as_str().to_string());
            Ok(())
        }

        #[pymethod]
        fn appendVar(&self, name: PyStrRef, value: PyStrRef, _vm: &VirtualMachine) -> PyResult<()> {
            self.inner.lock().unwrap().append_var(name.as_str().to_string(), value.as_str().to_string());
            Ok(())
        }

        #[pymethod]
        fn prependVar(&self, name: PyStrRef, value: PyStrRef, _vm: &VirtualMachine) -> PyResult<()> {
            self.inner.lock().unwrap().prepend_var(name.as_str().to_string(), value.as_str().to_string());
            Ok(())
        }
    }
}

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
/// This is wrapped in Rc<RefCell<>> to allow mutation from Python
#[derive(Debug, Clone)]
pub struct DataStoreInner {
    variables: HashMap<String, String>,
    read_log: Vec<String>,
    write_log: Vec<(String, String)>,
    expand_enabled: bool,
}

impl DataStoreInner {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            read_log: Vec::new(),
            write_log: Vec::new(),
            expand_enabled: true,
        }
    }

    /// Pre-populate with known variables (from static analysis)
    pub fn set_initial(&mut self, name: String, value: String) {
        self.variables.insert(name, value);
    }

    /// Called by Python: d.getVar('VAR', expand=True)
    pub fn get_var(&mut self, name: &str, expand: bool) -> Option<String> {
        self.read_log.push(name.to_string());
        if let Some(value) = self.variables.get(name) {
            if expand && self.expand_enabled {
                // Simple expansion: ${VAR} replacement
                Some(self.expand_value(value))
            } else {
                Some(value.clone())
            }
        } else {
            None
        }
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

    /// Simple variable expansion: ${VAR} -> value
    fn expand_value(&self, value: &str) -> String {
        let mut result = value.to_string();

        // Simple regex-free expansion for ${VAR}
        loop {
            if let Some(start) = result.find("${") {
                if let Some(end) = result[start..].find('}') {
                    let var_name = &result[start + 2..start + end];
                    let replacement = self.variables.get(var_name).cloned().unwrap_or_default();
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

    /// Get execution results
    pub fn into_result(self) -> PythonExecutionResult {
        PythonExecutionResult::success(self.variables, self.read_log)
    }
}

impl Default for DataStoreInner {
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
        // Create interpreter with module registration using InterpreterConfig
        let interp = InterpreterConfig::new()
            .init_hook(Box::new(|vm| {
                // Register our bitbake_internal module with the DataStore class
                vm.add_native_module(
                    "bitbake_internal".to_owned(),
                    Box::new(bitbake_internal::make_module),
                );
            }))
            .interpreter();

        // Execute in VM context
        match interp.enter(|vm| {
            self.execute_in_vm(vm, python_code, initial_vars)
        }) {
            Ok(result) => result,
            Err(e) => PythonExecutionResult::failure(format!("Execution error: {:?}", e)),
        }
    }

    fn execute_in_vm(
        &self,
        vm: &VirtualMachine,
        python_code: &str,
        initial_vars: &HashMap<String, String>,
    ) -> PyResult<PythonExecutionResult> {
        // Create inner DataStoreInner
        let inner = Arc::new(Mutex::new(DataStoreInner::new()));

        // Populate with initial variables
        for (key, value) in initial_vars {
            inner.lock().unwrap().set_initial(key.clone(), value.clone());
        }

        // Create DataStore as a Python object using our registered class
        let datastore = bitbake_internal::DataStore {
            inner: inner.clone(),
        };
        let d_obj = datastore.into_pyobject(vm);

        // Create a new scope and add 'd' as a global
        let scope = vm.new_scope_with_builtins();
        scope.globals.set_item("d", d_obj, vm)?;

        // Execute the Python code
        let code_obj = match vm.compile(python_code, rustpython_vm::compiler::Mode::Exec, "<bitbake>".to_owned()) {
            Ok(code) => code,
            Err(e) => return Ok(PythonExecutionResult::failure(format!("Compile error: {:?}", e))),
        };

        match vm.run_code_obj(code_obj, scope) {
            Ok(_) => {
                // Extract final state from inner DataStore
                // Try to unwrap Arc, if it fails (still has references), clone the data
                let result = match Arc::try_unwrap(inner) {
                    Ok(mutex) => mutex.into_inner().unwrap().into_result(),
                    Err(arc) => arc.lock().unwrap().clone().into_result(),
                };
                Ok(result)
            }
            Err(e) => {
                let error_msg = format!("Python error: {:?}", e);
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
    fn test_datastore_setvar() {
        let executor = PythonExecutor::new();

        let code = r#"
d.setVar("BUILD_DIR", "/tmp/build")
d.setVar("VERSION", "1.0.0")
"#;

        let result = executor.execute(code, &HashMap::new());
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("BUILD_DIR"), Some(&"/tmp/build".to_string()));
        assert_eq!(result.variables_set.get("VERSION"), Some(&"1.0.0".to_string()));
    }

    #[test]
    fn test_datastore_getvar() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("WORKDIR".to_string(), "/tmp/work".to_string());
        initial.insert("PN".to_string(), "mypackage".to_string());

        let code = r#"
workdir = d.getVar("WORKDIR")
pn = d.getVar("PN")
build_dir = workdir + "/build/" + pn
d.setVar("BUILD_DIR", build_dir)
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("BUILD_DIR"), Some(&"/tmp/work/build/mypackage".to_string()));
        assert!(result.variables_read.contains(&"WORKDIR".to_string()));
        assert!(result.variables_read.contains(&"PN".to_string()));
    }

    #[test]
    fn test_datastore_appendvar() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("DEPENDS".to_string(), "glibc".to_string());

        let code = r#"
d.appendVar("DEPENDS", " openssl zlib")
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("DEPENDS"), Some(&"glibc openssl zlib".to_string()));
    }

    #[test]
    fn test_datastore_prependvar() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("CFLAGS".to_string(), "-O2".to_string());

        let code = r#"
d.prependVar("CFLAGS", "-Wall ")
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("CFLAGS"), Some(&"-Wall -O2".to_string()));
    }

    #[test]
    fn test_variable_expansion() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("WORKDIR".to_string(), "/tmp/work".to_string());
        initial.insert("PN".to_string(), "mypackage".to_string());

        let code = r#"
d.setVar("S", "${WORKDIR}/${PN}/src")
expanded = d.getVar("S", True)
d.setVar("S_EXPANDED", expanded)
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("S_EXPANDED"), Some(&"/tmp/work/mypackage/src".to_string()));
    }

    #[test]
    fn test_complex_bitbake_code() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("PV".to_string(), "1.2.3".to_string());
        initial.insert("PR".to_string(), "r0".to_string());
        initial.insert("PN".to_string(), "myapp".to_string());

        let code = r#"
# Common BitBake pattern
pv = d.getVar("PV")
pr = d.getVar("PR")
pn = d.getVar("PN")

# Build full version string
full_version = pn + "-" + pv + "-" + pr
d.setVar("PF", full_version)

# Conditional logic
if pv.startswith("1."):
    d.setVar("MAJOR_VERSION", "1")
else:
    d.setVar("MAJOR_VERSION", "unknown")
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("PF"), Some(&"myapp-1.2.3-r0".to_string()));
        assert_eq!(result.variables_set.get("MAJOR_VERSION"), Some(&"1".to_string()));
    }
}
