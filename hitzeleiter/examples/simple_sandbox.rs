//! Simple sandbox demonstration
//!
//! Run with: cargo run --example simple_sandbox
//!
//! This demonstrates:
//! 1. Creating a sandbox with namespaces
//! 2. Executing a simple script
//! 3. Verifying outputs

use bitzel::sandbox::{SandboxBuilder, DependencyLayer};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║        BITZEL SANDBOX DEMONSTRATION                   ║");
    println!("║  Linux Namespaces + OverlayFS Proof of Concept       ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    // Create temporary directories
    let temp_dir = TempDir::new()?;
    let sandbox_dir = temp_dir.path().join("sandbox");
    fs::create_dir_all(&sandbox_dir)?;

    println!("📁 Sandbox directory: {}", sandbox_dir.display());
    println!();

    // ========== Test 1: Simple Script Execution ==========
    println!("🧪 Test 1: Simple script execution with namespaces");
    println!("─────────────────────────────────────────────────────");

    let script = r#"
set -e
echo "Hello from sandbox!"
echo "Current directory: $(pwd)"
echo "Hostname: $(hostname)"
echo "Process ID: $$"

# Create output file
mkdir -p /work/outputs
echo "Task completed successfully" > /work/outputs/test.done
cat /work/outputs/test.done
"#;

    let sandbox = SandboxBuilder::new()
        .sandbox_dir(sandbox_dir.clone())
        .script(script.to_string())
        .env("TEST_VAR".to_string(), "test_value".to_string())
        .build()?;

    println!("Executing sandboxed script...\n");

    match sandbox.execute() {
        Ok(output) => {
            println!("✅ Execution successful!\n");
            println!("STDOUT:");
            println!("{}", String::from_utf8_lossy(&output.stdout));

            if !output.stderr.is_empty() {
                println!("STDERR:");
                println!("{}", String::from_utf8_lossy(&output.stderr));
            }

            // Check output file
            let output_file = sandbox_dir.join("work/outputs/test.done");
            if output_file.exists() {
                let content = fs::read_to_string(&output_file)?;
                println!("📄 Output file content: {}", content.trim());
            }
        }
        Err(e) => {
            println!("❌ Execution failed: {}", e);
            return Err(e.into());
        }
    }

    println!();

    // ========== Test 2: Overlay Mount (if dependencies available) ==========
    println!("🧪 Test 2: OverlayFS mount demonstration");
    println!("─────────────────────────────────────────────────────");

    // Create fake dependency sysroots
    let dep1_dir = temp_dir.path().join("dep1/sysroot");
    let dep2_dir = temp_dir.path().join("dep2/sysroot");

    fs::create_dir_all(&dep1_dir.join("usr/lib"))?;
    fs::create_dir_all(&dep2_dir.join("usr/include"))?;

    // Create fake files in dependencies
    fs::write(dep1_dir.join("usr/lib/libfoo.so"), "fake lib from dep1")?;
    fs::write(dep2_dir.join("usr/include/bar.h"), "fake header from dep2")?;

    println!("Created fake dependencies:");
    println!("  dep1: usr/lib/libfoo.so");
    println!("  dep2: usr/include/bar.h");
    println!();

    let overlay_script = r#"
set -e
echo "Checking overlay-mounted sysroot..."

# Check if files from dependencies are visible
if [ -f /work/recipe-sysroot/usr/lib/libfoo.so ]; then
    echo "✅ Found libfoo.so from dep1"
    cat /work/recipe-sysroot/usr/lib/libfoo.so
else
    echo "❌ libfoo.so not found!"
    exit 1
fi

if [ -f /work/recipe-sysroot/usr/include/bar.h ]; then
    echo "✅ Found bar.h from dep2"
    cat /work/recipe-sysroot/usr/include/bar.h
else
    echo "❌ bar.h not found!"
    exit 1
fi

# List merged contents
echo ""
echo "Merged sysroot contents:"
find /work/recipe-sysroot -type f

# Create output
mkdir -p /work/outputs
echo "Overlay test passed" > /work/outputs/overlay.done
"#;

    let sandbox2_dir = temp_dir.path().join("sandbox2");
    fs::create_dir_all(&sandbox2_dir)?;

    let sandbox2 = SandboxBuilder::new()
        .sandbox_dir(sandbox2_dir.clone())
        .script(overlay_script.to_string())
        .target_dep(DependencyLayer {
            recipe: "dep1".to_string(),
            task: "do_populate_sysroot".to_string(),
            sysroot_path: dep1_dir,
        })
        .target_dep(DependencyLayer {
            recipe: "dep2".to_string(),
            task: "do_populate_sysroot".to_string(),
            sysroot_path: dep2_dir,
        })
        .build()?;

    println!("Executing with overlay mounts...\n");

    match sandbox2.execute() {
        Ok(output) => {
            println!("✅ Overlay execution successful!\n");
            println!("STDOUT:");
            println!("{}", String::from_utf8_lossy(&output.stdout));

            if !output.stderr.is_empty() {
                println!("STDERR:");
                println!("{}", String::from_utf8_lossy(&output.stderr));
            }
        }
        Err(e) => {
            println!("❌ Overlay execution failed: {}", e);
            println!("This is expected if overlayfs is not supported on this system");
        }
    }

    println!();
    println!("╔════════════════════════════════════════════════════════╗");
    println!("║                 TESTS COMPLETE                         ║");
    println!("╚════════════════════════════════════════════════════════╝");

    Ok(())
}
