//! Test real fetching using bb.fetch2 Python bridge
//!
//! This example demonstrates the complete Python-to-Rust fetch pipeline:
//! 1. Execute Python do_fetch code using RustPython
//! 2. Python calls bb.fetch2.Fetch()
//! 3. bb.fetch2 delegates to Rust fetch_handler
//! 4. Real git clone or wget download happens
//!
//! Run with:
//! ```sh
//! cargo run --example test_real_fetch
//! ```

use convenient_bitbake::PythonExecutor;
use std::collections::HashMap;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for visibility
    tracing_subscriber::fmt::init();

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║     Test Real Fetch with bb.fetch2 Python Bridge      ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    // Set downloads directory
    let downloads_dir = env::temp_dir().join("bitzel-test-downloads");
    std::fs::create_dir_all(&downloads_dir)?;
    unsafe {
        env::set_var("DL_DIR", &downloads_dir);
    }
    println!("📁 Downloads directory: {}\n", downloads_dir.display());

    // Test 1: Simple HTTP download
    println!("Test 1: HTTP File Download");
    println!("─────────────────────────────────────");
    test_http_fetch()?;
    println!();

    // Test 2: Git repository clone
    println!("Test 2: Git Repository Clone");
    println!("─────────────────────────────────────");
    test_git_fetch()?;
    println!();

    // Test 3: Real BitBake do_fetch Python code
    println!("Test 3: Real BitBake do_fetch");
    println!("─────────────────────────────────────");
    test_bitbake_do_fetch()?;
    println!();

    println!("╔════════════════════════════════════════════════════════╗");
    println!("║           ✅ ALL TESTS PASSED!                         ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    println!("📊 Summary:");
    println!("  • bb.fetch2 Python bridge: ✓ Working");
    println!("  • HTTP downloads: ✓ Working");
    println!("  • Git clones: ✓ Working");
    println!("  • Real BitBake Python: ✓ Working");
    println!("\n🎉 Bitzel can now execute real do_fetch tasks!\n");

    Ok(())
}

/// Test fetching a small file via HTTP
fn test_http_fetch() -> Result<(), Box<dyn std::error::Error>> {
    let python_code = r#"
import bb.fetch2

# Fetch a small test file
src_uri = ["https://raw.githubusercontent.com/torvalds/linux/master/README"]
fetcher = bb.fetch2.Fetch(src_uri, d)
fetcher.download()

print("✓ HTTP fetch completed!")
"#;

    let executor = PythonExecutor::new();
    let mut initial_vars = HashMap::new();
    initial_vars.insert("PN".to_string(), "test-http".to_string());

    let result = executor.execute(python_code, &initial_vars);

    if result.success {
        println!("  ✓ Successfully fetched README from linux repository");
    } else {
        eprintln!("  ✗ Failed: {:?}", result.error);
        return Err(format!("HTTP fetch failed: {:?}", result.error).into());
    }

    Ok(())
}

/// Test cloning a git repository
fn test_git_fetch() -> Result<(), Box<dyn std::error::Error>> {
    let python_code = r#"
import bb.fetch2

# Clone a small repository
src_uri = ["git://github.com/git/git.git;branch=master;protocol=https"]
fetcher = bb.fetch2.Fetch(src_uri, d)
fetcher.download()

print("✓ Git clone completed!")
"#;

    let executor = PythonExecutor::new();
    let mut initial_vars = HashMap::new();
    initial_vars.insert("PN".to_string(), "git".to_string());

    let result = executor.execute(python_code, &initial_vars);

    if result.success {
        println!("  ✓ Successfully cloned git repository");
    } else {
        eprintln!("  ✗ Failed: {:?}", result.error);
        return Err(format!("Git fetch failed: {:?}", result.error).into());
    }

    Ok(())
}

/// Test real BitBake do_fetch Python code
fn test_bitbake_do_fetch() -> Result<(), Box<dyn std::error::Error>> {
    // This is REAL BitBake Python code from recipes!
    let python_code = r#"
# This is how real BitBake do_fetch looks:
def do_fetch():
    src_uri = (d.getVar('SRC_URI') or "").split()
    if not src_uri:
        print("No SRC_URI defined")
        return

    print(f"Fetching {len(src_uri)} sources...")

    import bb.fetch2
    fetcher = bb.fetch2.Fetch(src_uri, d)
    fetcher.download()

    print("✓ All sources fetched!")

do_fetch()
"#;

    let executor = PythonExecutor::new();
    let mut initial_vars = HashMap::new();
    initial_vars.insert("PN".to_string(), "hello".to_string());
    initial_vars.insert(
        "SRC_URI".to_string(),
        "https://ftp.gnu.org/gnu/hello/hello-2.12.tar.gz".to_string(),
    );

    let result = executor.execute(python_code, &initial_vars);

    if result.success {
        println!("  ✓ Real BitBake do_fetch executed successfully!");
        println!("  ✓ Downloaded: hello-2.12.tar.gz");
    } else {
        eprintln!("  ✗ Failed: {:?}", result.error);
        return Err(format!("BitBake do_fetch failed: {:?}", result.error).into());
    }

    Ok(())
}
