// Example: Extract task dependencies from a BitBake recipe

use convenient_bitbake::{parse_addtask_statement, parse_task_flag, Task, TaskCollection};

fn main() {
    println!("=== BitBake Task Dependency Extraction Example ===\n");

    // Example BitBake recipe content
    let recipe_content = r#"
SUMMARY = "Example recipe with tasks"
LICENSE = "MIT"

inherit autotools

# Standard tasks
addtask fetch
addtask unpack after fetch
addtask patch after unpack
addtask configure after patch before compile
addtask compile after configure before install
addtask install after compile

# Task dependencies
do_compile[depends] = "virtual/libc:do_populate_sysroot openssl:do_populate_sysroot"
do_compile[rdepends] = "bash:do_populate_sysroot"
do_install[depends] = "virtual/fakeroot-native:do_populate_sysroot"

# Custom task
addtask custom_check after compile before install
do_custom_check[depends] = "python3-native:do_populate_sysroot"
"#;

    println!("Recipe content:\n{}\n", recipe_content);

    // Parse tasks from recipe
    let mut collection = TaskCollection::new();

    for line in recipe_content.lines() {
        let line = line.trim();

        // Parse addtask statements
        if let Some(task) = parse_addtask_statement(line) {
            println!("Found task: {}", task.name);
            if !task.after.is_empty() {
                println!("  After: {}", task.after.join(", "));
            }
            if !task.before.is_empty() {
                println!("  Before: {}", task.before.join(", "));
            }
            collection.add_task(task);
        }

        // Parse task flags
        if let Some((task_name, flag_name, value)) = parse_task_flag(line) {
            println!("Found task flag: {}[{}] = {}", task_name, flag_name, value);
            if let Some(task) = collection.tasks.get_mut(&task_name) {
                task.flags.insert(flag_name, value);
            } else {
                // Task not yet added, create it
                let mut task = Task::new(task_name);
                task.flags.insert(flag_name, value);
                collection.add_task(task);
            }
        }
    }

    println!("\n=== Task Analysis ===\n");

    // Compute task order
    match collection.compute_task_order() {
        Ok(()) => {
            println!("✓ Task execution order:");
            for (idx, task_name) in collection.task_order.iter().enumerate() {
                println!("  {}. {}", idx + 1, task_name);
            }
        }
        Err(e) => {
            println!("✗ Error computing task order: {}", e);
        }
    }

    // Show task dependencies
    println!("\n=== Task Dependencies ===\n");

    for task_name in collection.task_names() {
        let build_deps = collection.get_build_dependencies(task_name);
        let runtime_deps = collection.get_runtime_dependencies(task_name);

        if !build_deps.is_empty() || !runtime_deps.is_empty() {
            println!("Task: {}", task_name);

            if !build_deps.is_empty() {
                println!("  Build-time dependencies:");
                for dep in build_deps {
                    match dep {
                        convenient_bitbake::TaskDependency::BuildTime { recipe, task } => {
                            println!("    - {}:{}", recipe, task);
                        }
                        convenient_bitbake::TaskDependency::Recipe(name) => {
                            println!("    - {} (recipe-level)", name);
                        }
                        _ => {}
                    }
                }
            }

            if !runtime_deps.is_empty() {
                println!("  Runtime dependencies:");
                for dep in runtime_deps {
                    match dep {
                        convenient_bitbake::TaskDependency::Runtime { recipe, task } => {
                            println!("    - {}:{}", recipe, task);
                        }
                        convenient_bitbake::TaskDependency::Recipe(name) => {
                            println!("    - {} (recipe-level)", name);
                        }
                        _ => {}
                    }
                }
            }
            println!();
        }
    }

    // Show task graph structure
    println!("=== Task Graph (Graphviz DOT format) ===\n");
    println!("digraph tasks {{");
    println!("  rankdir=LR;");
    println!("  node [shape=box];");
    println!();

    for task_name in collection.task_names() {
        if let Some(task) = collection.get_task(task_name) {
            for after_task in &task.after {
                println!("  \"{}\" -> \"{}\";", after_task, task_name);
            }
        }
    }

    println!("}}");

    println!("\n=== Summary ===");
    println!("Total tasks: {}", collection.tasks.len());
    println!("Task order computed: ✓");
}
