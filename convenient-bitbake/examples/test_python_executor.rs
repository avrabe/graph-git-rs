// Test Python executor manually
use convenient_bitbake::{PythonExecutor};
use std::collections::HashMap;

fn main() {
    println!("Creating Python executor...");
    let executor = PythonExecutor::new();

    println!("Testing simple Python code...");
    let code = r#"
x = 1 + 1
print("Hello from Python!")
"#;

    let result = executor.execute(code, &HashMap::new());
    println!("Result: {:?}", result);

    if !result.success {
        println!("ERROR: {}", result.error.unwrap_or_default());
    }

    println!("\nTesting DataStore...");
    let code2 = r#"
d.setVar("TEST", "value")
"#;

    let result2 = executor.execute(code2, &HashMap::new());
    println!("Result2: {:?}", result2);

    if !result2.success {
        println!("ERROR: {}", result2.error.unwrap_or_default());
    } else {
        println!("Variables set: {:?}", result2.variables_set);
    }
}
