//! Direct execution of analyzed scripts without bash
//!
//! Executes simple scripts directly using Rust std::fs and std::io,
//! bypassing bash entirely for 2-5x speedup.

use super::script_analyzer::{DirectAction, LogLevel, ScriptAnalysis};
use super::types::ExecutionError;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, warn};

/// Result of direct execution
pub struct DirectExecutionResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

/// Execute analyzed script directly without bash
///
/// Takes a ScriptAnalysis and executes all actions directly using Rust APIs.
/// This is significantly faster than spawning bash (~2-5x speedup).
///
/// # Arguments
/// * `analysis` - Analyzed script with direct actions
/// * `work_dir` - Working directory for execution
/// * `env` - Additional environment variables
///
/// # Returns
/// DirectExecutionResult with stdout/stderr/exit_code
pub fn execute_direct(
    analysis: &ScriptAnalysis,
    work_dir: &Path,
    env: &HashMap<String, String>,
) -> Result<DirectExecutionResult, ExecutionError> {
    let start = Instant::now();
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;

    debug!("Direct execution: {} actions", analysis.actions.len());

    // Merge environment variables
    let mut full_env = analysis.env_vars.clone();
    for (k, v) in env {
        full_env.insert(k.clone(), v.clone());
    }

    // Ensure work directory exists
    if !work_dir.exists() {
        fs::create_dir_all(work_dir)
            .map_err(|e| ExecutionError::SandboxError(format!("Failed to create work_dir: {}", e)))?;
    }

    // Execute each action
    for (i, action) in analysis.actions.iter().enumerate() {
        match execute_action(action, work_dir, &full_env, &mut stdout, &mut stderr) {
            Ok(_) => {
                debug!("Action {} completed: {:?}", i, action);
            }
            Err(e) => {
                stderr.push_str(&format!("ERROR: Action {} failed: {}\n", i, e));
                exit_code = 1;
                warn!("Direct execution failed at action {}: {}", i, e);
                break;
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(DirectExecutionResult {
        exit_code,
        stdout,
        stderr,
        duration_ms,
    })
}

/// Execute a single action
fn execute_action(
    action: &DirectAction,
    work_dir: &Path,
    env: &HashMap<String, String>,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<(), ExecutionError> {
    match action {
        DirectAction::MakeDir { path } => {
            let full_path = resolve_path(path, work_dir, env)?;
            fs::create_dir_all(&full_path)
                .map_err(|e| ExecutionError::SandboxError(format!("mkdir failed: {}", e)))?;
            debug!("Created directory: {}", full_path.display());
        }

        DirectAction::Touch { path } => {
            let full_path = resolve_path(path, work_dir, env)?;

            // Create parent directory if needed
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ExecutionError::SandboxError(format!("mkdir parent failed: {}", e)))?;
            }

            // Touch file (create or update mtime)
            if full_path.exists() {
                // Update mtime
                let metadata = fs::metadata(&full_path)?;
                let mtime = filetime::FileTime::now();
                filetime::set_file_times(&full_path, mtime, mtime)?;
            } else {
                // Create empty file
                fs::write(&full_path, "")
                    .map_err(|e| ExecutionError::SandboxError(format!("touch failed: {}", e)))?;
            }
            debug!("Touched file: {}", full_path.display());
        }

        DirectAction::WriteFile { path, content } => {
            let full_path = resolve_path(path, work_dir, env)?;

            // Create parent directory if needed
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ExecutionError::SandboxError(format!("mkdir parent failed: {}", e)))?;
            }

            fs::write(&full_path, content)
                .map_err(|e| ExecutionError::SandboxError(format!("write failed: {}", e)))?;
            debug!("Wrote file: {} ({} bytes)", full_path.display(), content.len());
        }

        DirectAction::AppendFile { path, content } => {
            let full_path = resolve_path(path, work_dir, env)?;

            // Create parent directory if needed
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ExecutionError::SandboxError(format!("mkdir parent failed: {}", e)))?;
            }

            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&full_path)
                .map_err(|e| ExecutionError::SandboxError(format!("append failed: {}", e)))?;

            file.write_all(content.as_bytes())
                .map_err(|e| ExecutionError::SandboxError(format!("append write failed: {}", e)))?;

            debug!("Appended to file: {} ({} bytes)", full_path.display(), content.len());
        }

        DirectAction::Copy { src, dest, recursive, mode } => {
            let src_path = resolve_path(src, work_dir, env)?;
            let dest_path = resolve_path(dest, work_dir, env)?;

            // Create parent directory if needed
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ExecutionError::SandboxError(format!("mkdir parent failed: {}", e)))?;
            }

            if *recursive && src_path.is_dir() {
                // Recursive directory copy
                copy_dir_all(&src_path, &dest_path)?;
            } else {
                // Single file copy
                fs::copy(&src_path, &dest_path)
                    .map_err(|e| ExecutionError::SandboxError(format!("copy failed: {}", e)))?;
            }

            // Set permissions if specified
            #[cfg(unix)]
            if let Some(mode_val) = mode {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(*mode_val);
                fs::set_permissions(&dest_path, perms)
                    .map_err(|e| ExecutionError::SandboxError(format!("chmod failed: {}", e)))?;
            }

            debug!("Copied: {} -> {}", src_path.display(), dest_path.display());
        }

        DirectAction::Move { src, dest } => {
            let src_path = resolve_path(src, work_dir, env)?;
            let dest_path = resolve_path(dest, work_dir, env)?;

            // Create parent directory if needed
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ExecutionError::SandboxError(format!("mkdir parent failed: {}", e)))?;
            }

            fs::rename(&src_path, &dest_path)
                .map_err(|e| ExecutionError::SandboxError(format!("move failed: {}", e)))?;
            debug!("Moved: {} -> {}", src_path.display(), dest_path.display());
        }

        DirectAction::Remove { path, recursive, force } => {
            let full_path = resolve_path(path, work_dir, env)?;

            if !full_path.exists() {
                if *force {
                    // -f flag: ignore non-existent files
                    return Ok(());
                } else {
                    return Err(ExecutionError::SandboxError(format!(
                        "rm: {} does not exist",
                        full_path.display()
                    )));
                }
            }

            if full_path.is_dir() {
                if *recursive {
                    fs::remove_dir_all(&full_path)
                        .map_err(|e| ExecutionError::SandboxError(format!("rm -r failed: {}", e)))?;
                } else {
                    return Err(ExecutionError::SandboxError(format!(
                        "rm: {} is a directory (use -r flag)",
                        full_path.display()
                    )));
                }
            } else {
                fs::remove_file(&full_path)
                    .map_err(|e| ExecutionError::SandboxError(format!("rm failed: {}", e)))?;
            }
            debug!("Removed: {}", full_path.display());
        }

        DirectAction::Symlink { target, link } => {
            let target_path = resolve_path(target, work_dir, env)?;
            let link_path = resolve_path(link, work_dir, env)?;

            // Create parent directory if needed
            if let Some(parent) = link_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ExecutionError::SandboxError(format!("mkdir parent failed: {}", e)))?;
            }

            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&target_path, &link_path)
                    .map_err(|e| ExecutionError::SandboxError(format!("symlink failed: {}", e)))?;
            }

            #[cfg(windows)]
            {
                if target_path.is_dir() {
                    std::os::windows::fs::symlink_dir(&target_path, &link_path)
                        .map_err(|e| ExecutionError::SandboxError(format!("symlink failed: {}", e)))?;
                } else {
                    std::os::windows::fs::symlink_file(&target_path, &link_path)
                        .map_err(|e| ExecutionError::SandboxError(format!("symlink failed: {}", e)))?;
                }
            }

            debug!("Created symlink: {} -> {}", link_path.display(), target_path.display());
        }

        DirectAction::Chmod { path, mode } => {
            let full_path = resolve_path(path, work_dir, env)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(*mode);
                fs::set_permissions(&full_path, perms)
                    .map_err(|e| ExecutionError::SandboxError(format!("chmod failed: {}", e)))?;
                debug!("Changed permissions: {} to {:o}", full_path.display(), mode);
            }

            #[cfg(not(unix))]
            {
                warn!("chmod not supported on this platform");
            }
        }

        DirectAction::Log { level, message } => {
            let expanded = expand_env_in_message(message, env);
            let log_line = match level {
                LogLevel::Note => format!("NOTE: {}\n", expanded),
                LogLevel::Warn => format!("WARNING: {}\n", expanded),
                LogLevel::Error => format!("ERROR: {}\n", expanded),
                LogLevel::Debug => format!("DEBUG: {}\n", expanded),
            };

            // Note and Debug go to stdout, Warn and Error to stderr
            match level {
                LogLevel::Note | LogLevel::Debug => stdout.push_str(&log_line),
                LogLevel::Warn | LogLevel::Error => stderr.push_str(&log_line),
            }
        }

        DirectAction::SetEnv { key, value } => {
            // Environment is already tracked in analysis.env_vars
            // This is a no-op for execution, but we log it
            debug!("Set env: {}={}", key, value);
        }
    }

    Ok(())
}

/// Recursively copy a directory
fn copy_dir_all(src: &Path, dest: &Path) -> Result<(), ExecutionError> {
    fs::create_dir_all(dest)
        .map_err(|e| ExecutionError::SandboxError(format!("create dest dir failed: {}", e)))?;

    for entry in fs::read_dir(src)
        .map_err(|e| ExecutionError::SandboxError(format!("read source dir failed: {}", e)))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_all(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)
                .map_err(|e| ExecutionError::SandboxError(format!("copy file failed: {}", e)))?;
        }
    }

    Ok(())
}

/// Resolve path with environment variable expansion
fn resolve_path(
    path: &str,
    work_dir: &Path,
    env: &HashMap<String, String>,
) -> Result<PathBuf, ExecutionError> {
    let expanded = expand_env_in_message(path, env);
    let path_buf = PathBuf::from(&expanded);

    // If absolute, use as-is
    if path_buf.is_absolute() {
        Ok(path_buf)
    } else {
        // Relative to work_dir
        Ok(work_dir.join(path_buf))
    }
}

/// Expand environment variables in message
fn expand_env_in_message(msg: &str, env: &HashMap<String, String>) -> String {
    let mut result = msg.to_string();

    // Replace ${VAR} and $VAR
    for (key, value) in env {
        result = result.replace(&format!("${{{}}}", key), value);
        result = result.replace(&format!("${}", key), value);
    }

    // Common BitBake defaults (already applied in analyzer, but ensure consistency)
    let defaults: HashMap<&str, &str> = [
        ("PN", "unknown"),
        ("WORKDIR", "/work"),
        ("S", "/work/src"),
        ("B", "/work/build"),
        ("D", "/work/image"),
    ].iter().cloned().collect();

    for (key, default_value) in defaults {
        if !env.contains_key(key) {
            result = result.replace(&format!("${{{}}}", key), default_value);
            result = result.replace(&format!("${}", key), default_value);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::script_analyzer::analyze_script;
    use tempfile::TempDir;

    #[test]
    fn test_direct_execution_simple() {
        let tmp = TempDir::new().unwrap();
        let work_dir = tmp.path().join("work");

        let script = r#"#!/bin/bash
. /bitzel/prelude.sh
export PN="test-recipe"
bb_note "Starting test"
touch "$D/output.txt"
"#;

        let analysis = analyze_script(script);
        assert!(analysis.is_simple);

        let env = HashMap::new();
        let result = execute_direct(&analysis, &work_dir, &env).unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("NOTE: Starting test"));

        // Check file was created
        let output_file = work_dir.join("image/output.txt");
        assert!(output_file.exists());
    }

    #[test]
    fn test_direct_execution_mkdir() {
        let tmp = TempDir::new().unwrap();
        let work_dir = tmp.path().join("work");

        let script = r#"#!/bin/bash
. /bitzel/prelude.sh
export PN="test"
bbdirs "$D/usr/bin"
"#;

        let analysis = analyze_script(script);
        assert!(analysis.is_simple);

        let env = HashMap::new();
        let result = execute_direct(&analysis, &work_dir, &env).unwrap();

        assert_eq!(result.exit_code, 0);

        // Check directory was created
        let dir = work_dir.join("image/usr/bin");
        assert!(dir.exists());
        assert!(dir.is_dir());
    }
}
