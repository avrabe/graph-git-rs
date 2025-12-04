//! Cross-compilation toolchain management
//!
//! This module provides toolchain detection, configuration, and environment
//! variable generation for cross-compilation builds. It maps BitBake MACHINE
//! values to appropriate cross-compilation toolchains.
//!
//! ## Design Principles
//!
//! - **Auto-detection**: Find installed cross-compilers automatically
//! - **Explicit mapping**: MACHINE → toolchain prefix mapping
//! - **Environment injection**: Generate complete cross-compilation environment
//! - **SDK support**: Compatible with Yocto SDKs and standalone toolchains
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │ MACHINE="qemuarm64"                                     │
//! └─────────────────────────────────────────────────────────┘
//!                           ↓
//! ┌─────────────────────────────────────────────────────────┐
//! │ ToolchainManager::for_machine("qemuarm64")              │
//! │   ├─ Detect: aarch64-poky-linux-gcc                     │
//! │   ├─ Fallback: aarch64-linux-gnu-gcc                    │
//! │   └─ Generate environment variables                     │
//! └─────────────────────────────────────────────────────────┘
//!                           ↓
//! ┌─────────────────────────────────────────────────────────┐
//! │ Cross-compilation Environment                           │
//! │   CC=aarch64-linux-gnu-gcc                              │
//! │   CXX=aarch64-linux-gnu-g++                             │
//! │   LD=aarch64-linux-gnu-ld                               │
//! │   AR=aarch64-linux-gnu-ar                               │
//! │   TARGET_SYS=aarch64-poky-linux                         │
//! │   CFLAGS=--sysroot=/path/to/sysroot                     │
//! │   ...                                                   │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```no_run
//! use convenient_bitbake::toolchain::ToolchainManager;
//! use std::path::PathBuf;
//!
//! let manager = ToolchainManager::new();
//!
//! // Get toolchain for a specific machine
//! let toolchain = manager.for_machine("qemuarm64")?;
//!
//! // Generate environment variables for cross-compilation
//! let sysroot = PathBuf::from("/build/sysroot/qemuarm64");
//! let env = toolchain.generate_environment(&sysroot);
//!
//! // Use in task execution
//! // task_spec.env.extend(env);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during toolchain operations
#[derive(Debug, Error)]
pub enum ToolchainError {
    #[error("Toolchain not found for machine: {0}")]
    NotFound(String),

    #[error("Toolchain detection failed: {0}")]
    DetectionFailed(String),

    #[error("Invalid machine name: {0}")]
    InvalidMachine(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ToolchainResult<T> = Result<T, ToolchainError>;

/// Cross-compilation toolchain configuration
#[derive(Debug, Clone)]
pub struct Toolchain {
    /// Target machine (e.g., "qemuarm64")
    pub machine: String,

    /// Toolchain prefix (e.g., "aarch64-linux-gnu-")
    pub prefix: String,

    /// Target system triple (e.g., "aarch64-poky-linux")
    pub target_sys: String,

    /// Target architecture (e.g., "aarch64")
    pub target_arch: String,

    /// Path to toolchain binaries (if known)
    pub bin_path: Option<PathBuf>,

    /// Toolchain vendor (e.g., "poky", "gnu", "none")
    pub vendor: String,

    /// Operating system (e.g., "linux", "linux-musl")
    pub os: String,

    /// Additional CPU flags
    pub cpu_flags: Vec<String>,
}

impl Toolchain {
    /// Generate complete cross-compilation environment variables
    ///
    /// This generates all environment variables needed for cross-compilation,
    /// including compiler paths, flags, and sysroot configuration.
    pub fn generate_environment(&self, sysroot: &Path) -> HashMap<String, String> {
        let mut env = HashMap::new();

        // Compiler toolchain
        env.insert("CC".to_string(), format!("{}gcc", self.prefix));
        env.insert("CXX".to_string(), format!("{}g++", self.prefix));
        env.insert("CPP".to_string(), format!("{}gcc -E", self.prefix));
        env.insert("LD".to_string(), format!("{}ld", self.prefix));
        env.insert("AR".to_string(), format!("{}ar", self.prefix));
        env.insert("AS".to_string(), format!("{}as", self.prefix));
        env.insert("NM".to_string(), format!("{}nm", self.prefix));
        env.insert("RANLIB".to_string(), format!("{}ranlib", self.prefix));
        env.insert("OBJCOPY".to_string(), format!("{}objcopy", self.prefix));
        env.insert("OBJDUMP".to_string(), format!("{}objdump", self.prefix));
        env.insert("READELF".to_string(), format!("{}readelf", self.prefix));
        env.insert("STRIP".to_string(), format!("{}strip", self.prefix));

        // Build system identifiers
        env.insert("TARGET_SYS".to_string(), self.target_sys.clone());
        env.insert("TARGET_ARCH".to_string(), self.target_arch.clone());
        env.insert("TARGET_OS".to_string(), self.os.clone());
        env.insert("TARGET_VENDOR".to_string(), self.vendor.clone());

        // Sysroot configuration
        let sysroot_str = sysroot.to_string_lossy().to_string();
        env.insert("STAGING_DIR_HOST".to_string(), sysroot_str.clone());
        env.insert("PKG_CONFIG_SYSROOT_DIR".to_string(), sysroot_str.clone());

        // Compiler flags with sysroot
        let sysroot_flag = format!("--sysroot={}", sysroot_str);
        env.insert("CFLAGS".to_string(), sysroot_flag.clone());
        env.insert("CXXFLAGS".to_string(), sysroot_flag.clone());
        env.insert("CPPFLAGS".to_string(), sysroot_flag.clone());
        env.insert("LDFLAGS".to_string(), sysroot_flag.clone());

        // pkg-config configuration
        env.insert(
            "PKG_CONFIG_PATH".to_string(),
            format!("{}/usr/lib/pkgconfig:{}/usr/share/pkgconfig", sysroot_str, sysroot_str),
        );
        env.insert("PKG_CONFIG_LIBDIR".to_string(), format!("{}/usr/lib/pkgconfig", sysroot_str));

        // Build vs Host (for autotools)
        // BUILD_SYS is the machine doing the build (typically x86_64-linux)
        // HOST_SYS is the target machine we're cross-compiling for
        if let Ok(output) = Command::new("gcc").arg("-dumpmachine").output() {
            if output.status.success() {
                let build_sys = String::from_utf8_lossy(&output.stdout).trim().to_string();
                env.insert("BUILD_SYS".to_string(), build_sys);
            }
        }
        env.insert("HOST_SYS".to_string(), self.target_sys.clone());

        // Add bin path if known
        if let Some(bin_path) = &self.bin_path {
            env.insert(
                "CROSS_COMPILE_BIN".to_string(),
                bin_path.to_string_lossy().to_string(),
            );
        }

        debug!("Generated cross-compilation environment for {}", self.machine);
        debug!("  CC={}", env.get("CC").unwrap());
        debug!("  TARGET_SYS={}", env.get("TARGET_SYS").unwrap());
        debug!("  SYSROOT={}", sysroot_str);

        env
    }

    /// Get the compiler executable name (e.g., "aarch64-linux-gnu-gcc")
    pub fn compiler(&self) -> String {
        format!("{}gcc", self.prefix)
    }

    /// Get the C++ compiler executable name
    pub fn cxx_compiler(&self) -> String {
        format!("{}g++", self.prefix)
    }

    /// Check if this toolchain is available on the system
    pub fn is_available(&self) -> bool {
        Command::new(self.compiler())
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

/// Manager for cross-compilation toolchains
pub struct ToolchainManager {
    /// Known MACHINE → toolchain mappings
    machine_mappings: HashMap<String, Vec<ToolchainCandidate>>,
}

/// A candidate toolchain for a specific machine
#[derive(Debug, Clone)]
struct ToolchainCandidate {
    prefix: String,
    vendor: String,
    os: String,
    arch: String,
    priority: u32, // Higher = prefer this one
}

impl ToolchainManager {
    /// Create a new toolchain manager with default mappings
    pub fn new() -> Self {
        let mut manager = Self {
            machine_mappings: HashMap::new(),
        };

        manager.register_default_mappings();
        manager
    }

    /// Register default MACHINE → toolchain mappings
    fn register_default_mappings(&mut self) {
        // ARM 64-bit (AArch64)
        self.register_machine(
            "qemuarm64",
            vec![
                ToolchainCandidate {
                    prefix: "aarch64-poky-linux-".to_string(),
                    vendor: "poky".to_string(),
                    os: "linux".to_string(),
                    arch: "aarch64".to_string(),
                    priority: 100, // Prefer Yocto SDK toolchain
                },
                ToolchainCandidate {
                    prefix: "aarch64-linux-gnu-".to_string(),
                    vendor: "gnu".to_string(),
                    os: "linux".to_string(),
                    arch: "aarch64".to_string(),
                    priority: 80,
                },
                ToolchainCandidate {
                    prefix: "aarch64-none-linux-gnu-".to_string(),
                    vendor: "none".to_string(),
                    os: "linux".to_string(),
                    arch: "aarch64".to_string(),
                    priority: 70,
                },
            ],
        );

        // ARM 32-bit
        self.register_machine(
            "qemuarm",
            vec![
                ToolchainCandidate {
                    prefix: "arm-poky-linux-gnueabi-".to_string(),
                    vendor: "poky".to_string(),
                    os: "linux".to_string(),
                    arch: "arm".to_string(),
                    priority: 100,
                },
                ToolchainCandidate {
                    prefix: "arm-linux-gnueabihf-".to_string(),
                    vendor: "gnu".to_string(),
                    os: "linux".to_string(),
                    arch: "arm".to_string(),
                    priority: 80,
                },
                ToolchainCandidate {
                    prefix: "arm-none-linux-gnueabihf-".to_string(),
                    vendor: "none".to_string(),
                    os: "linux".to_string(),
                    arch: "arm".to_string(),
                    priority: 70,
                },
            ],
        );

        // x86-64 (typically native, but can be cross from ARM)
        self.register_machine(
            "qemux86-64",
            vec![
                ToolchainCandidate {
                    prefix: "x86_64-poky-linux-".to_string(),
                    vendor: "poky".to_string(),
                    os: "linux".to_string(),
                    arch: "x86_64".to_string(),
                    priority: 100,
                },
                ToolchainCandidate {
                    prefix: "x86_64-linux-gnu-".to_string(),
                    vendor: "gnu".to_string(),
                    os: "linux".to_string(),
                    arch: "x86_64".to_string(),
                    priority: 80,
                },
            ],
        );

        // RISC-V 64-bit
        self.register_machine(
            "qemuriscv64",
            vec![
                ToolchainCandidate {
                    prefix: "riscv64-poky-linux-".to_string(),
                    vendor: "poky".to_string(),
                    os: "linux".to_string(),
                    arch: "riscv64".to_string(),
                    priority: 100,
                },
                ToolchainCandidate {
                    prefix: "riscv64-linux-gnu-".to_string(),
                    vendor: "gnu".to_string(),
                    os: "linux".to_string(),
                    arch: "riscv64".to_string(),
                    priority: 80,
                },
            ],
        );

        // PowerPC 64-bit
        self.register_machine(
            "qemuppc64",
            vec![
                ToolchainCandidate {
                    prefix: "powerpc64-poky-linux-".to_string(),
                    vendor: "poky".to_string(),
                    os: "linux".to_string(),
                    arch: "powerpc64".to_string(),
                    priority: 100,
                },
                ToolchainCandidate {
                    prefix: "powerpc64-linux-gnu-".to_string(),
                    vendor: "gnu".to_string(),
                    os: "linux".to_string(),
                    arch: "powerpc64".to_string(),
                    priority: 80,
                },
            ],
        );

        // MIPS 64-bit
        self.register_machine(
            "qemumips64",
            vec![
                ToolchainCandidate {
                    prefix: "mips64-poky-linux-".to_string(),
                    vendor: "poky".to_string(),
                    os: "linux".to_string(),
                    arch: "mips64".to_string(),
                    priority: 100,
                },
                ToolchainCandidate {
                    prefix: "mips64-linux-gnu-".to_string(),
                    vendor: "gnu".to_string(),
                    os: "linux".to_string(),
                    arch: "mips64".to_string(),
                    priority: 80,
                },
            ],
        );
    }

    /// Register a machine with its toolchain candidates
    fn register_machine(&mut self, machine: &str, candidates: Vec<ToolchainCandidate>) {
        self.machine_mappings.insert(machine.to_string(), candidates);
    }

    /// Get toolchain for a specific machine
    ///
    /// This will:
    /// 1. Look up known toolchain candidates for the machine
    /// 2. Check which ones are actually installed
    /// 3. Return the highest priority available toolchain
    pub fn for_machine(&self, machine: &str) -> ToolchainResult<Toolchain> {
        let candidates = self
            .machine_mappings
            .get(machine)
            .ok_or_else(|| ToolchainError::NotFound(machine.to_string()))?;

        debug!("Looking for toolchain for machine: {}", machine);
        debug!("  Checking {} candidates", candidates.len());

        // Try each candidate in priority order
        let mut sorted_candidates = candidates.clone();
        sorted_candidates.sort_by(|a, b| b.priority.cmp(&a.priority));

        for candidate in sorted_candidates {
            let test_compiler = format!("{}gcc", candidate.prefix);
            debug!("  Trying: {}", test_compiler);

            if let Ok(output) = Command::new(&test_compiler).arg("--version").output() {
                if output.status.success() {
                    info!("✓ Found toolchain: {} (priority: {})", test_compiler, candidate.priority);

                    // Determine bin path by finding the compiler
                    let bin_path = which::which(&test_compiler).ok().and_then(|p| p.parent().map(|p| p.to_path_buf()));

                    let target_sys = format!("{}-{}-{}", candidate.arch, candidate.vendor, candidate.os);

                    return Ok(Toolchain {
                        machine: machine.to_string(),
                        prefix: candidate.prefix.clone(),
                        target_sys,
                        target_arch: candidate.arch.clone(),
                        bin_path,
                        vendor: candidate.vendor.clone(),
                        os: candidate.os.clone(),
                        cpu_flags: Vec::new(),
                    });
                }
            }
        }

        Err(ToolchainError::NotFound(format!(
            "No suitable toolchain found for machine '{}'. Tried: {}",
            machine,
            candidates
                .iter()
                .map(|c| format!("{}gcc", c.prefix))
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }

    /// Auto-detect all available cross-compilation toolchains on the system
    ///
    /// This scans common locations and PATH for cross-compiler binaries.
    pub fn detect_available(&self) -> Vec<String> {
        let mut found = Vec::new();

        for (machine, candidates) in &self.machine_mappings {
            for candidate in candidates {
                let compiler = format!("{}gcc", candidate.prefix);
                if Command::new(&compiler)
                    .arg("--version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
                {
                    found.push(machine.clone());
                    break; // Found one for this machine, move on
                }
            }
        }

        found
    }

    /// Get list of supported machines
    pub fn supported_machines(&self) -> Vec<String> {
        self.machine_mappings.keys().cloned().collect()
    }

    /// Check if a machine is supported
    pub fn is_supported(&self, machine: &str) -> bool {
        self.machine_mappings.contains_key(machine)
    }
}

impl Default for ToolchainManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toolchain_manager_creation() {
        let manager = ToolchainManager::new();
        let machines = manager.supported_machines();
        assert!(!machines.is_empty());
        assert!(machines.contains(&"qemuarm64".to_string()));
    }

    #[test]
    fn test_machine_support_check() {
        let manager = ToolchainManager::new();
        assert!(manager.is_supported("qemuarm64"));
        assert!(manager.is_supported("qemuarm"));
        assert!(!manager.is_supported("invalid-machine"));
    }

    #[test]
    fn test_toolchain_detection() {
        let manager = ToolchainManager::new();
        let available = manager.detect_available();
        // This test will vary based on what's installed
        // Just verify it doesn't panic
        println!("Available toolchains: {:?}", available);
    }

    #[test]
    fn test_environment_generation() {
        let toolchain = Toolchain {
            machine: "qemuarm64".to_string(),
            prefix: "aarch64-linux-gnu-".to_string(),
            target_sys: "aarch64-poky-linux".to_string(),
            target_arch: "aarch64".to_string(),
            bin_path: None,
            vendor: "poky".to_string(),
            os: "linux".to_string(),
            cpu_flags: Vec::new(),
        };

        let sysroot = PathBuf::from("/tmp/test-sysroot");
        let env = toolchain.generate_environment(&sysroot);

        assert_eq!(env.get("CC").unwrap(), "aarch64-linux-gnu-gcc");
        assert_eq!(env.get("CXX").unwrap(), "aarch64-linux-gnu-g++");
        assert_eq!(env.get("TARGET_SYS").unwrap(), "aarch64-poky-linux");
        assert_eq!(env.get("TARGET_ARCH").unwrap(), "aarch64");
        assert!(env.get("CFLAGS").unwrap().contains("--sysroot="));
    }

    #[test]
    fn test_toolchain_availability_check() {
        let toolchain = Toolchain {
            machine: "test".to_string(),
            prefix: "nonexistent-".to_string(),
            target_sys: "test-vendor-linux".to_string(),
            target_arch: "test".to_string(),
            bin_path: None,
            vendor: "vendor".to_string(),
            os: "linux".to_string(),
            cpu_flags: Vec::new(),
        };

        // Should return false for non-existent toolchain
        assert!(!toolchain.is_available());
    }
}
