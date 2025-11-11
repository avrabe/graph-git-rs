use convenient_bitbake::PythonExecutor;
use std::collections::HashMap;

fn main() {
    println!("Testing Python executor with getVar...");
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
    println!("\nResult: success={}, error={:?}", result.success, result.error);
    println!("Variables set: {:?}", result.variables_set);
    println!("Variables read: {:?}", result.variables_read);
}
