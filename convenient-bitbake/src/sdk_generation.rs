//! SDK generation support for cross-compilation

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};

/// SDK configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkConfig {
    pub name: String,
    pub version: String,
    pub target_arch: String,
    pub host_arch: String,
    pub sysroot_path: PathBuf,
    pub toolchain_path: PathBuf,
}

/// SDK generator
pub struct SdkGenerator {
    config: SdkConfig,
}

impl SdkGenerator {
    pub fn new(config: SdkConfig) -> Self {
        Self { config }
    }

    /// Generate SDK tarball
    pub fn generate(&self, output: &Path) -> std::io::Result<SdkMetadata> {
        println!("Generating SDK: {}", self.config.name);

        // Create temporary SDK directory structure
        let temp_dir = output.join(format!("{}-{}-temp", self.config.name, self.config.version));
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir)?;
        }
        fs::create_dir_all(&temp_dir)?;

        let mut file_count = 0;
        let mut total_size = 0u64;

        // 1. Copy sysroot if it exists
        let sdk_sysroot = temp_dir.join("sysroot");
        if self.config.sysroot_path.exists() {
            fs::create_dir_all(&sdk_sysroot)?;
            if self.config.sysroot_path.is_dir() {
                let (count, size) = self.copy_dir_recursive(&self.config.sysroot_path, &sdk_sysroot)?;
                file_count += count;
                total_size += size;
            }
        }

        // 2. Copy toolchain if it exists
        let sdk_toolchain = temp_dir.join("toolchain");
        if self.config.toolchain_path.exists() {
            fs::create_dir_all(&sdk_toolchain)?;
            if self.config.toolchain_path.is_dir() {
                let (count, size) = self.copy_dir_recursive(&self.config.toolchain_path, &sdk_toolchain)?;
                file_count += count;
                total_size += size;
            }
        }

        // 3. Generate environment setup script
        let env_script = self.create_env_script();
        let env_script_path = temp_dir.join("environment-setup");
        let mut file = File::create(&env_script_path)?;
        file.write_all(env_script.as_bytes())?;
        file_count += 1;
        total_size += env_script.len() as u64;

        // Make the script executable on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&env_script_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&env_script_path, perms)?;
        }

        // 4. Package as tarball
        let tarball_name = format!("{}-{}.tar.gz", self.config.name, self.config.version);
        let tarball_path = output.join(&tarball_name);

        // Use tar command to create the tarball
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&tarball_path)
            .arg("-C")
            .arg(output)
            .arg(temp_dir.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "Invalid temp directory name")
            })?)
            .status();

        // Clean up temp directory
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir)?;
        }

        // Check if tar command succeeded
        match status {
            Ok(status) if status.success() => {
                // Get actual tarball size
                let tarball_metadata = fs::metadata(&tarball_path)?;
                let size_mb = tarball_metadata.len() / (1024 * 1024);

                println!("SDK generated: {} ({} MB, {} files)", tarball_path.display(), size_mb, file_count);

                Ok(SdkMetadata {
                    name: self.config.name.clone(),
                    version: self.config.version.clone(),
                    size_mb,
                    files: file_count,
                })
            }
            Ok(status) => {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("tar command failed with status: {}", status)
                ))
            }
            Err(e) => Err(e),
        }
    }

    /// Recursively copy directory contents, return (file_count, total_size)
    fn copy_dir_recursive(&self, src: &Path, dst: &Path) -> std::io::Result<(usize, u64)> {
        let mut file_count = 0;
        let mut total_size = 0u64;

        if !dst.exists() {
            fs::create_dir_all(dst)?;
        }

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                let (count, size) = self.copy_dir_recursive(&src_path, &dst_path)?;
                file_count += count;
                total_size += size;
            } else {
                fs::copy(&src_path, &dst_path)?;
                file_count += 1;
                if let Ok(metadata) = fs::metadata(&src_path) {
                    total_size += metadata.len();
                }
            }
        }

        Ok((file_count, total_size))
    }

    /// Create environment setup script
    pub fn create_env_script(&self) -> String {
        format!(r#"#!/bin/bash
# SDK environment setup
export SDKTARGETSYSROOT="{sysroot}"
export PATH="{toolchain}/bin:$PATH"
export CC="{target}-gcc"
export CXX="{target}-g++"
export AR="{target}-ar"
export LD="{target}-ld"

echo "SDK {name} {version} ready"
"#,
            sysroot = self.config.sysroot_path.display(),
            toolchain = self.config.toolchain_path.display(),
            target = self.config.target_arch,
            name = self.config.name,
            version = self.config.version,
        )
    }
}

/// SDK metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkMetadata {
    pub name: String,
    pub version: String,
    pub size_mb: u64,
    pub files: usize,
}
