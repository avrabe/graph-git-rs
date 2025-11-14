// Extract first 3 tasks from busybox recipe

use convenient_bitbake::{TaskExtractor, ExtractionConfig};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Path to busybox recipe
    let recipe_path = PathBuf::from("/home/user/graph-git-rs/build/repos/poky/meta/recipes-core/busybox/busybox.inc");

    println!("🔍 Extracting tasks from: {}", recipe_path.display());
    println!();

    // Read recipe content
    let content = std::fs::read_to_string(&recipe_path)?;

    // Create task extractor
    let config = ExtractionConfig::default();
    let extractor = TaskExtractor::new(config);

    // Extract tasks
    let tasks = extractor.extract_tasks(&content)?;

    println!("📊 Found {} tasks\n", tasks.len());

    // Show first 3 tasks
    for (i, task) in tasks.iter().take(3).enumerate() {
        println!("═══════════════════════════════════════════════════════════");
        println!("Task {}: {}", i + 1, task.name);
        println!("═══════════════════════════════════════════════════════════");
        println!("Language: {:?}", task.language);
        println!("Lines: {}-{}", task.start_line, task.end_line);
        println!();
        println!("Implementation:");
        println!("-----------------------------------------------------------");

        // Show first 30 lines of implementation
        let lines: Vec<&str> = task.implementation.lines().collect();
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
