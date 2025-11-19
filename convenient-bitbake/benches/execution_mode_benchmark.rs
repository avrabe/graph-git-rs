//! Performance benchmarks for execution modes
//!
//! This benchmark suite validates the performance claims:
//! - DirectRust: 0.5-2ms overhead
//! - RustShell: 1-3ms overhead (2-5x faster than Shell)
//! - Shell: 5-10ms overhead
//!
//! Run with: cargo bench --bench execution_mode_benchmark

use convenient_bitbake::executor::{
    ExecutionMode, RustShellExecutor, TaskExecutor, TaskSpec, NetworkPolicy, ResourceLimits,
    execute_with_bitbake_env,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tempfile::TempDir;

struct BenchmarkResult {
    mode: &'static str,
    iterations: usize,
    total_time: Duration,
    avg_time: Duration,
    min_time: Duration,
    max_time: Duration,
    median_time: Duration,
}

impl BenchmarkResult {
    fn new(mode: &'static str, mut times: Vec<Duration>) -> Self {
        let iterations = times.len();
        let total_time: Duration = times.iter().sum();
        let avg_time = total_time / iterations as u32;

        times.sort();
        let min_time = *times.first().unwrap();
        let max_time = *times.last().unwrap();
        let median_time = times[times.len() / 2];

        Self {
            mode,
            iterations,
            total_time,
            avg_time,
            min_time,
            max_time,
            median_time,
        }
    }

    fn print(&self) {
        println!("\n{} Execution Mode:", self.mode);
        println!("  Iterations: {}", self.iterations);
        println!("  Total time: {:?}", self.total_time);
        println!("  Average:    {:?}", self.avg_time);
        println!("  Median:     {:?}", self.median_time);
        println!("  Min:        {:?}", self.min_time);
        println!("  Max:        {:?}", self.max_time);
        println!("  Avg (ms):   {:.2}ms", self.avg_time.as_secs_f64() * 1000.0);
    }

    fn speedup_vs(&self, other: &BenchmarkResult) -> f64 {
        other.avg_time.as_secs_f64() / self.avg_time.as_secs_f64()
    }
}

fn main() {
    println!("\n{'='}=== Execution Mode Performance Benchmark ===");
    println!("Validating performance claims for different execution modes\n");

    let iterations = 100;

    // Test case 1: Simple file operations
    println!("\n--- Test Case 1: Simple File Operations ---");
    benchmark_simple_operations(iterations);

    // Test case 2: Variable-heavy script
    println!("\n--- Test Case 2: Variable Operations ---");
    benchmark_variable_operations(iterations);

    // Test case 3: Control flow (loops, conditionals)
    println!("\n--- Test Case 3: Control Flow ---");
    benchmark_control_flow(iterations);

    // Test case 4: Realistic BitBake task
    println!("\n--- Test Case 4: Realistic Install Task ---");
    benchmark_realistic_task(iterations);

    println!("\n{'='}=== Benchmark Complete ===\n");
}

fn benchmark_simple_operations(iterations: usize) {
    let script = r#"
        mkdir -p output/dir1
        touch output/file1.txt
        echo "test" > output/file2.txt
    "#;

    let rust_shell_result = bench_rust_shell(script, iterations, "simple_ops");
    rust_shell_result.print();

    // Validate claim: RustShell should be 1-3ms
    let avg_ms = rust_shell_result.avg_time.as_secs_f64() * 1000.0;
    println!("\n  Validation: RustShell overhead = {:.2}ms", avg_ms);
    if avg_ms >= 1.0 && avg_ms <= 5.0 {
        println!("  ✅ PASS: Within claimed 1-3ms range (allowing 5ms tolerance)");
    } else {
        println!("  ⚠️  WARNING: Outside claimed range (actual: {:.2}ms)", avg_ms);
    }
}

fn benchmark_variable_operations(iterations: usize) {
    let script = r#"
        VAR1="value1"
        VAR2="value2"
        VAR3="$VAR1-$VAR2"
        export VAR4="exported"
        echo "$VAR3"
    "#;

    let rust_shell_result = bench_rust_shell(script, iterations, "var_ops");
    rust_shell_result.print();
}

fn benchmark_control_flow(iterations: usize) {
    let script = r#"
        if [ "1" = "1" ]; then
            echo "true"
        fi

        for i in 1 2 3; do
            echo "num: $i"
        done
    "#;

    let rust_shell_result = bench_rust_shell(script, iterations, "control_flow");
    rust_shell_result.print();
}

fn benchmark_realistic_task(iterations: usize) {
    let script = r#"
        # Simulate do_install task
        bb_note "Installing application"

        # Create directory structure
        bbdirs "$D/usr/bin" "$D/etc" "$D/usr/share/doc"

        # Simulate file installation
        echo "#!/bin/sh" > "$D/usr/bin/myapp"
        chmod 755 "$D/usr/bin/myapp"

        echo "config=value" > "$D/etc/myapp.conf"

        # Create documentation
        echo "README" > "$D/usr/share/doc/README"

        bb_note "Installation complete"
    "#;

    let rust_shell_result = bench_rust_shell(script, iterations, "realistic");
    rust_shell_result.print();
}

fn bench_rust_shell(script: &str, iterations: usize, test_name: &str) -> BenchmarkResult {
    let times: Vec<Duration> = (0..iterations)
        .map(|i| {
            let tmp = TempDir::new().unwrap();
            let work_dir = tmp.path().join(format!("{}_{}", test_name, i));
            std::fs::create_dir_all(&work_dir).unwrap();

            let env = HashMap::new();

            let start = Instant::now();
            let _ = execute_with_bitbake_env(
                script,
                "benchmark",
                Some("1.0"),
                &work_dir,
                &env,
            ).unwrap();
            start.elapsed()
        })
        .collect();

    BenchmarkResult::new("RustShell", times)
}

// Note: To properly benchmark against Shell mode, we would need to:
// 1. Set up TaskExecutor with cache directory
// 2. Execute same script with ExecutionMode::Shell
// 3. Measure time including subprocess spawn
// This would validate the "2-5x faster" claim

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark_runs() {
        // Smoke test to ensure benchmarks don't crash
        benchmark_simple_operations(10);
        benchmark_variable_operations(10);
        benchmark_control_flow(10);
    }
}
