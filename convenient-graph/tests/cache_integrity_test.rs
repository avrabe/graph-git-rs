//! Cache integrity and incremental build simulation test
//!
//! This test demonstrates the cache invalidation strategy for Bitzel:
//! 1. Content-based hashing for cache keys
//! 2. Automatic invalidation when dependencies change
//! 3. Cache hit detection for unchanged nodes
//! 4. Incremental build support

use convenient_graph::DAG;
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct Task {
    name: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
    command: String,
}

impl Task {
    fn new(name: &str, inputs: Vec<&str>, outputs: Vec<&str>, command: &str) -> Self {
        Self {
            name: name.to_string(),
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
            outputs: outputs.iter().map(|s| s.to_string()).collect(),
            command: command.to_string(),
        }
    }
}

/// Simulates a content-addressable cache
struct BuildCache {
    cache: HashMap<String, CacheEntry>,
    cache_hits: usize,
    total_lookups: usize,
}

#[derive(Clone)]
struct CacheEntry {
    outputs: Vec<String>,
}

impl BuildCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
            cache_hits: 0,
            total_lookups: 0,
        }
    }

    fn get(&self, hash: &str) -> Option<&CacheEntry> {
        self.cache.get(hash)
    }

    fn put(&mut self, hash: String, entry: CacheEntry) {
        let _ = self.cache.insert(hash, entry);
    }

    fn hit_rate(&self) -> f64 {
        if self.total_lookups == 0 {
            return 0.0;
        }
        self.cache_hits as f64 / self.total_lookups as f64
    }
}

fn execute_build(dag: &DAG<Task, ()>, cache: &mut BuildCache) -> Result<Vec<String>, String> {
    // Get topological order
    let order = dag.topological_sort().map_err(|e| e.to_string())?;

    let mut executed_tasks = Vec::new();

    for node_id in order {
        let task = dag.node(node_id).map_err(|e| e.to_string())?;

        // Calculate content hash including all dependencies
        let content_hash = dag.content_hash(node_id).map_err(|e| e.to_string())?;

        // Check cache
        cache.total_lookups += 1;
        if cache.get(&content_hash).is_some() {
            cache.cache_hits += 1;
            println!("✓ Cache HIT for task '{}' (hash: {}...)", task.name, &content_hash[..8]);
            // Cache hit - reuse outputs
            continue;
        }

        // Cache miss - execute task
        println!("⚡ Executing task '{}' (hash: {}...)", task.name, &content_hash[..8]);
        executed_tasks.push(task.name.clone());

        // Simulate task execution and store in cache
        let entry = CacheEntry {
            outputs: task.outputs.clone(),
        };
        cache.put(content_hash, entry);
    }

    Ok(executed_tasks)
}

#[test]
fn test_cache_incremental_build() {
    println!("\n=== Test: Incremental Build with Cache ===\n");

    // Build 1: Full build from scratch
    println!("Build 1: Full build from scratch");
    let mut dag1 = DAG::<Task, ()>::new();
    let mut cache = BuildCache::new();

    // Create a simple build graph: compile -> link -> package
    let compile = dag1.add_node(Task::new(
        "compile",
        vec!["main.c", "utils.c"],
        vec!["main.o", "utils.o"],
        "gcc -c main.c utils.c",
    ));

    let link = dag1.add_node(Task::new(
        "link",
        vec!["main.o", "utils.o"],
        vec!["app"],
        "gcc -o app main.o utils.o",
    ));

    let package = dag1.add_node(Task::new(
        "package",
        vec!["app", "README.md"],
        vec!["app.tar.gz"],
        "tar czf app.tar.gz app README.md",
    ));

    dag1.add_edge(compile, link, ()).unwrap();
    dag1.add_edge(link, package, ()).unwrap();

    let executed1 = execute_build(&dag1, &mut cache).unwrap();
    println!("\nBuild 1 executed {} tasks: {:?}", executed1.len(), executed1);
    assert_eq!(executed1.len(), 3); // All tasks executed
    assert_eq!(cache.hit_rate(), 0.0); // No cache hits

    // Build 2: Rebuild without changes - should use cache
    println!("\n\nBuild 2: Rebuild without changes (expect cache hits)");
    let executed2 = execute_build(&dag1, &mut cache).unwrap();
    println!("\nBuild 2 executed {} tasks: {:?}", executed2.len(), executed2);
    assert_eq!(executed2.len(), 0); // No tasks executed
    assert!(cache.hit_rate() > 0.0); // Some cache hits

    // Build 3: Modify source file - only affected tasks should rebuild
    println!("\n\nBuild 3: Modify source file (expect partial rebuild)");
    let mut dag3 = DAG::<Task, ()>::new();

    // Same graph but with modified compile task (simulating changed source)
    let compile3 = dag3.add_node(Task::new(
        "compile",
        vec!["main.c", "utils.c", "MODIFIED"],  // Simulated change
        vec!["main.o", "utils.o"],
        "gcc -c main.c utils.c",
    ));

    let link3 = dag3.add_node(Task::new(
        "link",
        vec!["main.o", "utils.o"],
        vec!["app"],
        "gcc -o app main.o utils.o",
    ));

    let package3 = dag3.add_node(Task::new(
        "package",
        vec!["app", "README.md"],
        vec!["app.tar.gz"],
        "tar czf app.tar.gz app README.md",
    ));

    dag3.add_edge(compile3, link3, ()).unwrap();
    dag3.add_edge(link3, package3, ()).unwrap();

    let executed3 = execute_build(&dag3, &mut cache).unwrap();
    println!("\nBuild 3 executed {} tasks: {:?}", executed3.len(), executed3);
    assert_eq!(executed3.len(), 3); // Compile + downstream tasks
    assert_eq!(executed3, vec!["compile", "link", "package"]);

    println!("\n=== Test Passed: Cache correctly invalidated affected tasks ===\n");
}

#[test]
fn test_cache_parallel_builds() {
    println!("\n=== Test: Parallel Build Paths with Shared Cache ===\n");

    let mut dag = DAG::<Task, ()>::new();
    let mut cache = BuildCache::new();

    // Create a diamond dependency graph
    //     root
    //    /    \
    //   a      b
    //    \    /
    //     final

    let root = dag.add_node(Task::new(
        "root",
        vec!["input.txt"],
        vec!["root.out"],
        "process input.txt",
    ));

    let a = dag.add_node(Task::new(
        "task_a",
        vec!["root.out"],
        vec!["a.out"],
        "transform root.out",
    ));

    let b = dag.add_node(Task::new(
        "task_b",
        vec!["root.out"],
        vec!["b.out"],
        "transform root.out",
    ));

    let final_task = dag.add_node(Task::new(
        "final",
        vec!["a.out", "b.out"],
        vec!["final.out"],
        "merge a.out b.out",
    ));

    dag.add_edge(root, a, ()).unwrap();
    dag.add_edge(root, b, ()).unwrap();
    dag.add_edge(a, final_task, ()).unwrap();
    dag.add_edge(b, final_task, ()).unwrap();

    // Build 1: Full build
    println!("Build 1: Full build");
    let executed1 = execute_build(&dag, &mut cache).unwrap();
    assert_eq!(executed1.len(), 4);

    // Build 2: Rebuild - all cached
    println!("\nBuild 2: Rebuild (all cached)");
    let executed2 = execute_build(&dag, &mut cache).unwrap();
    assert_eq!(executed2.len(), 0);

    // Build 3: Modify only task_a's input
    println!("\nBuild 3: Modify one path");
    let mut dag3 = DAG::<Task, ()>::new();

    let root3 = dag3.add_node(Task::new(
        "root",
        vec!["input.txt"],
        vec!["root.out"],
        "process input.txt",
    ));

    let a3 = dag3.add_node(Task::new(
        "task_a",
        vec!["root.out", "MODIFIED"],  // Simulated change
        vec!["a.out"],
        "transform root.out",
    ));

    let b3 = dag3.add_node(Task::new(
        "task_b",
        vec!["root.out"],
        vec!["b.out"],
        "transform root.out",
    ));

    let final3 = dag3.add_node(Task::new(
        "final",
        vec!["a.out", "b.out"],
        vec!["final.out"],
        "merge a.out b.out",
    ));

    dag3.add_edge(root3, a3, ()).unwrap();
    dag3.add_edge(root3, b3, ()).unwrap();
    dag3.add_edge(a3, final3, ()).unwrap();
    dag3.add_edge(b3, final3, ()).unwrap();

    let executed3 = execute_build(&dag3, &mut cache).unwrap();
    println!("\nBuild 3 executed: {:?}", executed3);

    // Only task_a and final should rebuild (root and task_b cached)
    assert!(executed3.contains(&"task_a".to_string()));
    assert!(executed3.contains(&"final".to_string()));
    assert!(!executed3.contains(&"root".to_string())); // Should be cached
    assert!(!executed3.contains(&"task_b".to_string())); // Should be cached

    println!("\n=== Test Passed: Parallel paths correctly share cache ===\n");
}

#[test]
fn test_cache_deep_dependency_chain() {
    println!("\n=== Test: Deep Dependency Chain Cache Invalidation ===\n");

    let mut dag = DAG::<Task, ()>::new();
    let mut cache = BuildCache::new();

    // Create a chain: a -> b -> c -> d -> e
    let mut tasks = Vec::new();
    for i in 0..5 {
        let name = format!("task_{}", (b'a' + i as u8) as char);
        let input = if i == 0 {
            format!("input_{}.txt", i)
        } else {
            format!("output_{}.txt", i - 1)
        };
        let output = format!("output_{}.txt", i);

        let task = Task::new(
            &name,
            vec![&input],
            vec![&output],
            &format!("process {} -> {}", input, output),
        );
        tasks.push(dag.add_node(task));
    }

    // Connect chain
    for i in 0..4 {
        dag.add_edge(tasks[i], tasks[i + 1], ()).unwrap();
    }

    // Build 1: Full build
    println!("Build 1: Full build of 5-task chain");
    let executed1 = execute_build(&dag, &mut cache).unwrap();
    assert_eq!(executed1.len(), 5);

    // Build 2: Modify middle task
    println!("\nBuild 2: Modify task_c (middle of chain)");
    let mut dag2 = DAG::<Task, ()>::new();

    let task_a = dag2.add_node(Task::new(
        "task_a",
        vec!["input_0.txt"],
        vec!["output_0.txt"],
        "process input_0.txt -> output_0.txt",
    ));

    let task_b = dag2.add_node(Task::new(
        "task_b",
        vec!["output_0.txt"],
        vec!["output_1.txt"],
        "process output_0.txt -> output_1.txt",
    ));

    let task_c = dag2.add_node(Task::new(
        "task_c",
        vec!["output_1.txt", "MODIFIED"],  // Changed
        vec!["output_2.txt"],
        "process output_1.txt -> output_2.txt",
    ));

    let task_d = dag2.add_node(Task::new(
        "task_d",
        vec!["output_2.txt"],
        vec!["output_3.txt"],
        "process output_2.txt -> output_3.txt",
    ));

    let task_e = dag2.add_node(Task::new(
        "task_e",
        vec!["output_3.txt"],
        vec!["output_4.txt"],
        "process output_3.txt -> output_4.txt",
    ));

    dag2.add_edge(task_a, task_b, ()).unwrap();
    dag2.add_edge(task_b, task_c, ()).unwrap();
    dag2.add_edge(task_c, task_d, ()).unwrap();
    dag2.add_edge(task_d, task_e, ()).unwrap();

    let executed2 = execute_build(&dag2, &mut cache).unwrap();
    println!("\nBuild 2 executed: {:?}", executed2);

    // task_a and task_b should be cached
    assert!(!executed2.contains(&"task_a".to_string()));
    assert!(!executed2.contains(&"task_b".to_string()));

    // task_c, task_d, task_e should rebuild
    assert!(executed2.contains(&"task_c".to_string()));
    assert!(executed2.contains(&"task_d".to_string()));
    assert!(executed2.contains(&"task_e".to_string()));

    println!("\n=== Test Passed: Deep chain correctly propagates changes ===\n");
}
