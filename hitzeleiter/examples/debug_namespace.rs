//! Debug namespace creation to find exact failure point

use nix::sched::{unshare, CloneFlags};
use nix::unistd::{fork, ForkResult, getuid, getgid};
use std::fs;

fn main() {
    println!("=== Namespace Debug Test ===\n");

    // Test 1: Can we unshare in parent process?
    println!("Test 1: Unshare in parent process");
    match unshare(CloneFlags::CLONE_NEWUSER) {
        Ok(_) => {
            println!("✅ CLONE_NEWUSER succeeded in parent");

            // Try to setup UID/GID mapping
            let uid = getuid();
            let gid = getgid();

            println!("  UID: {}, GID: {}", uid, gid);

            match fs::write("/proc/self/uid_map", format!("0 {} 1", uid)) {
                Ok(_) => println!("✅ uid_map write succeeded"),
                Err(e) => println!("❌ uid_map write failed: {}", e),
            }

            match fs::write("/proc/self/setgroups", "deny") {
                Ok(_) => println!("✅ setgroups write succeeded"),
                Err(e) => println!("❌ setgroups write failed: {}", e),
            }

            match fs::write("/proc/self/gid_map", format!("0 {} 1", gid)) {
                Ok(_) => println!("✅ gid_map write succeeded"),
                Err(e) => println!("❌ gid_map write failed: {}", e),
            }
        }
        Err(e) => println!("❌ CLONE_NEWUSER failed: {}", e),
    }

    println!("\nTest 2: Fork then unshare");

    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            println!("Parent waiting for child {}", child);
            let _ = nix::sys::wait::waitpid(child, None);
        }
        Ok(ForkResult::Child) => {
            println!("Child process starting");

            match unshare(CloneFlags::CLONE_NEWUSER) {
                Ok(_) => {
                    println!("✅ Child: CLONE_NEWUSER succeeded");

                    let uid = getuid();
                    let gid = getgid();
                    println!("  Child UID: {}, GID: {}", uid, gid);

                    match fs::write("/proc/self/uid_map", format!("0 {} 1", uid)) {
                        Ok(_) => println!("✅ Child: uid_map write succeeded"),
                        Err(e) => {
                            println!("❌ Child: uid_map write failed: {}", e);
                            std::process::exit(1);
                        }
                    }

                    println!("✅ Child succeeded!");
                    std::process::exit(0);
                }
                Err(e) => {
                    println!("❌ Child: CLONE_NEWUSER failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            println!("❌ Fork failed: {}", e);
        }
    }
}
