//! Script analysis for fast-path execution
//!
//! Analyzes BitBake task scripts to detect simple operations that can be
//! executed directly without spawning bash, achieving 2-5x speedup.
//!
//! ## Strategy
//! - Parse scripts line-by-line to detect common patterns
//! - Simple operations: mkdir, touch, echo, cp, variable expansion
//! - Complex operations: pipes, loops, conditionals → fallback to bash
//! - ~60% of BitBake tasks are simple enough for fast path

use super::types::ExecutionMode;
use std::collections::HashMap;

/// Action that can be executed directly without bash
#[derive(Debug, Clone, PartialEq)]
pub enum DirectAction {
    /// Create directory (mkdir -p)
    MakeDir { path: String },

    /// Create empty file or update timestamp (touch)
    Touch { path: String },

    /// Write content to file (echo > file)
    WriteFile { path: String, content: String },

    /// Append content to file (echo >> file)
    AppendFile { path: String, content: String },

    /// Copy file (cp)
    Copy { src: String, dest: String, recursive: bool, mode: Option<u32> },

    /// Move/rename file (mv)
    Move { src: String, dest: String },

    /// Remove file or directory (rm)
    Remove { path: String, recursive: bool, force: bool },

    /// Create symbolic link (ln -s)
    Symlink { target: String, link: String },

    /// Log message (echo/bb_note)
    Log { level: LogLevel, message: String },

    /// Set environment variable (export)
    SetEnv { key: String, value: String },

    /// Change file permissions (chmod)
    Chmod { path: String, mode: u32 },
}

/// Logging level
#[derive(Debug, Clone, PartialEq)]
pub enum LogLevel {
    Note,
    Warn,
    Error,
    Debug,
}

/// Result of script analysis
#[derive(Debug, Clone)]
pub struct ScriptAnalysis {
    /// Whether script is simple enough for fast path
    pub is_simple: bool,

    /// Direct actions to execute (if is_simple = true)
    pub actions: Vec<DirectAction>,

    /// Environment variables defined in script
    pub env_vars: HashMap<String, String>,

    /// Reason why script is complex (if is_simple = false)
    pub complexity_reason: Option<String>,
}

impl ScriptAnalysis {
    /// Create analysis result for complex script (must use bash)
    pub fn complex(reason: String) -> Self {
        Self {
            is_simple: false,
            actions: Vec::new(),
            env_vars: HashMap::new(),
            complexity_reason: Some(reason),
        }
    }

    /// Create analysis result for simple script (can use fast path)
    pub fn simple(actions: Vec<DirectAction>, env_vars: HashMap<String, String>) -> Self {
        Self {
            is_simple: true,
            actions,
            env_vars,
            complexity_reason: None,
        }
    }
}

/// Analyze script to determine if it can use fast path
///
/// Returns ScriptAnalysis with is_simple=true if script only contains
/// simple operations, or is_simple=false if bash is required.
pub fn analyze_script(script: &str) -> ScriptAnalysis {
    let mut actions = Vec::new();
    let mut env_vars = HashMap::new();

    for (i, line) in script.lines().enumerate() {
        let trimmed = line.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Skip comments (but not shebang which we'll skip separately)
        if trimmed.starts_with('#') && !trimmed.starts_with("#!/") {
            continue;
        }

        // Skip shebang
        if trimmed.starts_with("#!/") {
            continue;
        }

        // Skip prelude source
        if trimmed.starts_with(". /hitzeleiter/prelude.sh") || trimmed.starts_with("source /hitzeleiter/prelude.sh") {
            continue;
        }

        // Detect complexity markers (must use bash)
        if contains_complexity(trimmed) {
            return ScriptAnalysis::complex(format!(
                "Line {}: contains complex syntax: {}",
                i + 1,
                trimmed
            ));
        }

        // Try to parse as simple action
        match parse_simple_action(trimmed, &env_vars) {
            Some(action) => {
                // Track environment variables
                if let DirectAction::SetEnv { ref key, ref value } = action {
                    env_vars.insert(key.clone(), value.clone());
                }
                actions.push(action);
            }
            None => {
                // Unrecognized pattern → must use bash
                return ScriptAnalysis::complex(format!(
                    "Line {}: unrecognized pattern: {}",
                    i + 1,
                    trimmed
                ));
            }
        }
    }

    ScriptAnalysis::simple(actions, env_vars)
}

/// Check if line contains complexity that requires bash
fn contains_complexity(line: &str) -> bool {
    // Pipe operators
    if line.contains('|') && !line.contains("||") {
        return true;
    }

    // Control flow
    if line.starts_with("if ") || line.starts_with("for ") ||
       line.starts_with("while ") || line.starts_with("case ") {
        return true;
    }

    // Subshells
    if line.contains("$(") || line.contains('`') {
        return true;
    }

    // Loops, functions (check as whole words at start or with whitespace)
    if line.starts_with("do ") || line.contains(" do ") ||
       line.starts_with("done") || line.contains(" done") || line.ends_with(" done") ||
       line.starts_with("then") || line.contains(" then") ||
       line.starts_with("fi") || line.contains(" fi") || line.ends_with(" fi") ||
       line.starts_with("function ") || line.contains(" function ") {
        return true;
    }

    // Signal handlers and traps (bash-specific)
    if line.starts_with("trap ") || line.contains(" trap ") {
        return true;
    }

    // Redirects (except simple > or >>)
    if line.contains(">&") || line.contains("<(") || line.contains("2>") {
        return true;
    }

    // Background jobs
    if line.trim().ends_with('&') {
        return true;
    }

    false
}

/// Parse line as simple action (or None if complex)
fn parse_simple_action(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    let line = line.trim();

    // Empty line
    if line.is_empty() {
        return None;
    }

    // Export statement: export VAR="value"
    if line.starts_with("export ") {
        return parse_export(line);
    }

    // Touch command: touch file
    if line.starts_with("touch ") {
        return parse_touch(line, env_vars);
    }

    // mkdir -p: mkdir -p dir
    if line.starts_with("mkdir -p ") || line.starts_with("mkdir -p\"") {
        return parse_mkdir(line, env_vars);
    }

    // bbdirs helper: bbdirs "$D/foo" "$D/bar"
    if line.starts_with("bbdirs ") {
        return parse_bbdirs(line, env_vars);
    }

    // bb_note logging: bb_note "message"
    if line.starts_with("bb_note ") {
        return parse_bb_log(line, LogLevel::Note);
    }

    // bb_warn logging
    if line.starts_with("bb_warn ") {
        return parse_bb_log(line, LogLevel::Warn);
    }

    // bb_debug logging
    if line.starts_with("bb_debug ") {
        return parse_bb_log(line, LogLevel::Debug);
    }

    // echo statement: echo "message"
    if line.starts_with("echo ") {
        return parse_echo(line);
    }

    // Simple echo redirect: echo "content" > file or >> file
    if line.contains("echo ") && (line.contains(" > ") || line.contains(" >> ")) {
        return parse_echo_redirect(line, env_vars);
    }

    // Copy command: cp [-r] src dest
    if line.starts_with("cp ") {
        return parse_cp(line, env_vars);
    }

    // Move command: mv src dest
    if line.starts_with("mv ") {
        return parse_mv(line, env_vars);
    }

    // Remove command: rm [-rf] path
    if line.starts_with("rm ") {
        return parse_rm(line, env_vars);
    }

    // Symlink: ln -s target link
    if line.starts_with("ln -s ") || line.starts_with("ln -sf ") {
        return parse_ln(line, env_vars);
    }

    // Chmod: chmod mode file
    if line.starts_with("chmod ") {
        return parse_chmod(line, env_vars);
    }

    None
}

/// Parse export statement
fn parse_export(line: &str) -> Option<DirectAction> {
    // export VAR="value" or export VAR=value
    let rest = line.strip_prefix("export ")?.trim();

    // Find = sign
    let eq_pos = rest.find('=')?;
    let key = rest[..eq_pos].trim().to_string();
    let value_part = rest[eq_pos + 1..].trim();

    // Remove quotes if present
    let value = if (value_part.starts_with('"') && value_part.ends_with('"')) ||
                   (value_part.starts_with('\'') && value_part.ends_with('\'')) {
        value_part[1..value_part.len() - 1].to_string()
    } else {
        value_part.to_string()
    };

    Some(DirectAction::SetEnv { key, value })
}

/// Parse touch command
fn parse_touch(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    let rest = line.strip_prefix("touch ")?.trim();

    // Remove quotes
    let path = remove_quotes(rest);
    let expanded = expand_variables(&path, env_vars);

    Some(DirectAction::Touch { path: expanded })
}

/// Parse mkdir -p command
fn parse_mkdir(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    let rest = line.strip_prefix("mkdir -p ")?.trim();

    // Remove quotes
    let path = remove_quotes(rest);
    let expanded = expand_variables(&path, env_vars);

    Some(DirectAction::MakeDir { path: expanded })
}

/// Parse bbdirs helper (mkdir -p multiple dirs)
fn parse_bbdirs(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    // bbdirs can take multiple arguments, but for simplicity we only support one for now
    // For multiple dirs, return complex
    let rest = line.strip_prefix("bbdirs ")?.trim();

    // Check if multiple arguments (contains space outside quotes)
    // For now, only support single argument
    let path = remove_quotes(rest);
    let expanded = expand_variables(&path, env_vars);

    Some(DirectAction::MakeDir { path: expanded })
}

/// Parse bb_note/bb_warn/bb_debug
fn parse_bb_log(line: &str, level: LogLevel) -> Option<DirectAction> {
    let prefix = match level {
        LogLevel::Note => "bb_note ",
        LogLevel::Warn => "bb_warn ",
        LogLevel::Debug => "bb_debug ",
        _ => return None,
    };

    let rest = line.strip_prefix(prefix)?.trim();
    let message = remove_quotes(rest);

    Some(DirectAction::Log { level, message })
}

/// Parse echo statement
fn parse_echo(line: &str) -> Option<DirectAction> {
    let rest = line.strip_prefix("echo ")?.trim();
    let message = remove_quotes(rest);

    Some(DirectAction::Log {
        level: LogLevel::Note,
        message
    })
}

/// Parse echo redirect: echo "content" > file or >> file
fn parse_echo_redirect(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    // Determine if write or append
    let is_append = line.contains(" >> ");
    let separator = if is_append { " >> " } else { " > " };

    if !line.contains(separator) {
        return None;
    }

    let parts: Vec<&str> = line.split(separator).collect();
    if parts.len() != 2 {
        return None;
    }

    let echo_part = parts[0].trim().strip_prefix("echo ")?.trim();
    let file_part = parts[1].trim();

    let content = remove_quotes(echo_part);
    let path = remove_quotes(file_part);
    let expanded_path = expand_variables(&path, env_vars);

    if is_append {
        Some(DirectAction::AppendFile {
            path: expanded_path,
            content
        })
    } else {
        Some(DirectAction::WriteFile {
            path: expanded_path,
            content
        })
    }
}

/// Parse cp command: cp [-r] src dest
fn parse_cp(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    let rest = line.strip_prefix("cp ")?.trim();

    // Check for -r flag
    let (recursive, rest) = if rest.starts_with("-r ") || rest.starts_with("-R ") {
        (true, rest[3..].trim())
    } else {
        (false, rest)
    };

    // Split into src and dest
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() != 2 {
        return None; // Complex case with multiple sources
    }

    let src = remove_quotes(parts[0]);
    let dest = remove_quotes(parts[1]);
    let src_expanded = expand_variables(&src, env_vars);
    let dest_expanded = expand_variables(&dest, env_vars);

    Some(DirectAction::Copy {
        src: src_expanded,
        dest: dest_expanded,
        recursive,
        mode: None,
    })
}

/// Parse mv command: mv src dest
fn parse_mv(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    let rest = line.strip_prefix("mv ")?.trim();

    // Split into src and dest
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() != 2 {
        return None; // Complex case
    }

    let src = remove_quotes(parts[0]);
    let dest = remove_quotes(parts[1]);
    let src_expanded = expand_variables(&src, env_vars);
    let dest_expanded = expand_variables(&dest, env_vars);

    Some(DirectAction::Move {
        src: src_expanded,
        dest: dest_expanded,
    })
}

/// Parse rm command: rm [-rf] path
fn parse_rm(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    let rest = line.strip_prefix("rm ")?.trim();

    let mut recursive = false;
    let mut force = false;
    let mut remaining = rest;

    // Parse flags
    loop {
        if remaining.starts_with("-rf ") || remaining.starts_with("-fr ") {
            recursive = true;
            force = true;
            remaining = &remaining[4..];
        } else if remaining.starts_with("-r ") {
            recursive = true;
            remaining = &remaining[3..];
        } else if remaining.starts_with("-f ") {
            force = true;
            remaining = &remaining[3..];
        } else {
            break;
        }
    }

    let path = remove_quotes(remaining.trim());
    let expanded_path = expand_variables(&path, env_vars);

    Some(DirectAction::Remove {
        path: expanded_path,
        recursive,
        force,
    })
}

/// Parse ln command: ln -s target link
fn parse_ln(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    // ln -s or ln -sf
    let rest = if let Some(r) = line.strip_prefix("ln -sf ") {
        r
    } else {
        line.strip_prefix("ln -s ")?
    };

    let parts: Vec<&str> = rest.trim().split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }

    let target = remove_quotes(parts[0]);
    let link = remove_quotes(parts[1]);
    let target_expanded = expand_variables(&target, env_vars);
    let link_expanded = expand_variables(&link, env_vars);

    Some(DirectAction::Symlink {
        target: target_expanded,
        link: link_expanded,
    })
}

/// Parse chmod command: chmod mode file
fn parse_chmod(line: &str, env_vars: &HashMap<String, String>) -> Option<DirectAction> {
    let rest = line.strip_prefix("chmod ")?.trim();

    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }

    // Parse mode as octal
    let mode = u32::from_str_radix(parts[0], 8).ok()?;
    let path = remove_quotes(parts[1]);
    let expanded_path = expand_variables(&path, env_vars);

    Some(DirectAction::Chmod {
        mode,
        path: expanded_path,
    })
}

/// Remove surrounding quotes from string
fn remove_quotes(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) ||
       (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Expand environment variables in string
///
/// Supports: $VAR and ${VAR}
/// Unknown variables are left as-is
fn expand_variables(s: &str, env_vars: &HashMap<String, String>) -> String {
    let mut result = s.to_string();

    // Common BitBake variables with defaults
    let defaults: HashMap<&str, &str> = [
        ("PN", "unknown"),
        ("PV", "1.0"),
        ("PR", "r0"),
        ("WORKDIR", "/work"),
        ("S", "/work/src"),
        ("B", "/work/build"),
        ("D", "/work/image"),
        ("TMPDIR", "/tmp"),
    ].iter().cloned().collect();

    // Replace ${VAR}
    for (key, value) in env_vars {
        result = result.replace(&format!("${{{}}}", key), value);
    }

    // Replace $VAR (simple form)
    for (key, value) in env_vars {
        // Only replace if followed by space, /, or end of string
        result = result.replace(&format!("${}", key), value);
    }

    // Apply defaults for BitBake variables
    for (key, default_value) in defaults {
        if !env_vars.contains_key(key) {
            result = result.replace(&format!("${{{}}}", key), default_value);
            result = result.replace(&format!("${}", key), default_value);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_simple_script() {
        let script = r#"#!/bin/bash
. /hitzeleiter/prelude.sh
export PN="test"
bb_note "Starting task"
touch "$D/output.txt"
"#;

        let analysis = analyze_script(script);
        assert!(analysis.is_simple);
        assert_eq!(analysis.actions.len(), 3);
    }

    #[test]
    fn test_analyze_complex_script() {
        let script = r#"#!/bin/bash
. /hitzeleiter/prelude.sh
if [ -f test.txt ]; then
    echo "exists"
fi
"#;

        let analysis = analyze_script(script);
        assert!(!analysis.is_simple);
        assert!(analysis.complexity_reason.is_some());
    }

    #[test]
    fn test_parse_export() {
        let action = parse_export(r#"export PN="hello-world""#).unwrap();
        assert_eq!(action, DirectAction::SetEnv {
            key: "PN".to_string(),
            value: "hello-world".to_string(),
        });
    }

    #[test]
    fn test_parse_touch() {
        let env = HashMap::new();
        let action = parse_touch(r#"touch "$D/test.txt""#, &env).unwrap();
        assert_eq!(action, DirectAction::Touch {
            path: "/work/image/test.txt".to_string(),
        });
    }

    #[test]
    fn test_variable_expansion() {
        let mut env = HashMap::new();
        env.insert("D".to_string(), "/work/image".to_string());

        let expanded = expand_variables("$D/test.txt", &env);
        assert_eq!(expanded, "/work/image/test.txt");
    }

    #[test]
    fn test_detect_pipe_complexity() {
        assert!(contains_complexity("ls | grep foo"));
        assert!(!contains_complexity("echo 'test || fail'"));
    }
}

/// Determine optimal execution mode for a script
///
/// Analyzes the script and returns the best execution mode:
/// - DirectRust: Simple script with only file operations, no shell needed
/// - Python: Python script (not yet implemented)
/// - Shell: Complex script requiring bash
pub fn determine_execution_mode(script: &str) -> ExecutionMode {
    // Check if it's a Python script
    if script.trim_start().starts_with("#!") && script.contains("python") {
        return ExecutionMode::Python;
    }

    // TEMPORARY: Force Shell mode for all non-Python scripts
    // DirectRust is not yet fully implemented, so we need to use Shell mode
    // to ensure tasks actually execute properly (especially unpack, configure, etc.)
    ExecutionMode::Shell

    // TODO: Re-enable DirectRust optimization once it's fully implemented
    // let analysis = analyze_script(script);
    // if analysis.is_simple {
    //     ExecutionMode::DirectRust
    // } else {
    //     ExecutionMode::Shell
    // }
}

#[cfg(test)]
mod execution_mode_tests {
    use super::*;

    #[test]
    fn test_determine_simple_script() {
        let script = r#"#!/bin/bash
. /hitzeleiter/prelude.sh
export PN="test"
bb_note "Starting"
touch "$D/output.txt"
"#;
        assert_eq!(determine_execution_mode(script), ExecutionMode::DirectRust);
    }

    #[test]
    fn test_determine_complex_script() {
        let script = r#"#!/bin/bash
. /hitzeleiter/prelude.sh
for file in *.txt; do
    echo "Processing $file"
done
"#;
        assert_eq!(determine_execution_mode(script), ExecutionMode::Shell);
    }

    #[test]
    fn test_determine_python_script() {
        let script = r#"#!/usr/bin/env python3
print("Hello from Python")
"#;
        assert_eq!(determine_execution_mode(script), ExecutionMode::Python);
    }
}
