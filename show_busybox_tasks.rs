#!/usr/bin/env rust-script
//! Extract and show first 3 tasks from busybox recipe

use std::path::PathBuf;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recipe_path = PathBuf::from("/home/user/graph-git-rs/build/repos/poky/meta/recipes-core/busybox/busybox.inc");

    println!("Reading busybox recipe: {}\n", recipe_path.display());

    let content = fs::read_to_string(&recipe_path)?;

    // Find shell tasks
    println!("=== SHELL TASKS ===\n");

    let tasks = vec![
        ("do_prepare_config", 112, 134),
        ("do_configure", 136, 145),
        ("do_compile", 147, 209),
    ];

    for (task_name, start, end) in tasks {
        println!("╔═══════════════════════════════════════════════════════════╗");
        println!("║ Task: {:48} ║", task_name);
        println!("╠═══════════════════════════════════════════════════════════╣");

        let lines: Vec<&str> = content.lines().collect();
        for (idx, line) in lines[start-1..end].iter().enumerate() {
            println!("{:4} │ {}", start + idx, line);
        }
        println!("╚═══════════════════════════════════════════════════════════╝\n");
    }

    Ok(())
}
