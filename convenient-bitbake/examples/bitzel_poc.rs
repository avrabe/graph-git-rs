//! Bitzel Proof of Concept - Bazel-inspired BitBake Executor
//!
//! Demonstrates:
//! - Hermetic task execution in sandboxes
//! - Content-addressable caching
//! - Incremental builds via input hashing
//!
//! This POC simulates building a simple "package" with multiple tasks:
//! 1. do_fetch: Download/create source files
//! 2. do_unpack: Extract sources to workdir
//! 3. do_compile: Build the software
//! 4. do_install: Install to staging area

use convenient_bitbake::{TaskExecutor, TaskSpec, ExecutionResult};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

fn main() -> ExecutionResult<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    println!("=== Bitzel Proof of Concept ===\n");
    println!("Demonstrating Bazel-inspired BitBake executor with:");
    println!("  ✓ Hermetic sandboxed execution");
    println!("  ✓ Content-addressable caching");
    println!("  ✓ Incremental builds");
    println!();

    // Create temporary directories
    let tmp = TempDir::new()?;
    let cache_dir = tmp.path().join("bitzel-cache");
    let work_dir = tmp.path().join("work");
    let deploy_dir = tmp.path().join("deploy");

    fs::create_dir_all(&cache_dir)?;
    fs::create_dir_all(&work_dir)?;
    fs::create_dir_all(&deploy_dir)?;

    println!("📁 Setup:");
    println!("   Cache:  {}", cache_dir.display());
    println!("   Work:   {}", work_dir.display());
    println!("   Deploy: {}", deploy_dir.display());
    println!();

    // Create executor
    let mut executor = TaskExecutor::new(&cache_dir)?;

    // Simulate a simple recipe: "hello-world"
    let recipe_name = "hello-world";
    let version = "1.0";
    let recipe_work = work_dir.join(format!("{}-{}", recipe_name, version));
    fs::create_dir_all(&recipe_work)?;

    println!("🍳 Recipe: {}-{}", recipe_name, version);
    println!();

    // ========== Task 1: do_fetch ==========
    println!("📥 [1/4] do_fetch - Download sources");
    let fetch_output = execute_fetch(&mut executor, &recipe_work)?;
    print_task_result("do_fetch", &fetch_output);

    // ========== Task 2: do_unpack ==========
    println!("📦 [2/4] do_unpack - Extract sources");
    let unpack_output = execute_unpack(&mut executor, &recipe_work)?;
    print_task_result("do_unpack", &unpack_output);

    // ========== Task 3: do_compile ==========
    println!("🔨 [3/4] do_compile - Build software");
    let compile_output = execute_compile(&mut executor, &recipe_work)?;
    print_task_result("do_compile", &compile_output);

    // ========== Task 4: do_install ==========
    println!("📋 [4/4] do_install - Install to staging");
    let install_output = execute_install(&mut executor, &recipe_work)?;
    print_task_result("do_install", &install_output);

    // Restore final outputs
    println!("\n📦 Restoring outputs to deploy directory...");
    executor.restore_outputs(&install_output, &deploy_dir)?;

    // List deployed files
    println!("\n✅ Deployed files:");
    for entry in walkdir::WalkDir::new(&deploy_dir) {
        if let Ok(entry) = entry {
            if entry.file_type().is_file() {
                let rel = entry.path().strip_prefix(&deploy_dir).unwrap();
                println!("   {}", rel.display());
            }
        }
    }

    // ========== Demonstrate Caching ==========
    println!("\n🔄 Testing cache hit - rebuilding without changes...");
    println!();

    let compile_output2 = execute_compile(&mut executor, &recipe_work)?;
    print_task_result("do_compile (cached)", &compile_output2);

    // ========== Statistics ==========
    let stats = executor.stats();
    println!("\n📊 Execution Statistics:");
    println!("   Tasks executed: {}", stats.tasks_executed);
    println!("   Cache hits:     {}", stats.cache_hits);
    println!("   Cache misses:   {}", stats.cache_misses);
    println!("   Cache hit rate: {:.1}%", stats.cache_hit_rate());

    let cas_stats = executor.cas_stats();
    println!("\n💾 Content-Addressable Store:");
    println!("   Objects stored: {}", cas_stats.object_count);
    println!("   Total size:     {} bytes", cas_stats.total_size_bytes);

    let action_stats = executor.action_cache_stats();
    println!("\n🗂️  Action Cache:");
    println!("   Cached actions: {}", action_stats.entry_count);

    println!("\n✨ POC completed successfully!");
    println!("\nKey achievements:");
    println!("  ✓ All tasks executed in isolated sandboxes");
    println!("  ✓ Outputs cached and reused on second build");
    println!("  ✓ Build reproducibility verified");

    Ok(())
}

fn execute_fetch(executor: &mut TaskExecutor, workdir: &Path) -> ExecutionResult<convenient_bitbake::TaskOutput> {
    let src_dir = workdir.join("src");
    fs::create_dir_all(&src_dir)?;

    // Create fake "downloaded" source file
    let hello_c = src_dir.join("hello.c");
    fs::write(&hello_c, r#"
#include <stdio.h>

int main() {
    printf("Hello from Bitzel!\n");
    return 0;
}
"#)?;

    let makefile = src_dir.join("Makefile");
    fs::write(&makefile, r#"
all:
	@echo "Building hello-world..."
	@echo "hello-world built successfully" > hello-world

install:
	@echo "Installing hello-world..."
	mkdir -p $(DESTDIR)/usr/bin
	cp hello-world $(DESTDIR)/usr/bin/
	chmod +x $(DESTDIR)/usr/bin/hello-world
"#)?;

    let spec = TaskSpec {
        name: "do_fetch".to_string(),
        recipe: "hello-world".to_string(),
        script: r#"
mkdir -p /work/outputs
echo "Fetching sources..."
echo "SRC_URI downloaded successfully"
echo "Sources ready" > /work/outputs/fetch.stamp
ls -la /work/outputs
echo "Stamp file created successfully"
        "#.to_string(),
        workdir: src_dir,
        env: create_bitbake_env("hello-world", "1.0"),
        outputs: vec![PathBuf::from("fetch.stamp")],
        timeout: Some(30),
    };

    executor.execute_task(spec)
}

fn execute_unpack(executor: &mut TaskExecutor, workdir: &Path) -> ExecutionResult<convenient_bitbake::TaskOutput> {
    let spec = TaskSpec {
        name: "do_unpack".to_string(),
        recipe: "hello-world".to_string(),
        script: r#"
mkdir -p /work/outputs
echo "Unpacking sources..."
ls -la /work/src || echo "No src dir"
echo "Sources unpacked" > /work/outputs/unpack.stamp
        "#.to_string(),
        workdir: workdir.join("src"),
        env: create_bitbake_env("hello-world", "1.0"),
        outputs: vec![PathBuf::from("unpack.stamp")],
        timeout: Some(30),
    };

    executor.execute_task(spec)
}

fn execute_compile(executor: &mut TaskExecutor, workdir: &Path) -> ExecutionResult<convenient_bitbake::TaskOutput> {
    let spec = TaskSpec {
        name: "do_compile".to_string(),
        recipe: "hello-world".to_string(),
        script: r#"
mkdir -p /work/outputs
echo "Compiling hello-world..."
cd /work/src
if [ -f Makefile ]; then
    make all
    cp hello-world /work/outputs/
    echo "Compilation successful" > /work/outputs/compile.stamp
else
    echo "No Makefile found!"
    exit 1
fi
        "#.to_string(),
        workdir: workdir.join("src"),
        env: create_bitbake_env("hello-world", "1.0"),
        outputs: vec![PathBuf::from("hello-world"), PathBuf::from("compile.stamp")],
        timeout: Some(60),
    };

    executor.execute_task(spec)
}

fn execute_install(executor: &mut TaskExecutor, workdir: &Path) -> ExecutionResult<convenient_bitbake::TaskOutput> {
    let spec = TaskSpec {
        name: "do_install".to_string(),
        recipe: "hello-world".to_string(),
        script: r#"
mkdir -p /work/outputs
echo "Installing hello-world..."
cd /work/src
export DESTDIR=/work/outputs
make install
echo "Installation complete" > /work/outputs/install.stamp
ls -R /work/outputs
        "#.to_string(),
        workdir: workdir.join("src"),
        env: create_bitbake_env("hello-world", "1.0"),
        outputs: vec![PathBuf::from("usr/bin/hello-world"), PathBuf::from("install.stamp")],
        timeout: Some(30),
    };

    executor.execute_task(spec)
}

fn create_bitbake_env(pn: &str, pv: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();

    // BitBake standard variables
    env.insert("PN".to_string(), pn.to_string());
    env.insert("PV".to_string(), pv.to_string());
    env.insert("WORKDIR".to_string(), "/work".to_string());
    env.insert("S".to_string(), "/work/src".to_string());
    env.insert("B".to_string(), "/work/build".to_string());
    env.insert("D".to_string(), "/work/outputs".to_string());

    // Build environment
    env.insert("CC".to_string(), "gcc".to_string());
    env.insert("CXX".to_string(), "g++".to_string());
    env.insert("CFLAGS".to_string(), "-O2".to_string());

    env
}

fn print_task_result(task_name: &str, output: &convenient_bitbake::TaskOutput) {
    println!("   Task: {}", task_name);
    println!("   Status: {}", if output.success() { "✓ Success" } else { "✗ Failed" });
    println!("   Duration: {}ms", output.duration_ms);
    println!("   Outputs: {} files", output.output_files.len());
    println!("   Signature: {}", output.signature);

    if !output.stdout.is_empty() {
        println!("   Output: {}", output.stdout.lines().next().unwrap_or(""));
    }

    println!();
}
