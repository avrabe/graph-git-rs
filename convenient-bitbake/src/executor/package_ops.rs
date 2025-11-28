//! Package operations for BitBake task execution
//!
//! Provides Rust implementations for package splitting, ELF manipulation,
//! and kernel installation - replacing shell stubs with proper implementations.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Result type for package operations
pub type PackageResult<T> = Result<T, PackageError>;

/// Errors that can occur during package operations
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ELF parsing error: {0}")]
    ElfParse(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Package configuration error: {0}")]
    Config(String),
}

/// Package split configuration
#[derive(Debug, Clone)]
pub struct PackageSplitConfig {
    /// Base package name (PN)
    pub pn: String,
    /// Package version (PV)
    pub pv: String,
    /// Destination directory (D)
    pub destdir: PathBuf,
    /// Package output directory
    pub pkgdest: PathBuf,
    /// Files to include in each package (package name -> file patterns)
    pub package_files: HashMap<String, Vec<String>>,
}

/// Result of package splitting
#[derive(Debug, Default)]
pub struct PackageSplitResult {
    /// Created packages and their contents
    pub packages: HashMap<String, Vec<PathBuf>>,
    /// Total files processed
    pub total_files: usize,
    /// Warnings generated
    pub warnings: Vec<String>,
}

/// Split installed files into separate packages
///
/// This implements BitBake's `populate_packages` functionality:
/// - Main package: binaries, libraries
/// - -dev package: headers, static libs, pkg-config files
/// - -dbg package: debug symbols
/// - -doc package: documentation
/// - -locale package: locale files
pub fn populate_packages(config: &PackageSplitConfig) -> PackageResult<PackageSplitResult> {
    info!("Populating packages for {}-{}", config.pn, config.pv);

    let mut result = PackageSplitResult::default();

    // Ensure destination exists
    if !config.destdir.exists() {
        warn!("Destination directory does not exist: {}", config.destdir.display());
        return Ok(result);
    }

    // Create package directories
    let packages = vec![
        (format!("{}", config.pn), "main"),
        (format!("{}-dev", config.pn), "dev"),
        (format!("{}-dbg", config.pn), "dbg"),
        (format!("{}-doc", config.pn), "doc"),
        (format!("{}-locale", config.pn), "locale"),
        (format!("{}-staticdev", config.pn), "staticdev"),
    ];

    for (pkg_name, _pkg_type) in &packages {
        let pkg_dir = config.pkgdest.join(pkg_name);
        fs::create_dir_all(&pkg_dir)?;
        result.packages.insert(pkg_name.clone(), Vec::new());
    }

    // Walk through destdir and categorize files
    for entry in walkdir::WalkDir::new(&config.destdir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() && !entry.file_type().is_symlink() {
            continue;
        }

        let path = entry.path();
        let rel_path = path.strip_prefix(&config.destdir)
            .map_err(|_| PackageError::InvalidPath(path.display().to_string()))?;

        result.total_files += 1;

        // Categorize file based on path and extension
        let pkg_name = categorize_file(rel_path, &config.pn);
        let pkg_dir = config.pkgdest.join(&pkg_name);

        // Create parent directories and copy/link file
        let dest_path = pkg_dir.join(rel_path);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Hard link if possible, otherwise copy
        if fs::hard_link(path, &dest_path).is_err() {
            fs::copy(path, &dest_path)?;
        }

        if let Some(files) = result.packages.get_mut(&pkg_name) {
            files.push(rel_path.to_path_buf());
        }

        debug!("  {} -> {}", rel_path.display(), pkg_name);
    }

    info!("Package split complete: {} files into {} packages",
          result.total_files, result.packages.len());

    Ok(result)
}

/// Categorize a file into the appropriate package
fn categorize_file(rel_path: &Path, pn: &str) -> String {
    let path_str = rel_path.to_string_lossy();
    let extension = rel_path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // Debug symbols
    if path_str.contains("/.debug/") || extension == "debug" {
        return format!("{}-dbg", pn);
    }

    // Development files
    if path_str.starts_with("usr/include/")
        || path_str.starts_with("usr/lib/pkgconfig/")
        || path_str.starts_with("usr/share/pkgconfig/")
        || extension == "h"
        || extension == "hpp"
        || extension == "pc"
        || path_str.ends_with(".cmake")
        || path_str.contains("/cmake/")
    {
        return format!("{}-dev", pn);
    }

    // Static libraries
    if extension == "a" && !path_str.contains("firmware") {
        return format!("{}-staticdev", pn);
    }

    // Documentation
    if path_str.starts_with("usr/share/doc/")
        || path_str.starts_with("usr/share/man/")
        || path_str.starts_with("usr/share/info/")
        || extension == "md"
        || extension == "txt"
        || extension == "html"
    {
        return format!("{}-doc", pn);
    }

    // Locale files
    if path_str.starts_with("usr/share/locale/")
        || path_str.contains("/LC_MESSAGES/")
        || extension == "mo"
        || extension == "po"
    {
        return format!("{}-locale", pn);
    }

    // Everything else goes to main package
    pn.to_string()
}

/// ELF RPATH manipulation (chrpath replacement)
///
/// Modifies the RPATH/RUNPATH in ELF binaries to fix library search paths.
/// This is essential for cross-compilation where build paths differ from target paths.
#[derive(Debug)]
pub struct ChrpathResult {
    /// Original RPATH value
    pub old_rpath: Option<String>,
    /// New RPATH value (if changed)
    pub new_rpath: Option<String>,
    /// Whether the file was modified
    pub modified: bool,
}

/// Get the RPATH from an ELF file
pub fn get_rpath(path: &Path) -> PackageResult<Option<String>> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    match goblin::Object::parse(&buffer) {
        Ok(goblin::Object::Elf(elf)) => {
            // Look for DT_RPATH or DT_RUNPATH in dynamic section
            if let Some(dynamic) = &elf.dynamic {
                for dyn_entry in &dynamic.dyns {
                    match dyn_entry.d_tag {
                        goblin::elf::dynamic::DT_RPATH | goblin::elf::dynamic::DT_RUNPATH => {
                            if let Some(rpath) = elf.dynstrtab.get_at(dyn_entry.d_val as usize) {
                                return Ok(Some(rpath.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(None)
        }
        Ok(_) => Ok(None), // Not an ELF file
        Err(e) => {
            // Not a valid ELF or parsing error - just return None
            debug!("Not an ELF file or parse error for {}: {}", path.display(), e);
            Ok(None)
        }
    }
}

/// Delete RPATH from an ELF file
///
/// Note: This is a simplified implementation that zeros out the RPATH string.
/// A full implementation would need to modify the ELF structure.
pub fn delete_rpath(path: &Path) -> PackageResult<ChrpathResult> {
    let old_rpath = get_rpath(path)?;

    if old_rpath.is_none() {
        return Ok(ChrpathResult {
            old_rpath: None,
            new_rpath: None,
            modified: false,
        });
    }

    // Read the file
    let mut buffer = fs::read(path)?;

    // Parse to find RPATH location - collect offsets first to avoid borrow issues
    let file_offset = {
        let elf = match goblin::Object::parse(&buffer) {
            Ok(goblin::Object::Elf(elf)) => elf,
            _ => return Ok(ChrpathResult {
                old_rpath,
                new_rpath: None,
                modified: false,
            }),
        };

        let mut found_offset = None;

        // Find the string table offset for RPATH
        if let Some(dynamic) = &elf.dynamic {
            for dyn_entry in &dynamic.dyns {
                if dyn_entry.d_tag == goblin::elf::dynamic::DT_RPATH
                    || dyn_entry.d_tag == goblin::elf::dynamic::DT_RUNPATH
                {
                    let str_offset = dyn_entry.d_val as usize;
                    // Find actual file offset of the string in .dynstr section
                    if let Some(dynstr_section) = elf.section_headers.iter()
                        .find(|sh| sh.sh_type == goblin::elf::section_header::SHT_STRTAB
                            && elf.shdr_strtab.get_at(sh.sh_name).map(|n| n == ".dynstr").unwrap_or(false))
                    {
                        found_offset = Some(dynstr_section.sh_offset as usize + str_offset);
                        break;
                    }
                }
            }
        }
        found_offset
    };

    // Now modify the buffer (ELF borrow has been dropped)
    if let Some(offset) = file_offset {
        if offset < buffer.len() {
            let mut i = offset;
            while i < buffer.len() && buffer[i] != 0 {
                buffer[i] = 0;
                i += 1;
            }
        }
    }

    // Write back
    fs::write(path, &buffer)?;

    Ok(ChrpathResult {
        old_rpath,
        new_rpath: Some(String::new()),
        modified: true,
    })
}

/// Replace RPATH in an ELF file
///
/// The new RPATH must be shorter than or equal to the old RPATH length.
pub fn replace_rpath(path: &Path, new_rpath: &str) -> PackageResult<ChrpathResult> {
    let old_rpath = get_rpath(path)?;

    if old_rpath.is_none() {
        return Err(PackageError::ElfParse(
            format!("No RPATH found in {}", path.display())
        ));
    }

    let old_rpath_str = old_rpath.as_ref().unwrap();
    let old_rpath_len = old_rpath_str.len();

    // New RPATH must fit in the old space
    if new_rpath.len() > old_rpath_len {
        return Err(PackageError::ElfParse(
            format!("New RPATH ({} bytes) is longer than old RPATH ({} bytes)",
                    new_rpath.len(), old_rpath_len)
        ));
    }

    // Read the file
    let mut buffer = fs::read(path)?;

    // Parse to find RPATH location - collect offset first to avoid borrow issues
    let file_offset = {
        let elf = match goblin::Object::parse(&buffer) {
            Ok(goblin::Object::Elf(elf)) => elf,
            _ => return Err(PackageError::ElfParse("Failed to parse ELF".into())),
        };

        let mut found_offset = None;

        // Find the RPATH string offset
        if let Some(dynamic) = &elf.dynamic {
            for dyn_entry in &dynamic.dyns {
                if dyn_entry.d_tag == goblin::elf::dynamic::DT_RPATH
                    || dyn_entry.d_tag == goblin::elf::dynamic::DT_RUNPATH
                {
                    let str_offset = dyn_entry.d_val as usize;
                    if let Some(dynstr_section) = elf.section_headers.iter()
                        .find(|sh| sh.sh_type == goblin::elf::section_header::SHT_STRTAB
                            && elf.shdr_strtab.get_at(sh.sh_name).map(|n| n == ".dynstr").unwrap_or(false))
                    {
                        found_offset = Some(dynstr_section.sh_offset as usize + str_offset);
                        break;
                    }
                }
            }
        }
        found_offset
    };

    // Now modify the buffer (ELF borrow has been dropped)
    if let Some(offset) = file_offset {
        if offset + new_rpath.len() < buffer.len() {
            // Write new RPATH
            for (i, byte) in new_rpath.bytes().enumerate() {
                buffer[offset + i] = byte;
            }
            // Null terminate and pad
            for i in new_rpath.len()..=old_rpath_len {
                if offset + i < buffer.len() {
                    buffer[offset + i] = 0;
                }
            }
        }
    }

    // Write back
    fs::write(path, &buffer)?;

    Ok(ChrpathResult {
        old_rpath,
        new_rpath: Some(new_rpath.to_string()),
        modified: true,
    })
}

/// List RPATH for an ELF file (like chrpath -l)
pub fn list_rpath(path: &Path) -> PackageResult<String> {
    match get_rpath(path)? {
        Some(rpath) => Ok(format!("{}: RPATH={}", path.display(), rpath)),
        None => Ok(format!("{}: no RPATH or RUNPATH", path.display())),
    }
}

/// Kernel installation configuration
#[derive(Debug, Clone)]
pub struct KernelInstallConfig {
    /// Kernel source directory
    pub kernel_src: PathBuf,
    /// Destination directory (D)
    pub destdir: PathBuf,
    /// Kernel version string
    pub kernel_version: String,
    /// Kernel image type (Image, zImage, bzImage, etc.)
    pub kernel_imagetype: String,
    /// Deploy directory for bootable images
    pub deploydir: PathBuf,
}

/// Result of kernel installation
#[derive(Debug, Default)]
pub struct KernelInstallResult {
    /// Installed kernel image path
    pub kernel_image: Option<PathBuf>,
    /// Installed modules
    pub modules: Vec<PathBuf>,
    /// Installed device tree blobs
    pub dtbs: Vec<PathBuf>,
    /// Warnings
    pub warnings: Vec<String>,
}

/// Install kernel image, modules, and device trees
pub fn kernel_do_install(config: &KernelInstallConfig) -> PackageResult<KernelInstallResult> {
    info!("Installing kernel {}", config.kernel_version);

    let mut result = KernelInstallResult::default();

    // Create destination directories
    let boot_dir = config.destdir.join("boot");
    let lib_modules = config.destdir.join("lib/modules").join(&config.kernel_version);
    fs::create_dir_all(&boot_dir)?;
    fs::create_dir_all(&lib_modules)?;
    fs::create_dir_all(&config.deploydir)?;

    // Find and install kernel image
    let kernel_image_names = [
        format!("arch/arm64/boot/{}", config.kernel_imagetype),
        format!("arch/arm/boot/{}", config.kernel_imagetype),
        format!("arch/x86/boot/{}", config.kernel_imagetype),
        format!("{}", config.kernel_imagetype),
    ];

    for image_name in &kernel_image_names {
        let image_path = config.kernel_src.join(image_name);
        if image_path.exists() {
            let dest_name = format!("{}-{}", config.kernel_imagetype, config.kernel_version);
            let dest_path = boot_dir.join(&dest_name);
            fs::copy(&image_path, &dest_path)?;
            info!("Installed kernel image: {}", dest_path.display());

            // Also copy to deploy dir
            let deploy_path = config.deploydir.join(&dest_name);
            fs::copy(&image_path, &deploy_path)?;

            result.kernel_image = Some(dest_path);
            break;
        }
    }

    if result.kernel_image.is_none() {
        result.warnings.push(format!(
            "Kernel image {} not found in {}",
            config.kernel_imagetype, config.kernel_src.display()
        ));
    }

    // Install modules (if modules_install was run)
    let modules_dir = config.kernel_src.join("modules_install");
    if modules_dir.exists() {
        for entry in walkdir::WalkDir::new(&modules_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "ko").unwrap_or(false))
        {
            let rel_path = entry.path().strip_prefix(&modules_dir)
                .unwrap_or(entry.path());
            let dest_path = lib_modules.join(rel_path);
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &dest_path)?;
            result.modules.push(dest_path);
        }
    }

    // Install device tree blobs
    let dtb_dirs = [
        config.kernel_src.join("arch/arm64/boot/dts"),
        config.kernel_src.join("arch/arm/boot/dts"),
    ];

    for dtb_dir in &dtb_dirs {
        if dtb_dir.exists() {
            for entry in walkdir::WalkDir::new(dtb_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|ext| ext == "dtb").unwrap_or(false))
            {
                let file_name = entry.path().file_name().unwrap();
                let dest_path = boot_dir.join(file_name);
                fs::copy(entry.path(), &dest_path)?;

                // Also to deploy dir
                let deploy_path = config.deploydir.join(file_name);
                fs::copy(entry.path(), &deploy_path)?;

                result.dtbs.push(dest_path);
            }
        }
    }

    info!("Kernel install complete: image={}, {} modules, {} DTBs",
          result.kernel_image.is_some(),
          result.modules.len(),
          result.dtbs.len());

    Ok(result)
}

/// Kernel module installation configuration
#[derive(Debug, Clone)]
pub struct ModuleInstallConfig {
    /// Module source directory
    pub module_src: PathBuf,
    /// Destination directory
    pub destdir: PathBuf,
    /// Kernel version
    pub kernel_version: String,
}

/// Install out-of-tree kernel modules
pub fn module_do_install(config: &ModuleInstallConfig) -> PackageResult<Vec<PathBuf>> {
    info!("Installing kernel modules for {}", config.kernel_version);

    let mut installed = Vec::new();
    let modules_dir = config.destdir
        .join("lib/modules")
        .join(&config.kernel_version)
        .join("extra");

    fs::create_dir_all(&modules_dir)?;

    // Find .ko files in source directory
    for entry in walkdir::WalkDir::new(&config.module_src)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "ko").unwrap_or(false))
    {
        let file_name = entry.path().file_name().unwrap();
        let dest_path = modules_dir.join(file_name);
        fs::copy(entry.path(), &dest_path)?;
        info!("Installed module: {}", dest_path.display());
        installed.push(dest_path);
    }

    // Run depmod equivalent (create modules.dep)
    let modules_dep = config.destdir
        .join("lib/modules")
        .join(&config.kernel_version)
        .join("modules.dep");

    let mut dep_content = String::new();
    for module in &installed {
        if let Some(name) = module.file_name() {
            dep_content.push_str(&format!("extra/{}:\n", name.to_string_lossy()));
        }
    }
    fs::write(&modules_dep, dep_content)?;

    info!("Installed {} kernel modules", installed.len());

    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_categorize_file() {
        assert_eq!(categorize_file(Path::new("usr/include/foo.h"), "test"), "test-dev");
        assert_eq!(categorize_file(Path::new("usr/lib/libfoo.a"), "test"), "test-staticdev");
        assert_eq!(categorize_file(Path::new("usr/share/doc/README"), "test"), "test-doc");
        assert_eq!(categorize_file(Path::new("usr/share/locale/en/LC_MESSAGES/foo.mo"), "test"), "test-locale");
        assert_eq!(categorize_file(Path::new("usr/bin/foo"), "test"), "test");
        assert_eq!(categorize_file(Path::new("usr/lib/.debug/libfoo.so"), "test"), "test-dbg");
    }

    #[test]
    fn test_list_rpath_non_elf() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.txt");
        fs::write(&file, "not an elf file").unwrap();

        let result = list_rpath(&file).unwrap();
        assert!(result.contains("no RPATH"));
    }
}
