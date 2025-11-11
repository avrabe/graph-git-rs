// RustPython-based Python execution for BitBake recipes
// Executes Python code with mocked BitBake DataStore for high-accuracy variable extraction

#![cfg(feature = "python-execution")]

use rustpython_vm::{
    builtins::{PyDict, PyStr, PyStrRef},
    AsObject, Interpreter, PyObjectRef, PyResult, VirtualMachine,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
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

/// Python-accessible DataStore object
pub struct MockDataStore {
    inner: Rc<RefCell<DataStoreInner>>,
}

impl MockDataStore {
    pub fn new(inner: Rc<RefCell<DataStoreInner>>) -> Self {
        Self { inner }
    }

    /// Python method: d.getVar(name, expand=True)
    pub fn py_getvar(&self, name: PyStrRef, expand: Option<bool>, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let name_str = name.as_str();
        let expand_val = expand.unwrap_or(true);

        let result = self.inner.borrow_mut().get_var(name_str, expand_val);

        match result {
            Some(value) => Ok(vm.ctx.new_str(value).into()),
            None => Ok(vm.ctx.none()),
        }
    }

    /// Python method: d.setVar(name, value)
    pub fn py_setvar(&self, name: PyStrRef, value: PyStrRef, _vm: &VirtualMachine) -> PyResult<()> {
        self.inner.borrow_mut().set_var(name.as_str().to_string(), value.as_str().to_string());
        Ok(())
    }

    /// Python method: d.appendVar(name, value)
    pub fn py_appendvar(&self, name: PyStrRef, value: PyStrRef, _vm: &VirtualMachine) -> PyResult<()> {
        self.inner.borrow_mut().append_var(name.as_str().to_string(), value.as_str().to_string());
        Ok(())
    }

    /// Python method: d.prependVar(name, value)
    pub fn py_prependvar(&self, name: PyStrRef, value: PyStrRef, _vm: &VirtualMachine) -> PyResult<()> {
        self.inner.borrow_mut().prepend_var(name.as_str().to_string(), value.as_str().to_string());
        Ok(())
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
        // Create interpreter with minimal initialization
        let interp = Interpreter::with_init(Default::default(), |_vm| {
            // Minimal init - no stdlib needed
        });

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
        // Create inner DataStore
        let inner = Rc::new(RefCell::new(DataStoreInner::new()));

        // Populate with initial variables
        for (key, value) in initial_vars {
            inner.borrow_mut().set_initial(key.clone(), value.clone());
        }

        // Create Python object with methods
        let d_obj = create_datastore_object(vm, inner.clone())?;

        // Set 'd' as a global in builtins
        vm.builtins.set_attr("d", d_obj, vm)?;

        // Execute the Python code
        let code_obj = match vm.compile(python_code, rustpython_vm::compiler::Mode::Exec, "<bitbake>".to_owned()) {
            Ok(code) => code,
            Err(e) => return Ok(PythonExecutionResult::failure(format!("Compile error: {:?}", e))),
        };

        match vm.run_code_obj(code_obj, vm.new_scope_with_builtins()) {
            Ok(_) => {
                // Extract final state from inner DataStore
                let inner_clone = Rc::try_unwrap(inner).unwrap_or_else(|rc| (*rc).clone());
                let result = inner_clone.into_inner().into_result();
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

// Helper to create mock DataStore object with methods
fn create_datastore_object(
    vm: &VirtualMachine,
    inner: Rc<RefCell<DataStoreInner>>,
) -> PyResult<PyObjectRef> {
    // Create an object using builtins.object
    let obj = vm.ctx.new_base_object(vm.ctx.types.object_type.to_owned(), None);

    // Create closures that capture the inner DataStore
    let inner_getvar = inner.clone();
    let getvar_fn = vm.new_function(
        "getVar",
        move |name: PyStrRef, expand: Option<bool>, vm: &VirtualMachine| -> PyResult<PyObjectRef> {
            let expand_val = expand.unwrap_or(true);
            let result = inner_getvar.borrow_mut().get_var(name.as_str(), expand_val);
            match result {
                Some(value) => Ok(vm.ctx.new_str(value).into()),
                None => Ok(vm.ctx.none()),
            }
        },
    );

    let inner_setvar = inner.clone();
    let setvar_fn = vm.new_function(
        "setVar",
        move |name: PyStrRef, value: PyStrRef, _vm: &VirtualMachine| -> PyResult<()> {
            inner_setvar.borrow_mut().set_var(
                name.as_str().to_string(),
                value.as_str().to_string(),
            );
            Ok(())
        },
    );

    let inner_appendvar = inner.clone();
    let appendvar_fn = vm.new_function(
        "appendVar",
        move |name: PyStrRef, value: PyStrRef, _vm: &VirtualMachine| -> PyResult<()> {
            inner_appendvar.borrow_mut().append_var(
                name.as_str().to_string(),
                value.as_str().to_string(),
            );
            Ok(())
        },
    );

    let inner_prependvar = inner.clone();
    let prependvar_fn = vm.new_function(
        "prependVar",
        move |name: PyStrRef, value: PyStrRef, _vm: &VirtualMachine| -> PyResult<()> {
            inner_prependvar.borrow_mut().prepend_var(
                name.as_str().to_string(),
                value.as_str().to_string(),
            );
            Ok(())
        },
    );

    // Add methods as attributes
    obj.set_attr("getVar", getvar_fn, vm)?;
    obj.set_attr("setVar", setvar_fn, vm)?;
    obj.set_attr("appendVar", appendvar_fn, vm)?;
    obj.set_attr("prependVar", prependvar_fn, vm)?;

    Ok(obj.into())
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
