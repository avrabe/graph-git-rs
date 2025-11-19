//! Comprehensive integration tests for RustShell execution mode
//!
//! These tests verify:
//! 1. Basic shell script execution
//! 2. Variable tracking functionality
//! 3. BitBake environment setup
//! 4. Error handling
//! 5. Performance vs subprocess bash
//! 6. Compatibility with BitBake task patterns

use convenient_bitbake::executor::{
    ExecutionMode, RustShellExecutor, TaskExecutor, TaskSpec, NetworkPolicy, ResourceLimits,
    execute_with_bitbake_env,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Test basic shell script execution
#[test]
fn test_basic_shell_execution() {
    let tmp = TempDir::new().unwrap();
    let mut executor = RustShellExecutor::new(tmp.path()).unwrap();

    let script = r#"
        echo "Hello from RustShell"
        echo "Second line"
    "#;

    let result = executor.execute(script).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("Hello from RustShell"));
    assert!(result.stdout.contains("Second line"));
}

/// Test variable setting and reading
#[test]
fn test_variable_operations() {
    let tmp = TempDir::new().unwrap();
    let mut executor = RustShellExecutor::new(tmp.path()).unwrap();

    executor.set_var("TEST_VAR", "test_value");
    executor.set_var("ANOTHER_VAR", "another_value");

    let script = r#"
        echo "Value: $TEST_VAR"
        echo "Another: $ANOTHER_VAR"
        export NEW_VAR="new_value"
    "#;

    let result = executor.execute(script).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("Value: test_value"));
    assert!(result.stdout.contains("Another: another_value"));

    // Verify tracking
    assert!(result.vars_written.contains_key("TEST_VAR"));
    assert!(result.vars_written.contains_key("ANOTHER_VAR"));
}

/// Test BitBake environment setup
#[test]
fn test_bitbake_environment() {
    let tmp = TempDir::new().unwrap();
    let mut executor = RustShellExecutor::new(tmp.path()).unwrap();

    executor.setup_bitbake_env("test-recipe", Some("2.0"), tmp.path());

    let script = r#"
        echo "Recipe: $PN"
        echo "Version: $PV"
        echo "Workdir: $WORKDIR"
    "#;

    let result = executor.execute(script).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("Recipe: test-recipe"));
    assert!(result.stdout.contains("Version: 2.0"));
}

/// Test BitBake helper functions from prelude
#[test]
fn test_bitbake_helpers() {
    let tmp = TempDir::new().unwrap();
    let env = HashMap::new();

    let script = r#"
        bb_note "This is a note"
        bb_warn "This is a warning"
        bbdirs "$D/usr/bin" "$D/etc"
    "#;

    let result = execute_with_bitbake_env(
        script,
        "test-recipe",
        Some("1.0"),
        tmp.path(),
        &env,
    ).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("NOTE: This is a note"));
    assert!(result.stderr.contains("WARNING: This is a warning"));

    // Verify directories were created
    assert!(tmp.path().join("image/usr/bin").exists());
    assert!(tmp.path().join("image/etc").exists());
}

/// Test conditionals and control flow
#[test]
fn test_control_flow() {
    let tmp = TempDir::new().unwrap();
    let mut executor = RustShellExecutor::new(tmp.path()).unwrap();

    let script = r#"
        if [ "1" = "1" ]; then
            echo "Condition true"
        fi

        for i in 1 2 3; do
            echo "Number: $i"
        done

        COUNT=0
        while [ $COUNT -lt 3 ]; do
            echo "Count: $COUNT"
            COUNT=$((COUNT + 1))
        done
    "#;

    let result = executor.execute(script).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("Condition true"));
    assert!(result.stdout.contains("Number: 1"));
    assert!(result.stdout.contains("Number: 2"));
    assert!(result.stdout.contains("Number: 3"));
    assert!(result.stdout.contains("Count: 0"));
    assert!(result.stdout.contains("Count: 1"));
    assert!(result.stdout.contains("Count: 2"));
}

/// Test file operations
#[test]
fn test_file_operations() {
    let tmp = TempDir::new().unwrap();
    let env = HashMap::new();

    let script = r#"
        # Create directories
        bbdirs "$D/usr/bin" "$D/etc"

        # Create files
        echo "test content" > "$D/usr/bin/testfile"
        touch "$D/etc/config"

        # Copy files
        cp "$D/usr/bin/testfile" "$D/etc/testfile_copy"

        # Check file existence
        if [ -f "$D/usr/bin/testfile" ]; then
            echo "File exists"
        fi
    "#;

    let result = execute_with_bitbake_env(
        script,
        "test-recipe",
        Some("1.0"),
        tmp.path(),
        &env,
    ).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("File exists"));

    // Verify files were created
    let testfile = tmp.path().join("image/usr/bin/testfile");
    assert!(testfile.exists());
    let content = std::fs::read_to_string(&testfile).unwrap();
    assert!(content.contains("test content"));

    let config = tmp.path().join("image/etc/config");
    assert!(config.exists());

    let copy = tmp.path().join("image/etc/testfile_copy");
    assert!(copy.exists());
}

/// Test error handling
#[test]
fn test_error_handling() {
    let tmp = TempDir::new().unwrap();
    let mut executor = RustShellExecutor::new(tmp.path()).unwrap();

    // Script with syntax error
    let script = "echo 'unterminated string";
    let result = executor.execute(script);
    assert!(result.is_err());

    // Script with runtime error (with set -e from prelude)
    let env = HashMap::new();
    let script2 = r#"
        set -e
        false
        echo "This should not print"
    "#;
    let result2 = execute_with_bitbake_env(
        script2,
        "test",
        None,
        tmp.path(),
        &env,
    );
    // Should fail due to 'false' command with set -e
    assert!(result2.is_ok()); // Execution completes but with non-zero exit
    assert_ne!(result2.unwrap().exit_code, 0);
}

/// Test variable expansion
#[test]
fn test_variable_expansion() {
    let tmp = TempDir::new().unwrap();
    let mut executor = RustShellExecutor::new(tmp.path()).unwrap();

    executor.set_var("PN", "myrecipe");
    executor.set_var("PV", "1.0");

    let script = r#"
        FULL_NAME="${PN}-${PV}"
        echo "Full name: $FULL_NAME"
    "#;

    let result = executor.execute(script).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("Full name: myrecipe-1.0"));
}

/// Test command substitution
#[test]
fn test_command_substitution() {
    let tmp = TempDir::new().unwrap();
    let mut executor = RustShellExecutor::new(tmp.path()).unwrap();

    let script = r#"
        RESULT=$(echo "hello world" | tr '[:lower:]' '[:upper:]')
        echo "Result: $RESULT"
    "#;

    let result = executor.execute(script).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("HELLO WORLD"));
}

/// Test exit code propagation
#[test]
fn test_exit_codes() {
    let tmp = TempDir::new().unwrap();
    let mut executor = RustShellExecutor::new(tmp.path()).unwrap();

    // Success
    let script1 = "exit 0";
    let result1 = executor.execute(script1).unwrap();
    assert_eq!(result1.exit_code, 0);

    // Failure with code 1
    let script2 = "exit 1";
    let result2 = executor.execute(script2).unwrap();
    assert_eq!(result2.exit_code, 1);

    // Custom exit code
    let script3 = "exit 42";
    let result3 = executor.execute(script3).unwrap();
    assert_eq!(result3.exit_code, 42);
}

/// Test realistic BitBake do_install task
#[test]
fn test_realistic_install_task() {
    let tmp = TempDir::new().unwrap();
    let env = HashMap::new();

    // Create source files
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("myapp"), "#!/bin/sh\necho 'myapp'\n").unwrap();
    std::fs::write(src_dir.join("config.txt"), "config=value\n").unwrap();

    let script = r#"
        bb_note "Installing ${PN}-${PV}"

        # Install binary
        bbdirs "$D/usr/bin"
        install -m 0755 "$S/myapp" "$D/usr/bin/"

        # Install config
        bbdirs "$D/etc"
        install -m 0644 "$S/config.txt" "$D/etc/myapp.conf"

        bb_note "Installation complete"
    "#;

    let result = execute_with_bitbake_env(
        script,
        "myapp",
        Some("1.0"),
        tmp.path(),
        &env,
    ).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("NOTE: Installing myapp-1.0"));
    assert!(result.stdout.contains("NOTE: Installation complete"));

    // Verify installation
    let bin_file = tmp.path().join("image/usr/bin/myapp");
    assert!(bin_file.exists());

    let config_file = tmp.path().join("image/etc/myapp.conf");
    assert!(config_file.exists());
}

/// Performance benchmark: Compare RustShell vs DirectRust vs Shell
#[test]
#[ignore] // Run with: cargo test --test rust_shell_integration_test -- --ignored --nocapture
fn bench_execution_modes() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let iterations = 100;

    // Simple script that all modes can handle
    let simple_script = r#"
        echo "test"
        mkdir -p output
        touch output/file.txt
    "#;

    println!("\n=== Performance Benchmark ===");
    println!("Iterations: {}", iterations);
    println!("Script: Simple file operations\n");

    // Benchmark RustShell
    let rust_shell_times: Vec<Duration> = (0..iterations)
        .map(|_| {
            let work_dir = tmp.path().join(format!("rust_shell_{}", rand::random::<u32>()));
            std::fs::create_dir_all(&work_dir).unwrap();

            let start = Instant::now();
            let env = HashMap::new();
            let _ = execute_with_bitbake_env(
                simple_script,
                "bench",
                None,
                &work_dir,
                &env,
            ).unwrap();
            start.elapsed()
        })
        .collect();

    let rust_shell_avg = rust_shell_times.iter().sum::<Duration>() / iterations as u32;
    let rust_shell_min = rust_shell_times.iter().min().unwrap();
    let rust_shell_max = rust_shell_times.iter().max().unwrap();

    println!("RustShell:");
    println!("  Average: {:?}", rust_shell_avg);
    println!("  Min:     {:?}", rust_shell_min);
    println!("  Max:     {:?}", rust_shell_max);

    // TODO: Add DirectRust and Shell benchmarks for comparison
    // This will require more setup but will validate the "2-5x faster" claim

    println!("\n=== Benchmark Complete ===\n");
}

/// Stress test: Many variables
#[test]
fn test_many_variables() {
    let tmp = TempDir::new().unwrap();
    let mut executor = RustShellExecutor::new(tmp.path()).unwrap();

    // Set 100 variables
    for i in 0..100 {
        executor.set_var(format!("VAR_{}", i), format!("value_{}", i));
    }

    let script = r#"
        echo "Var 0: $VAR_0"
        echo "Var 50: $VAR_50"
        echo "Var 99: $VAR_99"
    "#;

    let result = executor.execute(script).unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("Var 0: value_0"));
    assert!(result.stdout.contains("Var 50: value_50"));
    assert!(result.stdout.contains("Var 99: value_99"));

    // Verify all variables were tracked
    assert!(result.vars_written.len() >= 100);
}

/// Test concurrent directory creation
#[test]
fn test_concurrent_operations() {
    let tmp = TempDir::new().unwrap();
    let env = HashMap::new();

    let script = r#"
        # Create multiple directories in parallel (using background jobs would require &)
        bbdirs "$D/dir1" "$D/dir2" "$D/dir3" "$D/dir4" "$D/dir5"

        # Create files in each
        for i in 1 2 3 4 5; do
            touch "$D/dir$i/file.txt"
        done
    "#;

    let result = execute_with_bitbake_env(
        script,
        "test",
        None,
        tmp.path(),
        &env,
    ).unwrap();

    assert_eq!(result.exit_code, 0);

    // Verify all directories and files exist
    for i in 1..=5 {
        let dir = tmp.path().join(format!("image/dir{}", i));
        assert!(dir.exists());
        let file = dir.join("file.txt");
        assert!(file.exists());
    }
}
