// Test Python executor manually
// Note: PythonExecutor is not publicly exported

fn main() {
    println!("PythonExecutor is not publicly exported from the convenient_bitbake crate.");
    println!("Python execution is handled internally by the RecipeExtractor.");
    println!("\nTo test Python functionality, use the ExtractionConfig:");
    println!("  config.use_python_executor = true;");
    println!("\nOr use the simple Python evaluator:");
    println!("  config.use_simple_python_eval = true;");
}
