// RustPython-based Python execution for BitBake recipes
// Executes Python code with BitBake DataStore for high-accuracy variable extraction
//
// NOTE: RustPython is now ALWAYS enabled (no longer a feature flag)

use rustpython::{
    vm::{builtins::PyStrRef, pyclass, pymodule, PyObjectRef, PyPayload, PyResult, VirtualMachine},
    InterpreterConfig, Interpreter,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

// Module containing bb.utils functions
#[pymodule]
mod bb_utils {
    use super::*;

    #[pyfunction]
    fn contains(
        var: PyStrRef,
        item: PyStrRef,
        true_val: PyObjectRef,
        false_val: PyObjectRef,
        d: PyObjectRef,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        // Get the DataStore from 'd'
        if let Some(datastore) = d.downcast_ref::<bitbake_internal::DataStore>() {
            // Get the variable value
            if let Some(value) = datastore.inner.lock().unwrap().get_var(var.as_str(), true) {
                // Check if item is in the value (space-separated)
                let items: Vec<&str> = value.split_whitespace().collect();
                if items.contains(&item.as_str()) {
                    Ok(true_val)
                } else {
                    Ok(false_val)
                }
            } else {
                // Variable not found, return false_val
                Ok(false_val)
            }
        } else {
            Err(vm.new_type_error("Expected DataStore as 'd' parameter".to_string()))
        }
    }
}

// Module containing bb
#[pymodule]
mod bb {
    use super::*;

    #[pyattr]
    fn utils(_vm: &VirtualMachine) -> PyObjectRef {
        // This will be set up in the interpreter init
        _vm.ctx.none()
    }
}

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
        #[pymethod(magic)]
        fn __init__(_zelf: PyObjectRef, _vm: &VirtualMachine) -> PyResult<()> {
            // Constructor for Python - not actually used since we create from Rust
            Ok(())
        }

        #[pymethod]
        fn getVar(&self, name: PyStrRef, expand: rustpython_vm::function::OptionalArg<bool>, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
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

        #[pymethod]
        fn expand(&self, value: PyStrRef, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
            let expanded = self.inner.lock().unwrap().expand_value(value.as_str());
            Ok(vm.ctx.new_str(expanded).into())
        }
    }
}

// Thread-local cached RustPython interpreter
// Each thread gets its own interpreter instance, dramatically reducing
// the overhead of interpreter creation (from ~50-200ms to ~0.01ms per eval)
thread_local! {
    static CACHED_INTERPRETER: RefCell<Option<Interpreter>> = RefCell::new(None);
}

// Performance tracking: count total interpreters created across all threads
static INTERPRETER_CREATION_COUNT: AtomicU64 = AtomicU64::new(0);
// Performance tracking: count total evaluations
static EVAL_COUNT: AtomicU64 = AtomicU64::new(0);
// Performance tracking: cumulative time spent in eval (microseconds)
static EVAL_TIME_US: AtomicU64 = AtomicU64::new(0);

/// Get or create the thread-local cached interpreter
fn get_cached_interpreter() -> Interpreter {
    CACHED_INTERPRETER.with(|cache| {
        let mut cache_mut = cache.borrow_mut();
        if cache_mut.is_none() {
            // First access in this thread - create and cache the interpreter
            let start = Instant::now();
            let interp = InterpreterConfig::new()
                .init_stdlib()
                .init_hook(Box::new(|vm| {
                    // Register our bitbake_internal module with the DataStore class
                    vm.add_native_module(
                        "bitbake_internal".to_owned(),
                        Box::new(bitbake_internal::make_module),
                    );

                    // Register bb.utils module
                    vm.add_native_module(
                        "bb.utils".to_owned(),
                        Box::new(bb_utils::make_module),
                    );

                    // Register bb.fetch2 module (Python-to-Rust fetch bridge)
                    vm.add_native_module(
                        "bb.fetch2".to_owned(),
                        Box::new(crate::python_bridge::bb_fetch2::make_module),
                    );

                    // Register top-level bb module
                    vm.add_native_module(
                        "bb".to_owned(),
                        Box::new(crate::python_bridge::bb::make_module),
                    );
                }))
                .interpreter();

            let creation_time = start.elapsed();
            let count = INTERPRETER_CREATION_COUNT.fetch_add(1, Ordering::Relaxed) + 1;

            // Log interpreter creation (helpful for debugging and performance analysis)
            if std::env::var("BITBAKE_PYTHON_DEBUG").is_ok() {
                eprintln!("[RustPython] Created interpreter #{} in thread {:?} (took {:?})",
                         count, std::thread::current().id(), creation_time);
            }

            *cache_mut = Some(interp);
        }
        // Clone the interpreter (Arc internally, so this is cheap)
        cache_mut.as_ref().unwrap().clone()
    })
}

/// Get performance statistics for Python execution
pub fn get_performance_stats() -> PythonPerformanceStats {
    PythonPerformanceStats {
        interpreter_count: INTERPRETER_CREATION_COUNT.load(Ordering::Relaxed),
        eval_count: EVAL_COUNT.load(Ordering::Relaxed),
        total_eval_time_us: EVAL_TIME_US.load(Ordering::Relaxed),
    }
}

/// Performance statistics for Python execution
#[derive(Debug, Clone)]
pub struct PythonPerformanceStats {
    /// Total number of interpreters created
    pub interpreter_count: u64,
    /// Total number of evaluations performed
    pub eval_count: u64,
    /// Total time spent in evaluations (microseconds)
    pub total_eval_time_us: u64,
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

    /// Expand variable references like ${PN} in a string
    fn expand_vars(&self, s: &str) -> String {
        let mut result = s.to_string();
        while let Some(start) = result.find("${") {
            if let Some(end) = result[start..].find('}') {
                let var_name = &result[start + 2..start + end];
                if let Some(value) = self.variables.get(var_name) {
                    result.replace_range(start..start + end + 1, value);
                } else {
                    break; // Stop if we can't expand
                }
            } else {
                break;
            }
        }
        result
    }

    /// Called by Python: d.setVar('VAR', 'value')
    pub fn set_var(&mut self, name: String, value: String) {
        let expanded_name = self.expand_vars(&name);
        self.write_log.push((expanded_name.clone(), value.clone()));
        self.variables.insert(expanded_name, value);
    }

    /// Called by Python: d.appendVar('VAR', ' suffix')
    pub fn append_var(&mut self, name: String, suffix: String) {
        let expanded_name = self.expand_vars(&name);
        let current = self.variables.get(&expanded_name).cloned().unwrap_or_default();
        let new_value = format!("{}{}", current, suffix);
        self.set_var(expanded_name, new_value);
    }

    /// Called by Python: d.prependVar('VAR', 'prefix ')
    pub fn prepend_var(&mut self, name: String, prefix: String) {
        let expanded_name = self.expand_vars(&name);
        let current = self.variables.get(&expanded_name).cloned().unwrap_or_default();
        let new_value = format!("{}{}", prefix, current);
        self.set_var(expanded_name, new_value);
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

    /// Dedent Python code by removing common leading whitespace
    /// Similar to Python's textwrap.dedent()
    fn dedent(code: &str) -> String {
        let lines: Vec<&str> = code.lines().collect();

        // Find minimum indentation (excluding empty lines)
        let min_indent = lines.iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);

        // Remove that amount of indentation from each line
        lines.iter()
            .map(|line| {
                if line.trim().is_empty() {
                    "" // Keep empty lines empty
                } else if line.len() >= min_indent {
                    &line[min_indent..]
                } else {
                    line
                }
            })
            .collect::<Vec<&str>>()
            .join("\n")
    }

    /// Evaluate a Python expression and return its value as a string
    ///
    /// This is used for BitBake's ${@python_expr} inline expressions.
    ///
    /// # Arguments
    /// * `python_expr` - The Python expression to evaluate (e.g., "d.getVar('CFLAGS')")
    /// * `initial_vars` - Variables to pre-populate in the DataStore
    ///
    /// # Returns
    /// Result with the expression value as a string, or an error message
    pub fn eval(
        &self,
        python_expr: &str,
        initial_vars: &HashMap<String, String>,
    ) -> Result<String, String> {
        let start = Instant::now();

        // Use thread-local cached interpreter
        let interp = get_cached_interpreter();

        // Execute in VM context
        let result = interp.enter(|vm| {
            self.eval_in_vm(vm, python_expr, initial_vars)
        }).map_err(|e| format!("Evaluation error: {:?}", e));

        // Record performance metrics
        let elapsed_us = start.elapsed().as_micros() as u64;
        EVAL_COUNT.fetch_add(1, Ordering::Relaxed);
        EVAL_TIME_US.fetch_add(elapsed_us, Ordering::Relaxed);

        if std::env::var("BITBAKE_PYTHON_DEBUG").is_ok() {
            eprintln!("[RustPython] eval() took {}μs: {}", elapsed_us,
                     if python_expr.len() > 50 {
                         format!("{}...", &python_expr[..50])
                     } else {
                         python_expr.to_string()
                     });
        }

        result
    }

    fn eval_in_vm(
        &self,
        vm: &VirtualMachine,
        python_expr: &str,
        initial_vars: &HashMap<String, String>,
    ) -> PyResult<String> {
        // Create inner DataStoreInner
        let inner = Arc::new(Mutex::new(DataStoreInner::new()));

        // Populate with initial variables
        for (key, value) in initial_vars {
            inner.lock().unwrap().set_initial(key.clone(), value.clone());
        }

        // Import our module first to ensure type registration
        let scope = vm.new_scope_with_builtins();
        vm.run_block_expr(scope.clone(), "import bitbake_internal")?;

        // Create DataStore as a Python object using our registered class
        let datastore = bitbake_internal::DataStore {
            inner: inner.clone(),
        };
        let d_obj = datastore.into_pyobject(vm);

        // Add 'd' as a global
        scope.globals.set_item("d", d_obj.clone(), vm)?;

        // Create bb.utils module with contains function and helper functions
        let bb_utils_code = r#"
class _BBUtils:
    def __init__(self, d_obj):
        self._d = d_obj

    def contains(self, var, item, true_val, false_val, d):
        # Get variable value from datastore
        value = d.getVar(var, True)
        if value is None:
            return false_val
        # Check if item is in the space-separated value
        items = value.split()
        return true_val if item in items else false_val

class _BB:
    def __init__(self, d_obj):
        self.utils = _BBUtils(d_obj)

bb = _BB(d)

# Helper function used by os-release recipe
def sanitise_value(value):
    """Sanitise value for unquoted OS release fields"""
    # Simple sanitisation: remove quotes and dangerous characters
    value = value.replace('"', '').replace("'", '').replace('`', '')
    return value.strip()
"#;
        vm.run_block_expr(scope.clone(), bb_utils_code)?;

        // Compile the expression in Eval mode
        let code_obj = match vm.compile(
            python_expr,
            rustpython_vm::compiler::Mode::Eval,
            "<bitbake_expr>".to_owned()
        ) {
            Ok(code) => code,
            Err(e) => {
                return Err(vm.new_exception_msg(
                    vm.ctx.exceptions.syntax_error.to_owned(),
                    format!("Compile error: {:?}", e),
                ));
            }
        };

        // Evaluate the expression
        let result_obj = vm.run_code_obj(code_obj, scope)?;

        // Convert result to string
        let result_str = result_obj.str(vm)?;
        Ok(result_str.as_str().to_string())
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
        // Use thread-local cached interpreter
        let interp = get_cached_interpreter();

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

        // Import our module first to ensure type registration
        let scope = vm.new_scope_with_builtins();
        vm.run_block_expr(scope.clone(), "import bitbake_internal")?;

        // Create DataStore as a Python object using our registered class
        let datastore = bitbake_internal::DataStore {
            inner: inner.clone(),
        };
        let d_obj = datastore.into_pyobject(vm);

        // Add 'd' as a global
        scope.globals.set_item("d", d_obj.clone(), vm)?;

        // Create bb.utils module with contains function and helper functions
        let bb_utils_code = r#"
class _BBUtils:
    def __init__(self, d_obj):
        self._d = d_obj

    def contains(self, var, item, true_val, false_val, d):
        # Get variable value from datastore
        value = d.getVar(var, True)
        if value is None:
            return false_val
        # Check if item is in the space-separated value
        items = value.split()
        return true_val if item in items else false_val

class _BB:
    def __init__(self, d_obj):
        self.utils = _BBUtils(d_obj)

bb = _BB(d)

# Helper function used by os-release recipe
def sanitise_value(value):
    """Sanitise value for unquoted OS release fields"""
    # Simple sanitisation: remove quotes and dangerous characters
    value = value.replace('"', '').replace("'", '').replace('`', '')
    return value.strip()
"#;
        vm.run_block_expr(scope.clone(), bb_utils_code)?;

        // Dedent the Python code to remove common leading whitespace
        let dedented_code = Self::dedent(python_code);

        // Execute the Python code
        let code_obj = match vm.compile(&dedented_code, rustpython_vm::compiler::Mode::Exec, "<bitbake>".to_owned()) {
            Ok(code) => code,
            Err(e) => return Ok(PythonExecutionResult::failure(format!("Compile error: {:?}", e))),
        };

        match vm.run_code_obj(code_obj, scope.clone()) {
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
                // Format the error as a string using Debug
                let error_msg = format!("{:?}", e);
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

    #[test]
    fn test_compile_error() {
        let executor = PythonExecutor::new();

        let code = r#"
# Invalid Python syntax
if True
    x = 1
"#;

        let result = executor.execute(code, &HashMap::new());
        assert!(!result.success, "Execution should fail");
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("Compile error"));
    }

    #[test]
    fn test_runtime_error() {
        let executor = PythonExecutor::new();

        let code = r#"
# This will cause a runtime error
x = undefined_variable
"#;

        let result = executor.execute(code, &HashMap::new());
        assert!(!result.success, "Execution should fail");
        assert!(result.error.is_some());
    }

    #[test]
    fn test_getvar_nonexistent() {
        let executor = PythonExecutor::new();

        let code = r#"
# Getting non-existent variable should return None
val = d.getVar("NONEXISTENT")
if val is None:
    d.setVar("RESULT", "was_none")
else:
    d.setVar("RESULT", "was_not_none")
"#;

        let result = executor.execute(code, &HashMap::new());
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("RESULT"), Some(&"was_none".to_string()));
    }

    #[test]
    fn test_getvar_no_expand() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("BASE".to_string(), "/usr".to_string());

        let code = r#"
d.setVar("PATH", "${BASE}/bin")
# Get without expansion
unexpanded = d.getVar("PATH", False)
d.setVar("UNEXPANDED", unexpanded)
# Get with expansion (default)
expanded = d.getVar("PATH")
d.setVar("EXPANDED", expanded)
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("UNEXPANDED"), Some(&"${BASE}/bin".to_string()));
        assert_eq!(result.variables_set.get("EXPANDED"), Some(&"/usr/bin".to_string()));
    }

    #[test]
    fn test_getvar_explicit_expand() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("PREFIX".to_string(), "/opt".to_string());

        let code = r#"
d.setVar("INSTALL_DIR", "${PREFIX}/myapp")
expanded = d.getVar("INSTALL_DIR", True)
d.setVar("RESULT", expanded)
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("RESULT"), Some(&"/opt/myapp".to_string()));
    }

    #[test]
    fn test_multiple_variable_operations() {
        let executor = PythonExecutor::new();

        let code = r#"
# Test multiple operations on same variable
d.setVar("FLAGS", "a")
d.appendVar("FLAGS", " b")
d.appendVar("FLAGS", " c")
d.prependVar("FLAGS", "0 ")
"#;

        let result = executor.execute(code, &HashMap::new());
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("FLAGS"), Some(&"0 a b c".to_string()));
    }

    #[test]
    fn test_nested_variable_expansion() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("BASE".to_string(), "/usr".to_string());
        initial.insert("SUBDIR".to_string(), "local".to_string());

        let code = r#"
d.setVar("PATH1", "${BASE}/${SUBDIR}")
d.setVar("PATH2", "${PATH1}/bin")
result = d.getVar("PATH2")
d.setVar("FINAL", result)
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("FINAL"), Some(&"/usr/local/bin".to_string()));
    }

    #[test]
    fn test_empty_code() {
        let executor = PythonExecutor::new();

        let result = executor.execute("", &HashMap::new());
        assert!(result.success, "Empty code should succeed");
        assert!(result.variables_set.is_empty());
    }

    #[test]
    fn test_default_trait() {
        let executor1 = PythonExecutor::new();
        let executor2 = PythonExecutor::default();

        let code = "d.setVar('TEST', 'value')";

        let result1 = executor1.execute(code, &HashMap::new());
        let result2 = executor2.execute(code, &HashMap::new());

        assert!(result1.success);
        assert!(result2.success);
        assert_eq!(result1.variables_set.get("TEST"), result2.variables_set.get("TEST"));
    }

    #[test]
    fn test_variable_tracking() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("VAR1".to_string(), "val1".to_string());
        initial.insert("VAR2".to_string(), "val2".to_string());
        initial.insert("VAR3".to_string(), "val3".to_string());

        let code = r#"
# Read some variables
v1 = d.getVar("VAR1")
v2 = d.getVar("VAR2")
v3 = d.getVar("VAR3")
# Set some variables
d.setVar("OUT1", v1)
d.setVar("OUT2", v2)
# Append and prepend
d.appendVar("VAR3", " extra")
d.prependVar("VAR3", "prefix ")
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert!(result.variables_read.contains(&"VAR1".to_string()));
        assert!(result.variables_read.contains(&"VAR2".to_string()));
        assert!(result.variables_read.contains(&"VAR3".to_string()));
        // OUT1, OUT2, VAR3 (from setVar), VAR3 (from appendVar), VAR3 (from prependVar)
        // But since they're the same key, should be 3 unique keys in the map
        assert!(result.variables_set.contains_key("OUT1"));
        assert!(result.variables_set.contains_key("OUT2"));
        assert!(result.variables_set.contains_key("VAR3"));
    }

    #[test]
    fn test_python_stdlib_available() {
        let executor = PythonExecutor::new();

        let code = r#"
import os
import sys
# Test that stdlib is available
d.setVar("HAS_OS", "yes" if hasattr(os, 'path') else "no")
d.setVar("HAS_SYS", "yes" if hasattr(sys, 'version') else "no")
"#;

        let result = executor.execute(code, &HashMap::new());
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("HAS_OS"), Some(&"yes".to_string()));
        assert_eq!(result.variables_set.get("HAS_SYS"), Some(&"yes".to_string()));
    }

    #[test]
    fn test_bb_utils_contains() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("DISTRO_FEATURES".to_string(), "systemd pam usrmerge".to_string());

        let code = r#"
# Test bb.utils.contains - returns True if systemd in DISTRO_FEATURES
# bb object is already available in global scope
result = bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d)
if result:
    d.appendVar('RDEPENDS', ' systemd')
else:
    d.appendVar('RDEPENDS', ' sysvinit')
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("RDEPENDS"), Some(&" systemd".to_string()));
    }

    #[test]
    fn test_bb_utils_contains_not_found() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("DISTRO_FEATURES".to_string(), "pam usrmerge".to_string());

        let code = r#"
# Test bb.utils.contains - returns False if systemd not in DISTRO_FEATURES
# bb object is already available in global scope
result = bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d)
if result:
    d.appendVar('RDEPENDS', ' systemd')
else:
    d.appendVar('RDEPENDS', ' sysvinit')
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("RDEPENDS"), Some(&" sysvinit".to_string()));
    }

    #[test]
    fn test_complex_python_with_in_operator() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("PACKAGECONFIG".to_string(), "feature1 feature2".to_string());

        let code = r#"
# Test 'in' operator with getVar
pkgconfig = d.getVar('PACKAGECONFIG', True) or ''
if 'feature1' in pkgconfig:
    d.appendVar('DEPENDS', ' feature1-lib')
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("DEPENDS"), Some(&" feature1-lib".to_string()));
    }

    #[test]
    fn test_complex_python_with_startswith() {
        let executor = PythonExecutor::new();

        let mut initial = HashMap::new();
        initial.insert("MACHINE".to_string(), "qemux86-64".to_string());

        let code = r#"
# Test string method .startswith()
machine = d.getVar('MACHINE', True)
if machine and machine.startswith('qemu'):
    d.appendVar('RDEPENDS', ' qemu-helper')
"#;

        let result = executor.execute(code, &initial);
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("RDEPENDS"), Some(&" qemu-helper".to_string()));
    }

    #[test]
    fn test_dedent() {
        // Test dedenting code with common leading whitespace
        let indented_code = "    x = 1\n    y = 2\n    z = x + y";
        let expected = "x = 1\ny = 2\nz = x + y";
        assert_eq!(PythonExecutor::dedent(indented_code), expected);

        // Test with mixed indentation
        let mixed_code = "    line1\n        line2\n    line3";
        let expected_mixed = "line1\n    line2\nline3";
        assert_eq!(PythonExecutor::dedent(mixed_code), expected_mixed);

        // Test with empty lines
        let with_empty = "    x = 1\n\n    y = 2";
        let expected_empty = "x = 1\n\ny = 2";
        assert_eq!(PythonExecutor::dedent(with_empty), expected_empty);
    }

    #[test]
    fn test_execute_indented_code() {
        // Test that executor can handle indented code (like from BitBake recipes)
        let executor = PythonExecutor::new();

        let indented_code = "    x = 1 + 1\n    d.setVar('RESULT', str(x))";

        let result = executor.execute(indented_code, &HashMap::new());
        assert!(result.success, "Execution should succeed: {:?}", result.error);
        assert_eq!(result.variables_set.get("RESULT"), Some(&"2".to_string()));
    }
}
