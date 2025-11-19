//! Integration test for parallel execution with priority-based scheduling

use convenient_bitbake::{
    AsyncTaskExecutor, ExecutionProgress, RecipeGraph, TaskExecutor, TaskGraph,
    TaskGraphBuilder, TaskScheduler, TaskSpec,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test]
async fn test_parallel_execution_with_scheduler() {
    // Create temporary directory for test
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("cache");
    let work_dir = tmp.path().join("work");
    std::fs::create_dir_all(&work_dir).unwrap();

    // Build a simple recipe graph
    let mut recipe_graph = RecipeGraph::new();

    // Create two recipes with dependencies
    let recipe_a = recipe_graph.add_recipe("recipe-a");
    let recipe_b = recipe_graph.add_recipe("recipe-b");

    // Add dependency: recipe-b depends on recipe-a
    recipe_graph.add_dependency(recipe_b, recipe_a);

    // Add tasks to recipe-a
    let a_fetch = recipe_graph.add_task(recipe_a, "do_fetch");
    let a_compile = recipe_graph.add_task(recipe_a, "do_compile");
    let a_install = recipe_graph.add_task(recipe_a, "do_install");

    // Set up task ordering within recipe-a
    if let Some(task) = recipe_graph.get_task_mut(a_compile) {
        task.after.push(a_fetch);
    }
    if let Some(task) = recipe_graph.get_task_mut(a_install) {
        task.after.push(a_compile);
    }

    // Add tasks to recipe-b
    let b_fetch = recipe_graph.add_task(recipe_b, "do_fetch");
    let b_compile = recipe_graph.add_task(recipe_b, "do_compile");
    let b_install = recipe_graph.add_task(recipe_b, "do_install");

    // Set up task ordering within recipe-b
    if let Some(task) = recipe_graph.get_task_mut(b_compile) {
        task.after.push(b_fetch);
    }
    if let Some(task) = recipe_graph.get_task_mut(b_install) {
        task.after.push(b_compile);
    }

    // Build task execution graph
    let builder = TaskGraphBuilder::new(recipe_graph.clone());
    let task_graph = builder.build_full_graph().unwrap();

    println!("Task graph built with {} tasks", task_graph.tasks.len());

    // Create task specifications
    let mut task_specs = HashMap::new();
    for (_task_id, task) in &task_graph.tasks {
        let task_key = format!("{}:{}", task.recipe_name, task.task_name);
        let task_workdir = work_dir.join(&task.recipe_name).join(&task.task_name);
        std::fs::create_dir_all(&task_workdir).unwrap();

        let spec = TaskSpec {
            name: task.task_name.clone(),
            recipe: task.recipe_name.clone(),
            script: format!(
                "#!/bin/bash\necho 'Executing {}:{}'\nsleep 0.1\ntouch done.txt\n",
                task.recipe_name, task.task_name
            ),
            workdir: task_workdir,
            env: HashMap::new(),
            outputs: vec![PathBuf::from("done.txt")],
            timeout: Some(Duration::from_secs(10)),
            execution_mode: convenient_bitbake::executor::ExecutionMode::Shell,
            network_policy: convenient_bitbake::executor::NetworkPolicy::Isolated,
            resource_limits: convenient_bitbake::executor::ResourceLimits::default(),
        };

        task_specs.insert(task_key, spec);
    }

    // Create executor and scheduler
    let executor = TaskExecutor::new(&cache_dir).unwrap();
    let async_executor = AsyncTaskExecutor::with_parallelism(executor, 2);
    let mut scheduler = TaskScheduler::new(recipe_graph);

    // Track progress updates
    let progress_updates = Arc::new(Mutex::new(Vec::new()));
    let progress_updates_clone = progress_updates.clone();

    let progress_callback = Box::new(move |progress: &ExecutionProgress| {
        progress_updates_clone
            .lock()
            .unwrap()
            .push(progress.clone());
        println!("{}", progress.format());
    });

    // Execute with scheduler
    let result = async_executor
        .execute_graph_with_scheduler(
            &task_graph,
            task_specs,
            &mut scheduler,
            Some(progress_callback),
        )
        .await;

    // Verify execution succeeded
    assert!(result.is_ok(), "Execution should succeed");

    let summary = result.unwrap();
    println!("\n{}", summary.format());

    // Verify all tasks completed
    assert_eq!(summary.total_tasks, 6, "Should have 6 tasks total");
    assert_eq!(summary.completed, 6, "All 6 tasks should complete");
    assert_eq!(summary.failed, 0, "No tasks should fail");

    // Verify we got progress updates
    let updates = progress_updates.lock().unwrap();
    assert!(
        !updates.is_empty(),
        "Should have received progress updates"
    );
    println!("\nReceived {} progress updates", updates.len());

    // Verify progress increased over time
    if updates.len() > 1 {
        let first_completed = updates.first().unwrap().completed;
        let last_completed = updates.last().unwrap().completed;
        assert!(
            last_completed >= first_completed,
            "Completion should increase or stay same"
        );
    }

    // Verify scheduler statistics
    let stats = scheduler.get_stats();
    assert_eq!(
        stats.completed, 6,
        "Scheduler should track 6 completed tasks"
    );
    println!(
        "\nScheduler stats: {}/{} completed ({:.1}%)",
        stats.completed,
        stats.total_tasks,
        stats.completion_percent()
    );
}

#[tokio::test]
async fn test_scheduler_critical_path_analysis() {
    // Create a more complex graph to test critical path analysis
    let mut recipe_graph = RecipeGraph::new();

    // Create a diamond dependency pattern
    //     A
    //    / \
    //   B   C
    //    \ /
    //     D

    let a = recipe_graph.add_recipe("a");
    let b = recipe_graph.add_recipe("b");
    let c = recipe_graph.add_recipe("c");
    let d = recipe_graph.add_recipe("d");

    recipe_graph.add_dependency(b, a);
    recipe_graph.add_dependency(c, a);
    recipe_graph.add_dependency(d, b);
    recipe_graph.add_dependency(d, c);

    // Add one task per recipe for simplicity
    recipe_graph.add_task(a, "do_compile");
    recipe_graph.add_task(b, "do_compile");
    recipe_graph.add_task(c, "do_compile");
    recipe_graph.add_task(d, "do_compile");

    // Create scheduler and analyze critical paths
    let mut scheduler = TaskScheduler::new(recipe_graph);
    scheduler.analyze_critical_paths();

    // Get statistics
    let stats = scheduler.get_stats();
    println!("Total tasks: {}", stats.total_tasks);

    // Get critical path
    let critical_path = scheduler.get_critical_path();
    println!("Critical path length: {}", critical_path.len());
    assert!(!critical_path.is_empty(), "Should have a critical path");

    // Estimate critical path time
    let estimated_time = scheduler.estimate_critical_path_time();
    println!("Estimated critical path time: {}ms", estimated_time);
    assert!(estimated_time > 0, "Critical path should have non-zero time");
}

#[test]
fn test_execution_progress_formatting() {
    let progress = ExecutionProgress {
        completed: 5,
        running: 2,
        pending: 3,
        total: 10,
        elapsed: Duration::from_secs(30),
        estimated_remaining: Duration::from_secs(30),
        completion_percent: 50.0,
        parallelism_utilization: 66.7,
    };

    let formatted = progress.format();
    assert!(formatted.contains("5/10"));
    assert!(formatted.contains("50.0%"));
    assert!(formatted.contains("Running: 2"));

    let rate = progress.completion_rate();
    assert!((rate - 0.1667).abs() < 0.001); // ~5 tasks / 30 seconds = 0.1667 tasks/sec
}

#[test]
fn test_execution_summary_statistics() {
    let summary = convenient_bitbake::executor::ExecutionSummary {
        total_tasks: 100,
        completed: 95,
        failed: 5,
        total_duration: Duration::from_secs(100),
        results: HashMap::new(),
    };

    assert_eq!(summary.success_rate(), 95.0);
    assert_eq!(summary.average_task_duration(), Duration::from_secs(1)); // ~100s / 95 tasks ≈ 1.05s

    let formatted = summary.format();
    assert!(formatted.contains("Total: 100 tasks"));
    assert!(formatted.contains("Completed: 95"));
    assert!(formatted.contains("Failed: 5"));
}
