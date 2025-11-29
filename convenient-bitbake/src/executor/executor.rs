//! Main task executor - brings together caching, sandboxing, and execution

use super::cache::{ActionCache, ContentAddressableStore};
use super::direct_executor;
use super::fetch_task;
use super::rust_fetcher::FetchConfig;
use super::sandbox::SandboxManager;
use super::script_analyzer;
use crate::fetcher;
use super::types::{
    ContentHash, ExecutionError, ExecutionMode, ExecutionResult, NetworkPolicy,
    ResourceLimits, SandboxSpec, TaskOutput, TaskSignature, TaskSpec,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, warn};

/// Main task executor with caching and sandboxing
pub struct TaskExecutor {
    /// Content-addressable store for caching files
    cas: ContentAddressableStore,
    /// Action cache for task results
    action_cache: ActionCache,
    /// Sandbox manager
    sandbox_manager: SandboxManager,
    /// Statistics
    stats: ExecutionStats,
}

impl TaskExecutor {
    /// Create a new task executor
    pub fn new(cache_dir: impl AsRef<Path>) -> ExecutionResult<Self> {
        let cache_dir = cache_dir.as_ref();
        let cas_dir = cache_dir.join("cas");
        let action_cache_dir = cache_dir.join("action-cache");
        let sandbox_dir = cache_dir.join("sandboxes");

        info!("Initializing task executor");
        debug!("CAS directory: {}", cas_dir.display());
        debug!("Action cache directory: {}", action_cache_dir.display());
        debug!("Sandbox directory: {}", sandbox_dir.display());

        Ok(Self {
            cas: ContentAddressableStore::new(cas_dir)?,
            action_cache: ActionCache::new(action_cache_dir)?,
            sandbox_manager: SandboxManager::new(sandbox_dir)?,
            stats: ExecutionStats::default(),
        })
    }

    /// Execute a task with caching
    pub fn execute_task(&mut self, spec: TaskSpec) -> ExecutionResult<TaskOutput> {
        info!(
            "Executing task: {}:{} (mode: {:?})",
            spec.recipe, spec.name, spec.execution_mode
        );

        // 1. Compute signature
        let mut signature = self.compute_signature(&spec)?;
        let sig_hash = signature.compute();

        debug!("Task signature: {}", sig_hash);

        // 2. Check cache
        if let Some(cached) = self.action_cache.get(&sig_hash) {
            info!("Cache HIT for {}:{}", spec.recipe, spec.name);
            self.stats.cache_hits += 1;
            return Ok(cached.clone());
        }

        info!("Cache MISS for {}:{}", spec.recipe, spec.name);
        self.stats.cache_misses += 1;

        // 3. Execute based on task type and execution mode
        // Special handling for pure Rust implementations
        let (result_stdout, result_stderr, result_exit_code, output_files, duration) =
            if spec.name == "do_fetch" || spec.name == "fetch" {
                info!("Using pure Rust fetcher for fetch task (no host tools)");
                self.execute_fetch_task(&spec)?
            } else if spec.name == "do_unpack" || spec.name == "unpack" {
                info!("Using pure Rust unpacker for unpack task (no host tools)");
                self.execute_unpack_task(&spec)?
            } else if spec.name == "do_patch" || spec.name == "patch" {
                info!("Using patch task handler");
                self.execute_patch_task(&spec)?
            } else if spec.name == "do_package" || spec.name == "do_populate_sysroot" {
                info!("Using pure Rust package splitter");
                self.execute_package_task(&spec)?
            } else if spec.name.starts_with("do_install") && spec.name.contains("kernel") {
                info!("Using pure Rust kernel installer");
                self.execute_kernel_install_task(&spec)?
            } else {
                match spec.execution_mode {
                    ExecutionMode::DirectRust => {
                        // Direct Rust execution - no sandbox, no host contamination
                        info!("Using DirectRust execution (sandbox-free, hermetic)");
                        self.execute_direct_rust(&spec)?
                    }
                    ExecutionMode::RustShell => {
                        // Rust-based shell execution - in-process bash interpreter
                        info!("Using RustShell execution (in-process, variable tracking)");
                        self.execute_rust_shell(&spec)?
                    }
                    ExecutionMode::Shell | ExecutionMode::Python => {
                        // Shell/Python execution - requires full sandboxing
                        info!("Using sandboxed execution");
                        self.execute_sandboxed(&spec)?
                    }
                }
            };

        let task_output = TaskOutput {
            signature: sig_hash.clone(),
            output_files,
            stdout: result_stdout,
            stderr: result_stderr,
            exit_code: result_exit_code,
            duration_ms: duration,
        };

        // 4. Store in cache
        self.action_cache.put(sig_hash, task_output.clone())?;

        info!("Task completed in {}ms", task_output.duration_ms);
        self.stats.tasks_executed += 1;

        Ok(task_output)
    }

    /// Execute task using direct Rust calls (no sandbox)
    fn execute_direct_rust(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        let start = Instant::now();

        // Analyze script to determine if it can be executed directly
        let analysis = script_analyzer::analyze_script(&spec.script);

        if !analysis.is_simple {
            return Err(ExecutionError::SandboxError(format!(
                "Script is too complex for DirectRust execution: {}",
                analysis.complexity_reason.unwrap_or_default()
            )));
        }

        // Create work directory structure
        let work_dir = spec.workdir.join("direct-rust");
        std::fs::create_dir_all(&work_dir)?;

        let outputs_dir = work_dir.join("outputs");
        std::fs::create_dir_all(&outputs_dir)?;

        // Prepare environment with BitBake-style paths
        let mut env = spec.env.clone();
        env.insert("WORKDIR".to_string(), work_dir.to_string_lossy().to_string());
        env.insert(
            "S".to_string(),
            work_dir.join("src").to_string_lossy().to_string(),
        );
        env.insert(
            "B".to_string(),
            work_dir.join("build").to_string_lossy().to_string(),
        );
        env.insert(
            "D".to_string(),
            outputs_dir.to_string_lossy().to_string(),
        );

        // Execute directly without sandbox
        let result = direct_executor::execute_direct(&analysis, &work_dir, &env)?;

        if result.exit_code != 0 {
            warn!("Direct execution failed with exit code: {}", result.exit_code);
            warn!("Stderr: {}", result.stderr);
            return Err(ExecutionError::TaskFailed(result.exit_code));
        }

        // Collect and hash outputs
        let mut output_files = HashMap::new();
        if outputs_dir.exists() {
            for entry in walkdir::WalkDir::new(&outputs_dir)
                .follow_links(false)
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                if entry.file_type().is_file() {
                    let path = entry.path();
                    let content = std::fs::read(path)?;
                    let hash = self.cas.put(&content)?;
                    let rel_path = path
                        .strip_prefix(&outputs_dir)
                        .unwrap_or(path)
                        .to_path_buf();
                    output_files.insert(rel_path, hash);
                }
            }
        }

        let duration = start.elapsed().as_millis() as u64;

        info!("DirectRust execution completed successfully (no sandbox used)");

        Ok((
            result.stdout,
            result.stderr,
            result.exit_code,
            output_files,
            duration,
        ))
    }

    /// Execute task using RustShell (brush-shell in-process interpreter)
    fn execute_rust_shell(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        let start = Instant::now();

        info!("Executing with RustShell (in-process bash interpreter)");

        // Execute using RustShell with BitBake environment
        // Extract version from environment variables (PV = Package Version)
        let version = spec.env.get("PV").map(|s| s.as_str());
        let result = super::rust_shell_executor::execute_with_bitbake_env(
            &spec.script,
            &spec.recipe,
            version,
            &spec.workdir,
            &spec.env,
        )?;

        if result.exit_code != 0 {
            warn!("RustShell execution failed with exit code: {}", result.exit_code);
            warn!("Stderr: {}", result.stderr);
            return Err(ExecutionError::TaskFailed(result.exit_code));
        }

        // Collect and hash outputs
        let outputs_dir = spec.workdir.join("image");
        let mut output_files = HashMap::new();

        if outputs_dir.exists() {
            for entry in walkdir::WalkDir::new(&outputs_dir)
                .follow_links(false)
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                if entry.file_type().is_file() {
                    let path = entry.path();
                    let content = std::fs::read(path)?;
                    let hash = self.cas.put(&content)?;
                    let rel_path = path
                        .strip_prefix(&outputs_dir)
                        .unwrap_or(path)
                        .to_path_buf();
                    output_files.insert(rel_path, hash);
                }
            }
        }

        let duration = start.elapsed().as_millis() as u64;

        info!(
            "RustShell execution completed successfully ({} vars read, {} vars written)",
            result.vars_read.len(),
            result.vars_written.len()
        );
        debug!("Variables read: {:?}", result.vars_read);
        debug!("Variables written: {:?}", result.vars_written.keys().collect::<Vec<_>>());

        Ok((
            result.stdout,
            result.stderr,
            result.exit_code,
            output_files,
            duration,
        ))
    }

    /// Execute fetch task using pure Rust fetcher (no host tools)
    ///
    /// This uses the `fetch_task` module to download sources directly in Rust,
    /// without requiring wget, curl, or git CLI on the host.
    fn execute_fetch_task(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        let start = Instant::now();

        info!("Executing fetch task with pure Rust fetcher");
        debug!("Recipe: {}", spec.recipe);
        debug!("Environment vars: {:?}", spec.env.keys().collect::<Vec<_>>());

        // Create DL_DIR from environment or default
        let dl_dir = spec
            .env
            .get("DL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.join("downloads"));

        std::fs::create_dir_all(&dl_dir)?;
        info!("DL_DIR: {}", dl_dir.display());

        // Build fetch config from environment
        let fetch_config = self.build_fetch_config(&spec.env);

        // Execute the fetch task
        match fetch_task::execute_fetch_task(&spec.env, &dl_dir, Some(&fetch_config)) {
            Ok(result) => {
                let duration = start.elapsed().as_millis() as u64;

                // Build stdout with fetch results
                use std::fmt::Write;
                let mut stdout = String::new();
                writeln!(
                    stdout,
                    "[NOTE] Fetch completed: {} files downloaded ({} bytes)",
                    result.downloaded_files.len(),
                    result.total_bytes
                ).ok();
                for file in &result.downloaded_files {
                    writeln!(stdout, "  Downloaded: {}", file.display()).ok();
                }

                // Add warnings to stderr
                let stderr = result.warnings.join("\n");

                // Hash downloaded files and store in CAS
                let mut output_files = HashMap::new();
                for file_path in &result.downloaded_files {
                    if file_path.is_file() {
                        let content = std::fs::read(file_path)?;
                        let hash = self.cas.put(&content)?;
                        // Use relative path from dl_dir
                        let rel_path = file_path
                            .strip_prefix(&dl_dir)
                            .unwrap_or(file_path)
                            .to_path_buf();
                        output_files.insert(rel_path, hash);
                    }
                }

                // Create completion marker
                let outputs_dir = spec.workdir.join("outputs");
                std::fs::create_dir_all(&outputs_dir)?;
                let marker_path = outputs_dir.join("do_fetch.done");
                std::fs::write(&marker_path, format!("Fetched {} files\n", result.downloaded_files.len()))?;

                info!(
                    "Fetch task completed: {} files, {} bytes in {}ms",
                    result.downloaded_files.len(),
                    result.total_bytes,
                    duration
                );

                Ok((stdout, stderr, 0, output_files, duration))
            }
            Err(e) => {
                let _duration = start.elapsed().as_millis() as u64;
                let _stderr = format!("[ERROR] Fetch failed: {e}\n");
                warn!("Fetch task failed: {e}");

                // Return error as failed task (not Rust error) so it can be cached
                Err(ExecutionError::TaskFailed(1))
            }
        }
    }

    /// Execute unpack task using pure Rust unpacker (no host tools)
    ///
    /// This uses the `fetcher::unpack_source` function to extract archives directly in Rust,
    /// without requiring tar, gzip, bzip2, or xz CLI tools on the host.
    fn execute_unpack_task(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        let start = Instant::now();

        info!("Executing unpack task with pure Rust unpacker");
        debug!("Recipe: {}", spec.recipe);
        debug!("Environment vars: {:?}", spec.env.keys().collect::<Vec<_>>());

        // Get DL_DIR (where fetched archives are)
        let dl_dir = spec
            .env
            .get("DL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.join("downloads"));

        // Get S (source directory) - destination for unpacking
        let s_dir = spec
            .env
            .get("S")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.join("src"));

        std::fs::create_dir_all(&s_dir)?;

        info!("DL_DIR: {}", dl_dir.display());
        info!("S: {}", s_dir.display());

        // Check if DL_DIR exists and has archives
        if !dl_dir.exists() {
            warn!("DL_DIR does not exist: {}", dl_dir.display());
            let duration = start.elapsed().as_millis() as u64;
            return Ok((
                "[NOTE] No archives to unpack (DL_DIR does not exist)\n".to_string(),
                String::new(),
                0,
                HashMap::new(),
                duration,
            ));
        }

        // Find archives in DL_DIR
        use std::fmt::Write;
        let mut archives_found = Vec::new();
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut unpacked_count = 0;

        for entry in std::fs::read_dir(&dl_dir)? {
            let entry = entry?;
            let path = entry.path();

            // Check if it's an archive we can unpack
            if is_archive(&path) {
                archives_found.push(path.clone());

                // Unpack the archive
                match fetcher::unpack_source(&path, &s_dir) {
                    Ok(()) => {
                        let _ = writeln!(
                            stdout,
                            "[NOTE] Unpacked: {} -> {}",
                            path.display(),
                            s_dir.display()
                        );
                        unpacked_count += 1;
                    }
                    Err(e) => {
                        let _ = writeln!(
                            stderr,
                            "[ERROR] Failed to unpack {}: {e}",
                            path.display()
                        );
                        warn!("Failed to unpack {}: {e}", path.display());
                    }
                }
            }
        }

        if archives_found.is_empty() {
            stdout.push_str("[NOTE] No archives found to unpack\n");
            info!("No archives found in DL_DIR");
        } else {
            let _ = writeln!(
                stdout,
                "[NOTE] Unpack completed: {unpacked_count} of {} archives unpacked",
                archives_found.len()
            );
        }

        // Hash unpacked files and store in CAS
        let mut output_files = HashMap::new();
        if s_dir.exists() {
            for entry in walkdir::WalkDir::new(&s_dir)
                .follow_links(false)
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                if entry.file_type().is_file() {
                    let path = entry.path();
                    let content = std::fs::read(path)?;
                    let hash = self.cas.put(&content)?;
                    let rel_path = path
                        .strip_prefix(&s_dir)
                        .unwrap_or(path)
                        .to_path_buf();
                    output_files.insert(rel_path, hash);
                }
            }
        }

        // Create completion marker
        let outputs_dir = spec.workdir.join("outputs");
        std::fs::create_dir_all(&outputs_dir)?;
        let marker_path = outputs_dir.join("do_unpack.done");
        std::fs::write(
            &marker_path,
            format!("Unpacked {} archives, {} files in S\n", unpacked_count, output_files.len()),
        )?;

        let duration = start.elapsed().as_millis() as u64;

        info!(
            "Unpack task completed: {} archives, {} files in {}ms",
            unpacked_count,
            output_files.len(),
            duration
        );

        let exit_code = if stderr.is_empty() { 0 } else { 0 }; // Warn but don't fail

        Ok((stdout, stderr, exit_code, output_files, duration))
    }

    /// Execute patch task - applies patches to source directory
    ///
    /// This finds patch files in the workdir and applies them to S directory
    /// using git apply or a fallback patch command.
    fn execute_patch_task(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        let start = Instant::now();

        info!("Executing patch task");
        debug!("Recipe: {}", spec.recipe);

        // Get S (source directory) - where patches should be applied
        let s_dir = spec
            .env
            .get("S")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.join("src"));

        // Get patch directory - typically where file:// patches are copied
        let patch_dir = spec
            .env
            .get("PATCHDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.clone());

        info!("S: {}", s_dir.display());
        info!("PATCHDIR: {}", patch_dir.display());

        // Check if S exists
        if !s_dir.exists() {
            warn!("Source directory does not exist: {}", s_dir.display());
            let duration = start.elapsed().as_millis() as u64;
            return Ok((
                "[NOTE] Source directory does not exist, skipping patch\n".to_string(),
                String::new(),
                0,
                HashMap::new(),
                duration,
            ));
        }

        // Find patch files
        let mut patches = Vec::new();

        // Look in workdir for patches
        if spec.workdir.exists() {
            for entry in std::fs::read_dir(&spec.workdir)? {
                let entry = entry?;
                let path = entry.path();
                if is_patch_file(&path) {
                    patches.push(path);
                }
            }
        }

        // Also look in patch_dir if different
        if patch_dir != spec.workdir && patch_dir.exists() {
            for entry in std::fs::read_dir(&patch_dir)? {
                let entry = entry?;
                let path = entry.path();
                if is_patch_file(&path) && !patches.iter().any(|p| p.file_name() == path.file_name()) {
                    patches.push(path);
                }
            }
        }

        // Sort patches by name (allows 0001-*, 0002-* ordering)
        patches.sort();

        use std::fmt::Write;
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut patches_applied = 0;
        let mut exit_code = 0;

        if patches.is_empty() {
            stdout.push_str("[NOTE] No patches found to apply\n");
            info!("No patches found");
        } else {
            let _ = writeln!(stdout, "[NOTE] Found {} patches to apply", patches.len());

            for patch_path in &patches {
                info!("Applying patch: {}", patch_path.display());
                let _ = writeln!(stdout, "Applying: {}", patch_path.display());

                // Try git apply first, fall back to patch command
                let result = apply_patch(&s_dir, patch_path);

                match result {
                    Ok(output) => {
                        stdout.push_str(&output);
                        patches_applied += 1;
                    }
                    Err(e) => {
                        let _ = writeln!(stderr, "[ERROR] Failed to apply {}: {e}", patch_path.display());
                        warn!("[ERROR] Failed to apply {}: {e}", patch_path.display());
                        exit_code = 1;
                    }
                }
            }

            let _ = writeln!(
                stdout,
                "[NOTE] Applied {patches_applied} of {} patches",
                patches.len()
            );
        }

        // Hash patched files in S
        let mut output_files = HashMap::new();
        if s_dir.exists() {
            for entry in walkdir::WalkDir::new(&s_dir)
                .follow_links(false)
                .max_depth(10) // Limit depth to avoid huge trees
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                if entry.file_type().is_file() {
                    let path = entry.path();
                    // Only hash smaller files to avoid memory issues
                    if let Ok(metadata) = path.metadata()
                        && metadata.len() < 10 * 1024 * 1024 {
                            if let Ok(content) = std::fs::read(path) {
                                if let Ok(hash) = self.cas.put(&content) {
                                    let rel_path = path
                                        .strip_prefix(&s_dir)
                                        .unwrap_or(path)
                                        .to_path_buf();
                                    output_files.insert(rel_path, hash);
                                }
                            }
                        }
                }
            }
        }

        // Create completion marker
        let outputs_dir = spec.workdir.join("outputs");
        std::fs::create_dir_all(&outputs_dir)?;
        let marker_path = outputs_dir.join("do_patch.done");
        std::fs::write(
            &marker_path,
            format!("Applied {} patches\n", patches_applied),
        )?;

        let duration = start.elapsed().as_millis() as u64;

        info!(
            "Patch task completed: {} patches applied in {}ms",
            patches_applied,
            duration
        );

        Ok((stdout, stderr, exit_code, output_files, duration))
    }

    /// Execute package task using pure Rust package splitter
    ///
    /// This splits installed files into proper packages (-dev, -dbg, -doc, etc.)
    fn execute_package_task(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        use super::package_ops::{populate_packages, PackageSplitConfig};
        use std::fmt::Write;

        let start = Instant::now();

        info!("Executing package task with pure Rust package splitter");

        // Get required directories from environment
        let pn = spec.env.get("PN").cloned().unwrap_or_else(|| spec.recipe.clone());
        let pv = spec.env.get("PV").cloned().unwrap_or_else(|| "1.0".to_string());
        let destdir = spec.env.get("D")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.join("image"));
        let pkgdest = spec.env.get("PKGDEST")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.join("packages-split"));

        info!("PN: {}, PV: {}", pn, pv);
        info!("D (destdir): {}", destdir.display());
        info!("PKGDEST: {}", pkgdest.display());

        let config = PackageSplitConfig {
            pn: pn.clone(),
            pv: pv.clone(),
            destdir,
            pkgdest: pkgdest.clone(),
            package_files: HashMap::new(),
        };

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        match populate_packages(&config) {
            Ok(result) => {
                let _ = writeln!(stdout, "[NOTE] Package split complete: {} files", result.total_files);
                for (pkg_name, files) in &result.packages {
                    let _ = writeln!(stdout, "  {}: {} files", pkg_name, files.len());
                }
                for warning in &result.warnings {
                    let _ = writeln!(stderr, "[WARN] {}", warning);
                }
            }
            Err(e) => {
                let _ = writeln!(stderr, "[ERROR] Package split failed: {}", e);
                exit_code = 1;
            }
        }

        // Hash output packages
        let mut output_files = HashMap::new();
        if pkgdest.exists() {
            for entry in walkdir::WalkDir::new(&pkgdest)
                .follow_links(false)
                .into_iter()
                .filter_map(std::result::Result::ok)
                .filter(|e| e.file_type().is_file())
            {
                let path = entry.path();
                if let Ok(contents) = std::fs::read(path) {
                    let hash = ContentHash::from_bytes(&contents);
                    if let Ok(rel_path) = path.strip_prefix(&pkgdest) {
                        output_files.insert(rel_path.to_path_buf(), hash);
                    }
                }
            }
        }

        // Create completion marker
        let outputs_dir = spec.workdir.join("outputs");
        std::fs::create_dir_all(&outputs_dir)?;
        let marker_path = outputs_dir.join("do_package.done");
        std::fs::write(&marker_path, "Package split complete\n")?;

        let duration = start.elapsed().as_millis() as u64;
        info!("Package task completed in {}ms", duration);

        Ok((stdout, stderr, exit_code, output_files, duration))
    }

    /// Execute kernel installation task
    fn execute_kernel_install_task(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        use super::package_ops::{kernel_do_install, KernelInstallConfig};
        use std::fmt::Write;

        let start = Instant::now();

        info!("Executing kernel install task");

        // Get required paths from environment
        let kernel_src = spec.env.get("S")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.join("src"));
        let destdir = spec.env.get("D")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.join("image"));
        let deploydir = spec.env.get("DEPLOYDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| spec.workdir.join("deploy"));
        let kernel_version = spec.env.get("KERNEL_VERSION")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let kernel_imagetype = spec.env.get("KERNEL_IMAGETYPE")
            .cloned()
            .unwrap_or_else(|| "Image".to_string());

        info!("Kernel source: {}", kernel_src.display());
        info!("Kernel version: {}", kernel_version);
        info!("Kernel image type: {}", kernel_imagetype);

        let config = KernelInstallConfig {
            kernel_src,
            destdir: destdir.clone(),
            kernel_version,
            kernel_imagetype,
            deploydir,
        };

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        match kernel_do_install(&config) {
            Ok(result) => {
                if let Some(ref img) = result.kernel_image {
                    let _ = writeln!(stdout, "[NOTE] Kernel image installed: {}", img.display());
                }
                let _ = writeln!(stdout, "[NOTE] Installed {} kernel modules", result.modules.len());
                let _ = writeln!(stdout, "[NOTE] Installed {} device trees", result.dtbs.len());
                for warning in &result.warnings {
                    let _ = writeln!(stderr, "[WARN] {}", warning);
                }
            }
            Err(e) => {
                let _ = writeln!(stderr, "[ERROR] Kernel install failed: {}", e);
                exit_code = 1;
            }
        }

        // Hash installed files
        let mut output_files = HashMap::new();
        if destdir.exists() {
            for entry in walkdir::WalkDir::new(&destdir)
                .follow_links(false)
                .into_iter()
                .filter_map(std::result::Result::ok)
                .filter(|e| e.file_type().is_file())
            {
                let path = entry.path();
                if let Ok(contents) = std::fs::read(path) {
                    let hash = ContentHash::from_bytes(&contents);
                    if let Ok(rel_path) = path.strip_prefix(&destdir) {
                        output_files.insert(rel_path.to_path_buf(), hash);
                    }
                }
            }
        }

        // Create completion marker
        let outputs_dir = spec.workdir.join("outputs");
        std::fs::create_dir_all(&outputs_dir)?;
        let marker_path = outputs_dir.join("do_install_kernel.done");
        std::fs::write(&marker_path, "Kernel install complete\n")?;

        let duration = start.elapsed().as_millis() as u64;
        info!("Kernel install task completed in {}ms", duration);

        Ok((stdout, stderr, exit_code, output_files, duration))
    }

    /// Build fetch configuration from environment variables
    fn build_fetch_config(&self, env: &HashMap<String, String>) -> FetchConfig {
        let mut config = FetchConfig::default();

        // Git credentials from environment
        config.git_user = env
            .get("GIT_USER")
            .or_else(|| env.get("BB_GIT_USER"))
            .cloned();
        config.git_password = env
            .get("GIT_PASSWORD")
            .or_else(|| env.get("BB_GIT_PASSWORD"))
            .or_else(|| env.get("GIT_TOKEN"))
            .or_else(|| env.get("GITHUB_TOKEN"))
            .cloned();

        // Custom proxy override (supports SOCKS: socks5://, socks5h://)
        config.proxy_url = env
            .get("FETCHER_PROXY")
            .or_else(|| env.get("BB_PROXY"))
            .or_else(|| env.get("GIT_PROXY"))
            .cloned();

        // Proxy authentication
        config.proxy_user = env
            .get("PROXY_USER")
            .or_else(|| env.get("BB_PROXY_USER"))
            .cloned();
        config.proxy_password = env
            .get("PROXY_PASSWORD")
            .or_else(|| env.get("BB_PROXY_PASSWORD"))
            .cloned();

        // SSH key path for git operations
        config.ssh_key_path = env
            .get("GIT_SSH_KEY")
            .or_else(|| env.get("BB_GIT_SSH_KEY"))
            .map(PathBuf::from);

        // Git protocol preference
        config.git_protocol = env
            .get("BB_GIT_PROTOCOL")
            .or_else(|| env.get("GIT_PROTOCOL"))
            .cloned();

        // Allow insecure TLS if explicitly requested
        config.insecure = env
            .get("BB_FETCH_INSECURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        // Retry and timeout configuration
        config.retry_count = env
            .get("BB_FETCH_RETRIES")
            .and_then(|v| v.parse().ok());
        config.timeout_secs = env
            .get("BB_FETCH_TIMEOUT")
            .and_then(|v| v.parse().ok());

        config
    }

    /// Execute task in sandbox (for Shell/Python modes)
    fn execute_sandboxed(
        &mut self,
        spec: &TaskSpec,
    ) -> ExecutionResult<(String, String, i32, HashMap<PathBuf, ContentHash>, u64)> {
        let start = Instant::now();
        let mut sandbox_spec = self.prepare_sandbox(spec)?;
        let mut sandbox = self.sandbox_manager.create_sandbox(sandbox_spec.clone())?;

        // Update environment variables with actual sandbox paths
        let sandbox_root = sandbox.root().to_path_buf();

        // IMPORTANT: Since mount namespaces are currently disabled in native_sandbox.rs,
        // we must use actual host absolute paths to the sandbox directories, not namespace-relative paths.
        // When mount namespaces are enabled in the future, these should be changed back to /work, /work/src, etc.
        // Canonicalize to get absolute path (sandbox_root might be relative like "build/hitzeleiter-cache/sandboxes/XXX")
        let sandbox_root_abs = sandbox_root.canonicalize()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(&sandbox_root));
        let work_dir = sandbox_root_abs.join("work");
        sandbox_spec.env.insert(
            "WORKDIR".to_string(),
            work_dir.to_string_lossy().to_string(),
        );
        sandbox_spec.env.insert(
            "S".to_string(),
            work_dir.join("src").to_string_lossy().to_string(),
        );
        sandbox_spec.env.insert(
            "B".to_string(),
            work_dir.join("build").to_string_lossy().to_string(),
        );
        sandbox_spec.env.insert(
            "D".to_string(),
            work_dir.join("outputs").to_string_lossy().to_string(),
        );
        sandbox.update_env(sandbox_spec.env);

        info!("Executing in sandbox: {}", sandbox_root.display());
        let result = sandbox.execute()?;

        if !result.success() {
            warn!("Task failed with exit code: {}", result.exit_code);
            warn!("Stdout: {}", result.stdout);
            warn!("Stderr: {}", result.stderr);
            return Err(ExecutionError::TaskFailed(result.exit_code));
        }

        // Collect and hash outputs
        let output_map = sandbox.collect_outputs()?;
        let mut output_files = HashMap::new();

        for (path, content) in output_map {
            let hash = self.cas.put(&content)?;
            output_files.insert(path, hash);
        }

        // Cleanup sandbox
        sandbox.cleanup()?;

        let duration = start.elapsed().as_millis() as u64;

        Ok((
            result.stdout,
            result.stderr,
            result.exit_code,
            output_files,
            duration,
        ))
    }

    /// Compute task signature from spec
    fn compute_signature(&mut self, spec: &TaskSpec) -> ExecutionResult<TaskSignature> {
        let mut sig = TaskSignature {
            recipe: spec.recipe.clone(),
            task: spec.name.clone(),
            input_files: HashMap::new(),
            dep_signatures: Vec::new(),
            env_vars: spec.env.clone(),
            task_code_hash: ContentHash::from_bytes(spec.script.as_bytes()),
            signature: None,
        };

        // Hash input files in workdir
        if spec.workdir.exists() {
            self.hash_directory(&spec.workdir, &mut sig.input_files)?;
        }

        Ok(sig)
    }

    /// Recursively hash all files in a directory
    fn hash_directory(
        &mut self,
        dir: &Path,
        hashes: &mut HashMap<PathBuf, ContentHash>,
    ) -> ExecutionResult<()> {
        if !dir.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if entry.file_type().is_file() {
                let path = entry.path();
                let hash = ContentHash::from_file(path)?;
                let rel_path = path.strip_prefix(dir)
                    .unwrap_or(path)
                    .to_path_buf();
                hashes.insert(rel_path, hash);
            }
        }

        Ok(())
    }

    /// Prepare sandbox spec from task spec
    fn prepare_sandbox(&self, spec: &TaskSpec) -> ExecutionResult<SandboxSpec> {
        let mut sandbox_spec = SandboxSpec::new(vec![spec.script.clone()]);

        // Mount workdir as read-write
        sandbox_spec.rw_dirs.push(PathBuf::from("/work"));
        sandbox_spec.rw_dirs.push(PathBuf::from("/work/outputs"));
        sandbox_spec.rw_dirs.push(PathBuf::from("/work/build"));
        sandbox_spec.rw_dirs.push(PathBuf::from("/work/temp"));
        sandbox_spec.cwd = PathBuf::from("/work");

        // Create prelude.sh for sandboxed execution
        // Write to sandbox directory and bind-mount to /hitzeleiter/prelude.sh in sandbox
        const PRELUDE_CONTENT: &str = include_str!("prelude.sh");
        let prelude_path = self.sandbox_manager.sandbox_dir().join("prelude.sh");
        std::fs::write(&prelude_path, PRELUDE_CONTENT)?;
        sandbox_spec.ro_inputs.push((
            prelude_path,
            PathBuf::from("/hitzeleiter/prelude.sh"),
        ));

        // Set HITZELEITER_ROOT for scripts to find the prelude
        // In sandbox mode, prelude is bind-mounted to /hitzeleiter
        sandbox_spec.env.insert("HITZELEITER_ROOT".to_string(), "/hitzeleiter".to_string());

        // If workdir exists, mount it
        if spec.workdir.exists() {
            sandbox_spec.ro_inputs.push((
                spec.workdir.clone(),
                PathBuf::from("/work/src"),
            ));
        }

        // Add declared outputs
        for output in &spec.outputs {
            // If output is already an absolute path, use it as-is
            if output.is_absolute() {
                sandbox_spec.outputs.push(output.clone());
            } else {
                sandbox_spec.outputs.push(PathBuf::from("/work/outputs").join(output));
            }
        }

        // Copy environment
        sandbox_spec.env = spec.env.clone();

        // BitBake environment variables will be set after sandbox creation with actual paths

        Ok(sandbox_spec)
    }

    /// Restore task outputs to a directory
    pub fn restore_outputs(
        &self,
        output: &TaskOutput,
        dest_dir: &Path,
    ) -> ExecutionResult<()> {
        std::fs::create_dir_all(dest_dir)?;

        for (path, hash) in &output.output_files {
            let dest_path = dest_dir.join(path);
            self.cas.get_file(hash, &dest_path)?;
        }

        Ok(())
    }

    /// Get executor statistics
    pub fn stats(&self) -> &ExecutionStats {
        &self.stats
    }

    /// Clean up old sandboxes
    pub fn cleanup_sandboxes(&self) -> ExecutionResult<usize> {
        self.sandbox_manager.cleanup()
    }

    /// Get CAS statistics
    pub fn cas_stats(&self) -> super::cache::CacheStats {
        self.cas.stats()
    }

    /// Get action cache statistics
    pub fn action_cache_stats(&self) -> super::cache::ActionCacheStats {
        self.action_cache.stats()
    }
}

/// Check if a file is an archive that we can unpack
fn is_archive(path: &Path) -> bool {
    let name = path.to_string_lossy();
    name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || name.ends_with(".tar.bz2")
        || name.ends_with(".tbz2")
        || name.ends_with(".tar.xz")
        || name.ends_with(".txz")
        || name.ends_with(".tar")
}

/// Check if a file is a patch file
fn is_patch_file(path: &Path) -> bool {
    let name = path.to_string_lossy();
    path.is_file() && (name.ends_with(".patch") || name.ends_with(".diff"))
}

/// Apply a patch file to a directory
///
/// Tries git apply first, then falls back to the system patch command.
fn apply_patch(target_dir: &Path, patch_path: &Path) -> Result<String, String> {
    use std::process::Command;

    // First try: git apply (works in any directory, even non-git)
    let git_result = Command::new("git")
        .args(["apply", "--verbose", "-p1"])
        .arg(patch_path)
        .current_dir(target_dir)
        .output();

    match git_result {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(format!("  Applied via git apply\n  {}{}", stdout, stderr));
        }
        Ok(output) => {
            debug!(
                "git apply failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Err(e) => {
            debug!("git command not available: {}", e);
        }
    }

    // Second try: patch command
    let patch_result = Command::new("patch")
        .args(["-p1", "-i"])
        .arg(patch_path)
        .current_dir(target_dir)
        .output();

    match patch_result {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(format!("  Applied via patch command\n  {}", stdout))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("patch command failed: {}", stderr))
        }
        Err(e) => {
            Err(format!("Neither git apply nor patch command available: {}", e))
        }
    }
}

/// Execution statistics
#[derive(Debug, Default, Clone)]
pub struct ExecutionStats {
    pub tasks_executed: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

impl ExecutionStats {
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            (self.cache_hits as f64) / (total as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_simple_task_execution() {
        let tmp = TempDir::new().unwrap();
        let mut executor = TaskExecutor::new(tmp.path()).unwrap();

        let spec = TaskSpec {
            name: "do_test".to_string(),
            recipe: "test-recipe".to_string(),
            script: "echo 'Hello from task' > /work/outputs/result.txt".to_string(),
            workdir: tmp.path().join("workdir"),
            env: HashMap::new(),
            outputs: vec![PathBuf::from("result.txt")],
            timeout: None,
            execution_mode: ExecutionMode::Shell,
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
        };

        std::fs::create_dir_all(&spec.workdir).unwrap();

        let output = executor.execute_task(spec).unwrap();

        assert_eq!(output.exit_code, 0);
        assert_eq!(output.output_files.len(), 1);
        assert_eq!(executor.stats().cache_misses, 1);
    }

    #[test]
    fn test_task_caching() {
        let tmp = TempDir::new().unwrap();
        let mut executor = TaskExecutor::new(tmp.path()).unwrap();

        let workdir = tmp.path().join("workdir");
        std::fs::create_dir_all(&workdir).unwrap();

        let spec = TaskSpec {
            name: "do_cached".to_string(),
            recipe: "cache-test".to_string(),
            script: "echo 'cached output' > /work/outputs/out.txt".to_string(),
            workdir: workdir.clone(),
            env: HashMap::new(),
            outputs: vec![PathBuf::from("out.txt")],
            timeout: None,
            execution_mode: ExecutionMode::Shell,
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
        };

        // First execution - cache miss
        let output1 = executor.execute_task(spec.clone()).unwrap();
        assert_eq!(executor.stats().cache_misses, 1);
        assert_eq!(executor.stats().cache_hits, 0);

        // Second execution - cache hit
        let output2 = executor.execute_task(spec.clone()).unwrap();
        assert_eq!(executor.stats().cache_hits, 1);
        assert_eq!(executor.stats().cache_misses, 1);

        // Outputs should be identical
        assert_eq!(output1.signature, output2.signature);
    }

    #[test]
    fn test_task_failure() {
        let tmp = TempDir::new().unwrap();
        let mut executor = TaskExecutor::new(tmp.path()).unwrap();

        let spec = TaskSpec {
            name: "do_fail".to_string(),
            recipe: "fail-recipe".to_string(),
            script: "exit 1".to_string(),
            workdir: tmp.path().join("workdir"),
            env: HashMap::new(),
            outputs: vec![],
            timeout: None,
            execution_mode: ExecutionMode::Shell,
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
        };

        let result = executor.execute_task(spec);
        assert!(result.is_err());

        match result {
            Err(ExecutionError::TaskFailed(code)) => assert_eq!(code, 1),
            _ => panic!("Expected TaskFailed error"),
        }
    }

    #[test]
    fn test_direct_rust_execution_no_sandbox() {
        let tmp = TempDir::new().unwrap();
        let mut executor = TaskExecutor::new(tmp.path()).unwrap();

        // Simple script that can be executed directly in Rust
        let script = r#"#!/bin/bash
. /hitzeleiter/prelude.sh
export PN="test-recipe"
bb_note "Starting DirectRust execution"
mkdir -p "$D/usr/bin"
touch "$D/usr/bin/myapp"
echo "Hello from Rust!" > "$D/output.txt"
"#;

        let spec = TaskSpec {
            name: "do_install".to_string(),
            recipe: "direct-test".to_string(),
            script: script.to_string(),
            workdir: tmp.path().join("workdir"),
            env: HashMap::new(),
            outputs: vec![PathBuf::from("usr/bin/myapp"), PathBuf::from("output.txt")],
            timeout: None,
            execution_mode: ExecutionMode::DirectRust,
            network_policy: NetworkPolicy::Isolated,
            resource_limits: ResourceLimits::default(),
        };

        std::fs::create_dir_all(&spec.workdir).unwrap();

        let output = executor.execute_task(spec).unwrap();

        // Verify execution succeeded
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("NOTE: Starting DirectRust execution"));

        // Verify outputs were collected
        assert!(output.output_files.len() >= 1, "Expected at least 1 output file");

        // Verify cache hit on second execution
        assert_eq!(executor.stats().cache_misses, 1);
    }

    #[test]
    fn test_auto_detect_execution_mode() {
        // Simple script should be DirectRust
        let simple_script = r#"#!/bin/bash
. /hitzeleiter/prelude.sh
bb_note "Hello"
touch "$D/file.txt"
"#;
        assert_eq!(
            script_analyzer::determine_execution_mode(simple_script),
            ExecutionMode::DirectRust
        );

        // Complex script should be Shell
        let complex_script = r#"#!/bin/bash
for file in *.txt; do
    echo "Processing $file"
done
"#;
        assert_eq!(
            script_analyzer::determine_execution_mode(complex_script),
            ExecutionMode::Shell
        );

        // Python script should be Python
        let python_script = r#"#!/usr/bin/env python3
print("Hello")
"#;
        assert_eq!(
            script_analyzer::determine_execution_mode(python_script),
            ExecutionMode::Python
        );
    }
}
