// Example: Flat recipe graph with ID-based references
// Demonstrates modern compiler IR design pattern

use convenient_bitbake::RecipeGraph;

fn main() {
    println!("=== BitBake Recipe Graph Demo ===\n");
    println!("Using flat structure with ID-based references (rustc/LLVM style)\n");

    let mut graph = RecipeGraph::new();

    // === Build a realistic dependency graph ===

    println!("--- Building Dependency Graph ---\n");

    // Core system libraries
    let glibc = graph.add_recipe("glibc");
    graph.get_recipe_mut(glibc).unwrap().version = Some("2.35".to_string());
    println!("Added: glibc-2.35 (id: {:?})", glibc);

    let gcc = graph.add_recipe("gcc");
    graph.get_recipe_mut(gcc).unwrap().version = Some("12.2".to_string());
    println!("Added: gcc-12.2 (id: {:?})", gcc);

    // Utilities
    let bash = graph.add_recipe("bash");
    graph.add_dependency(bash, glibc);
    println!("Added: bash depends on glibc");

    let coreutils = graph.add_recipe("coreutils");
    graph.add_dependency(coreutils, glibc);
    println!("Added: coreutils depends on glibc");

    // Libraries
    let openssl = graph.add_recipe("openssl");
    graph.add_dependency(openssl, glibc);
    println!("Added: openssl depends on glibc");

    let zlib = graph.add_recipe("zlib");
    graph.add_dependency(zlib, glibc);
    println!("Added: zlib depends on glibc");

    // Application
    let curl = graph.add_recipe("curl");
    graph.add_dependency(curl, openssl);
    graph.add_dependency(curl, zlib);
    println!("Added: curl depends on openssl, zlib");

    // Kernel (virtual provider)
    let linux_yocto = graph.add_recipe("linux-yocto");
    graph.get_recipe_mut(linux_yocto).unwrap().version = Some("5.15".to_string());
    graph.get_recipe_mut(linux_yocto).unwrap().provides.push("virtual/kernel".to_string());
    graph.register_provider(linux_yocto, "virtual/kernel");
    println!("Added: linux-yocto-5.15 provides virtual/kernel");

    // Add some tasks
    let compile_task = graph.add_task(curl, "do_compile");
    let install_task = graph.add_task(curl, "do_install");
    println!("\nAdded tasks to curl: do_compile, do_install");

    println!("\n--- Graph Statistics ---\n");
    let stats = graph.statistics();
    println!("Recipes: {}", stats.recipe_count);
    println!("Tasks: {}", stats.task_count);
    println!("Total dependencies: {}", stats.total_dependencies);
    println!("Providers: {}", stats.provider_count);
    println!("Max dependency depth: {}", stats.max_dependency_depth);

    println!("\n--- Provider Resolution ---\n");
    if let Some(kernel_id) = graph.resolve_provider("virtual/kernel") {
        let kernel = graph.get_recipe(kernel_id).unwrap();
        println!("virtual/kernel resolves to: {}", kernel.full_name());
    }

    if let Some(glibc_id) = graph.resolve_provider("glibc") {
        let glibc_recipe = graph.get_recipe(glibc_id).unwrap();
        println!("glibc resolves to: {}", glibc_recipe.full_name());
    }

    println!("\n--- Dependency Analysis ---\n");

    // Get direct dependencies
    let deps = graph.get_dependencies(curl);
    println!("curl direct dependencies:");
    for dep_id in deps {
        let dep = graph.get_recipe(dep_id).unwrap();
        println!("  - {}", dep.name);
    }

    // Get all transitive dependencies
    let all_deps = graph.get_all_dependencies(curl);
    println!("\ncurl transitive dependencies:");
    for dep_id in all_deps {
        let dep = graph.get_recipe(dep_id).unwrap();
        println!("  - {}", dep.name);
    }

    // Get dependents (reverse lookup)
    let dependents = graph.get_dependents(glibc);
    println!("\nRecipes that depend on glibc:");
    for dep_id in dependents {
        let dep = graph.get_recipe(dep_id).unwrap();
        println!("  - {}", dep.name);
    }

    println!("\n--- Build Order (Topological Sort) ---\n");
    match graph.topological_sort() {
        Ok(sorted) => {
            println!("Optimal build order:");
            for (idx, recipe_id) in sorted.iter().enumerate() {
                let recipe = graph.get_recipe(*recipe_id).unwrap();
                println!("  {}. {}", idx + 1, recipe.full_name());
            }
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }

    println!("\n--- Cycle Detection ---\n");
    let cycles = graph.detect_cycles();
    if cycles.is_empty() {
        println!("✓ No circular dependencies detected");
    } else {
        println!("✗ Found {} circular dependencies", cycles.len());
        for cycle in cycles {
            println!("  Cycle: {:?}", cycle);
        }
    }

    println!("\n--- Task Information ---\n");
    let curl_tasks = graph.get_recipe_tasks(curl);
    println!("Tasks for curl:");
    for task in curl_tasks {
        println!("  - {} (id: {:?})", task.name, task.id);
    }

    // Demonstrate finding a task
    if let Some(task_id) = graph.find_task(curl, "do_compile") {
        let task = graph.get_task(task_id).unwrap();
        println!("\nFound task by name: {} (id: {:?})", task.name, task.id);
    }

    println!("\n--- Benefits of Flat Structure ---\n");
    println!("✓ IDs are cheap to copy and compare");
    println!("✓ No ownership issues - recipes can be referenced from multiple places");
    println!("✓ Circular dependencies are representable");
    println!("✓ Easy to serialize/deserialize");
    println!("✓ Perfect for graph databases (Neo4j)");
    println!("✓ Efficient queries and traversals");
    println!("✓ Follows rustc/LLVM/rust-analyzer design patterns");

    println!("\n--- Graphviz Export ---\n");
    let dot = graph.to_dot();
    println!("{}",  &dot[..dot.len().min(500)]);
    if dot.len() > 500 {
        println!("... (truncated)");
    }

    println!("\n--- Memory Efficiency ---\n");
    println!("RecipeId size: {} bytes", std::mem::size_of::<convenient_bitbake::RecipeId>());
    println!("TaskId size: {} bytes", std::mem::size_of::<convenient_bitbake::TaskId>());
    println!("(Compare to Box<Recipe> size: {} bytes)", std::mem::size_of::<Box<u8>>());
    println!("\n✓ IDs are just u32 wrappers - extremely lightweight!");
}
