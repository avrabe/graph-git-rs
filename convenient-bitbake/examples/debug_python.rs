// Note: PythonExecutor is not publicly exported

fn main() {
    println!("PythonExecutor is not publicly exported from the convenient_bitbake crate.");
    println!("Python execution is handled internally by the RecipeExtractor.");
    println!("\nTo test Python functionality with getVar/setVar:");
    println!("  1. Create a test recipe with Python code:");
    println!("     WORKDIR = \"${{@d.getVar('TMPDIR')}}/work\"");
    println!("  2. Configure the extractor:");
    println!("     config.use_python_executor = true;");
    println!("  3. Extract the recipe:");
    println!("     extractor.extract_from_content(&mut graph, name, content)");
}
