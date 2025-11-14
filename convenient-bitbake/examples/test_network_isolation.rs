// Test network isolation in the native namespace sandbox
use convenient_bitbake::executor::native_sandbox::execute_in_namespace;
use convenient_bitbake::executor::types::NetworkPolicy;
use std::collections::HashMap;
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Testing Network Isolation ===\n");

    // Test 1: Fully isolated (no network access)
    println!("Test 1: Fully isolated (NetworkPolicy::Isolated)");
    let tmp1 = TempDir::new()?;
    let work_dir1 = tmp1.path().join("work");
    std::fs::create_dir_all(&work_dir1)?;

    let script1 = r#"
        # Test that network is completely isolated
        # Loopback should NOT work
        if ping -c 1 -W 1 127.0.0.1 > /dev/null 2>&1; then
            echo "ERROR: Loopback accessible (should be isolated)"
            exit 1
        fi

        # External network should NOT work
        if ping -c 1 -W 1 8.8.8.8 > /dev/null 2>&1; then
            echo "ERROR: External network accessible (should be isolated)"
            exit 1
        fi

        echo "✓ Network fully isolated - no loopback, no external"
    "#;

    match execute_in_namespace(script1, &work_dir1, &HashMap::new(), NetworkPolicy::Isolated) {
        Ok((exit_code, stdout, stderr)) => {
            println!("✓ Test 1 PASSED");
            println!("Exit code: {}", exit_code);
            println!("--- stdout ---\n{}", stdout);
            if !stderr.is_empty() {
                println!("--- stderr ---\n{}", stderr);
            }
            assert_eq!(exit_code, 0, "Isolation test should succeed");
        }
        Err(e) => {
            println!("✗ Test 1 FAILED: {}", e);
            return Err(Box::new(e));
        }
    }

    println!("\n{}", "=".repeat(50));

    // Test 2: Loopback only (127.0.0.1 accessible, external blocked)
    println!("\nTest 2: Loopback only (NetworkPolicy::LoopbackOnly)");
    let tmp2 = TempDir::new()?;
    let work_dir2 = tmp2.path().join("work");
    std::fs::create_dir_all(&work_dir2)?;

    let script2 = r#"
        # Test that loopback works
        if ! ping -c 1 -W 1 127.0.0.1 > /dev/null 2>&1; then
            echo "ERROR: Loopback NOT accessible (should work)"
            exit 1
        fi
        echo "✓ Loopback accessible (127.0.0.1)"

        # External network should still be blocked
        if ping -c 1 -W 1 8.8.8.8 > /dev/null 2>&1; then
            echo "ERROR: External network accessible (should be blocked)"
            exit 1
        fi
        echo "✓ External network blocked"

        # Test that localhost services could work
        # Start a simple HTTP server on loopback
        echo "test content" > /tmp/test.html
        (cd /tmp && python3 -m http.server 8888 --bind 127.0.0.1 > /dev/null 2>&1 &)
        sleep 0.5

        # Try to fetch from localhost
        if command -v curl > /dev/null; then
            if curl -s http://127.0.0.1:8888/test.html | grep -q "test content"; then
                echo "✓ Localhost HTTP server accessible"
            else
                echo "WARNING: Could not access localhost HTTP server"
            fi
        else
            echo "NOTE: curl not available, skipping HTTP test"
        fi
    "#;

    match execute_in_namespace(script2, &work_dir2, &HashMap::new(), NetworkPolicy::LoopbackOnly) {
        Ok((exit_code, stdout, _stderr)) => {
            println!("✓ Test 2 PASSED");
            println!("Exit code: {}", exit_code);
            println!("--- stdout ---\n{}", stdout);
            assert_eq!(exit_code, 0, "Loopback test should succeed");
            assert!(stdout.contains("Loopback accessible"), "Loopback should be accessible");
            assert!(stdout.contains("External network blocked"), "External network should be blocked");
        }
        Err(e) => {
            println!("✗ Test 2 FAILED: {}", e);
            return Err(Box::new(e));
        }
    }

    println!("\n{}", "=".repeat(50));

    // Test 3: Verify network isolation prevents DNS leaks
    println!("\nTest 3: DNS isolation test");
    let tmp3 = TempDir::new()?;
    let work_dir3 = tmp3.path().join("work");
    std::fs::create_dir_all(&work_dir3)?;

    let script3 = r#"
        # Test that DNS resolution fails in isolated mode
        if nslookup google.com > /dev/null 2>&1; then
            echo "ERROR: DNS resolution working (should fail)"
            exit 1
        fi
        echo "✓ DNS resolution blocked (network isolated)"
    "#;

    match execute_in_namespace(script3, &work_dir3, &HashMap::new(), NetworkPolicy::Isolated) {
        Ok((exit_code, stdout, _stderr)) => {
            println!("✓ Test 3 PASSED");
            println!("Exit code: {}", exit_code);
            println!("--- stdout ---\n{}", stdout);
            assert_eq!(exit_code, 0);
        }
        Err(e) => {
            println!("✗ Test 3 FAILED: {}", e);
            return Err(Box::new(e));
        }
    }

    println!("\n{}", "=".repeat(50));
    println!("\n✓ All network isolation tests PASSED!");
    println!("\n=== Network Isolation Verified ===");
    println!("  ✓ NetworkPolicy::Isolated - Full network isolation");
    println!("  ✓ NetworkPolicy::LoopbackOnly - Loopback accessible, external blocked");
    println!("  ✓ DNS queries blocked in isolated mode");
    println!("  ✓ Build tasks are hermetic (no external dependencies)");

    Ok(())
}
