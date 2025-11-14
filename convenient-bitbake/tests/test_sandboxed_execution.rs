//! Integration test for full sandboxed task execution
//!
//! This test demonstrates the complete pipeline:
//! 1. Generate task script
//! 2. Prepare sandbox workspace
//! 3. Execute in sandbox (with bubblewrap/sandbox-exec)
//! 4. Collect outputs
//! 5. Map errors and logs

use convenient_bitbake::executor::{SandboxManager, SandboxSpec};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_complete_sandboxed_task_execution() {
    // Initialize tracing to see logs
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    println!("\n🔧 Starting sandboxed task execution test\n");

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("cache");
    let sandbox_dir = cache_dir.join("sandboxes");

    println!("📁 Test directories:");
    println!("   Cache:   {}", cache_dir.display());
    println!("   Sandbox: {}", sandbox_dir.display());
    println!();

    // ========== Step 1: Generate Task Script ==========
    println!("📝 Step 1: Generating task script");

    let task_script = generate_bitbake_task_script(
        "example-recipe",
        "do_compile",
        r#"
# Simulate a typical compile task
echo "Starting compilation..."

# Ensure directories exist (important for Basic sandbox)
mkdir -p ${S} ${B} ${D}

cd ${S}
echo "Compiling source files..."
sleep 0.1

# Create build artifacts
echo "Build artifact from do_compile" > ${B}/compiled.o
echo "Compilation log output" > ${B}/compile.log

# Create output marker
echo "do_compile completed successfully" > ${D}/compile.done

echo "Compilation finished!"
"#,
    );

    println!("   Generated script:");
    println!("   ```bash");
    for (i, line) in task_script.lines().enumerate() {
        println!("   {:3} | {}", i + 1, line);
    }
    println!("   ```");
    println!();

    // ========== Step 2: Prepare Sandbox Workspace ==========
    println!("🏗️  Step 2: Preparing sandbox workspace");

    let mut spec = SandboxSpec::new(vec![task_script.clone()]);
    spec.cwd = PathBuf::from("/work");
    spec.rw_dirs.push(PathBuf::from("/work"));

    // Set BitBake environment variables
    spec.env.insert("PN".to_string(), "example-recipe".to_string());
    spec.env.insert("WORKDIR".to_string(), "/work".to_string());
    spec.env.insert("S".to_string(), "/work/src".to_string());
    spec.env.insert("B".to_string(), "/work/build".to_string());
    spec.env.insert("D".to_string(), "/work/outputs".to_string());

    // Specify outputs to collect
    spec.outputs.push(PathBuf::from("/work/outputs/compile.done"));
    spec.outputs.push(PathBuf::from("/work/build/compiled.o"));
    spec.outputs.push(PathBuf::from("/work/build/compile.log"));

    println!("   Environment:");
    for (key, value) in &spec.env {
        println!("     {}={}", key, value);
    }
    println!();
    println!("   Expected outputs:");
    for output in &spec.outputs {
        println!("     - {}", output.display());
    }
    println!();

    // ========== Step 3: Execute in Sandbox ==========
    println!("🚀 Step 3: Executing in sandbox");

    let manager = SandboxManager::new(&sandbox_dir).unwrap();
    let sandbox = manager.create_sandbox(spec).unwrap();

    println!("   Sandbox root: {}", sandbox.root().display());
    println!("   Executing task...");
    println!();

    let result = sandbox.execute().unwrap();

    println!("   ✅ Execution completed");
    println!("   Exit code: {}", result.exit_code);
    println!("   Duration:  {}ms", result.duration_ms);
    println!();

    // ========== Step 4: Display Execution Logs ==========
    println!("📋 Step 4: Execution logs");

    if !result.stdout.is_empty() {
        println!("   STDOUT:");
        for line in result.stdout.lines() {
            println!("     | {}", line);
        }
        println!();
    }

    if !result.stderr.is_empty() {
        println!("   STDERR:");
        for line in result.stderr.lines() {
            println!("     ! {}", line);
        }
        println!();
    }

    assert!(result.success(), "Task execution should succeed");

    // ========== Step 5: Collect Outputs ==========
    println!("📦 Step 5: Collecting outputs");

    let outputs = sandbox.collect_outputs().unwrap();

    println!("   Collected {} output files:", outputs.len());
    for (path, content) in &outputs {
        let content_str = String::from_utf8_lossy(content);
        println!("     📄 {}", path.display());
        println!("        Size: {} bytes", content.len());
        if content.len() < 200 {
            println!("        Content: {}", content_str.trim());
        } else {
            println!("        Content: {}... (truncated)", content_str.chars().take(100).collect::<String>());
        }
        println!();
    }

    // Verify expected outputs exist
    assert!(outputs.contains_key(&PathBuf::from("/work/outputs/compile.done")));
    assert!(outputs.contains_key(&PathBuf::from("/work/build/compiled.o")));
    assert!(outputs.contains_key(&PathBuf::from("/work/build/compile.log")));

    // ========== Step 6: Cleanup ==========
    println!("🧹 Step 6: Cleaning up sandbox");
    sandbox.cleanup().unwrap();
    println!("   ✅ Sandbox cleaned up");
    println!();

    println!("✅ Test completed successfully!\n");
}

#[test]
fn test_task_execution_with_failure() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    println!("\n❌ Testing task execution with failure\n");

    let tmp = TempDir::new().unwrap();
    let sandbox_dir = tmp.path().join("sandboxes");

    let task_script = generate_bitbake_task_script(
        "failing-recipe",
        "do_compile",
        r#"
echo "Starting compilation..."
echo "ERROR: Compilation failed!" >&2
exit 1
"#,
    );

    let mut spec = SandboxSpec::new(vec![task_script]);
    spec.cwd = PathBuf::from("/work");
    spec.rw_dirs.push(PathBuf::from("/work"));

    let manager = SandboxManager::new(&sandbox_dir).unwrap();
    let sandbox = manager.create_sandbox(spec).unwrap();

    println!("   Executing failing task...");
    let result = sandbox.execute().unwrap();

    println!("   Exit code: {}", result.exit_code);
    println!("   STDOUT: {}", result.stdout);
    println!("   STDERR: {}", result.stderr);
    println!();

    assert!(!result.success(), "Task should fail");
    assert_eq!(result.exit_code, 1, "Exit code should be 1");
    assert!(result.stderr.contains("ERROR: Compilation failed!"));

    sandbox.cleanup().unwrap();
    println!("✅ Failure handling test passed!\n");
}

#[test]
fn test_task_with_file_generation() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    println!("\n📝 Testing task with multiple file generation\n");

    let tmp = TempDir::new().unwrap();
    let sandbox_dir = tmp.path().join("sandboxes");

    let task_code = "
echo Installing files...

# Create directory structure
mkdir -p ${D}/usr/bin
mkdir -p ${D}/usr/lib
mkdir -p ${D}/etc

# Generate multiple files
printf '%s\\n' '#!/bin/sh' > ${D}/usr/bin/myapp
printf '%s\\n' 'echo Hello' >> ${D}/usr/bin/myapp
chmod +x ${D}/usr/bin/myapp

echo Library > ${D}/usr/lib/libmyapp.so

printf '%s\\n' '# Configuration' > ${D}/etc/myapp.conf
printf '%s\\n' 'option1=value1' >> ${D}/etc/myapp.conf

# Create completion marker
mkdir -p ${D}
echo done > ${D}/install.done

echo Installed 3 files
";

    let task_script = generate_bitbake_task_script(
        "multi-file-recipe",
        "do_install",
        task_code,
    );

    let mut spec = SandboxSpec::new(vec![task_script]);
    spec.cwd = PathBuf::from("/work");
    spec.rw_dirs.push(PathBuf::from("/work"));
    spec.env.insert("D".to_string(), "/work/outputs".to_string());

    // Collect entire output directory
    spec.outputs.push(PathBuf::from("/work/outputs"));

    let manager = SandboxManager::new(&sandbox_dir).unwrap();
    let sandbox = manager.create_sandbox(spec).unwrap();

    println!("   Executing install task...");
    let result = sandbox.execute().unwrap();

    assert!(result.success());
    println!("   ✅ Task completed");
    println!();

    let outputs = sandbox.collect_outputs().unwrap();

    println!("   Generated files:");
    for (path, content) in &outputs {
        let content_str = String::from_utf8_lossy(content);
        println!("     📄 {} ({} bytes)", path.display(), content.len());
        if content.len() < 100 {
            println!("        {}", content_str.trim().replace('\n', "\n        "));
        }
    }
    println!();

    // Verify files were created
    assert!(outputs.iter().any(|(p, _)| p.to_string_lossy().contains("myapp")));
    assert!(outputs.iter().any(|(p, _)| p.to_string_lossy().contains("libmyapp.so")));
    assert!(outputs.iter().any(|(p, _)| p.to_string_lossy().contains("myapp.conf")));

    sandbox.cleanup().unwrap();
    println!("✅ Multi-file generation test passed!\n");
}

/// Helper function to generate a complete BitBake-style task script
fn generate_bitbake_task_script(recipe: &str, task: &str, task_code: &str) -> String {
    format!(
        r#"#!/bin/bash
set -e

# ========================================
# BitBake Task: {}:{}
# ========================================

# BitBake Environment Variables
export PN="${{PN:-{}}}"
export WORKDIR="${{WORKDIR:-/work}}"
export S="${{S:-${{WORKDIR}}/src}}"
export B="${{B:-${{WORKDIR}}/build}}"
export D="${{D:-${{WORKDIR}}/outputs}}"

# Create working directories
mkdir -p "$S" "$B" "$D"

# Task Implementation
{}

# Verify task completed
echo "Task {} completed successfully"
"#,
        recipe, task, recipe, task_code, task
    )
}
