// Extract first 3 tasks from busybox recipe

use convenient_bitbake::TaskExtractor;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Path to busybox recipe
    let recipe_path = PathBuf::from("/home/user/graph-git-rs/build/repos/poky/meta/recipes-core/busybox/busybox.inc");

    println!("🔍 Extracting tasks from: {}", recipe_path.display());
    println!();

    // Read recipe content
    let content = std::fs::read_to_string(&recipe_path)?;

    // Create task extractor
    let extractor = TaskExtractor::new();

    // Extract tasks
    let tasks = extractor.extract_from_content(&content);

    println!("📊 Found {} tasks\n", tasks.len());

    // Show first 3 tasks
    for (i, (_name, task)) in tasks.iter().take(3).enumerate() {
        println!("═══════════════════════════════════════════════════════════");
        println!("Task {}: {}", i + 1, task.name);
        println!("═══════════════════════════════════════════════════════════");
        println!("Type: {:?}", task.impl_type);
        println!("Line: {}", task.line_number);
        if let Some(ref suffix) = task.override_suffix {
            println!("Override: {}", suffix);
        }
        println!();
        println!("Implementation:");
        println!("-----------------------------------------------------------");

        // Show first 30 lines of implementation
        let lines: Vec<&str> = task.code.lines().collect();
        for (idx, line) in lines.iter().take(30).enumerate() {
            println!("{:4} | {}", idx + 1, line);
        }

        if lines.len() > 30 {
            println!("     | ... ({} more lines)", lines.len() - 30);
        }

        println!();
    }

    Ok(())
}
