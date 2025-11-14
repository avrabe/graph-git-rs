//! Execution log and error mapping
//!
//! Provides structured logging and error mapping for task execution,
//! making it easy to understand what happened during sandboxed execution.

use super::sandbox_backend::SandboxResult;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Structured execution log for a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLog {
    /// Task identifier (recipe:task)
    pub task_id: String,

    /// Execution outcome
    pub outcome: ExecutionOutcome,

    /// Exit code from the process
    pub exit_code: i32,

    /// Execution duration in milliseconds
    pub duration_ms: u64,

    /// Standard output (captured)
    pub stdout: String,

    /// Standard error (captured)
    pub stderr: String,

    /// Output files generated
    pub outputs: Vec<PathBuf>,

    /// Structured error information (if failed)
    pub error: Option<ExecutionError>,
}

/// Execution outcome
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionOutcome {
    /// Task completed successfully
    Success,

    /// Task failed with non-zero exit code
    Failed,

    /// Task was killed by signal
    Killed,

    /// Task timed out
    Timeout,

    /// Sandbox error (couldn't execute)
    SandboxError,
}

impl fmt::Display for ExecutionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => write!(f, "✅ SUCCESS"),
            Self::Failed => write!(f, "❌ FAILED"),
            Self::Killed => write!(f, "💀 KILLED"),
            Self::Timeout => write!(f, "⏱️  TIMEOUT"),
            Self::SandboxError => write!(f, "🚫 SANDBOX ERROR"),
        }
    }
}

/// Structured error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionError {
    /// Error category
    pub category: ErrorCategory,

    /// Human-readable error message
    pub message: String,

    /// Suggested fix or next steps
    pub suggestion: Option<String>,

    /// Related log lines (from stderr)
    pub related_logs: Vec<String>,
}

/// Error category for better error handling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCategory {
    /// Compilation error (missing files, syntax errors)
    CompilationError,

    /// Missing dependency (library, tool)
    MissingDependency,

    /// Permission error (file access)
    PermissionError,

    /// Network error (download failed)
    NetworkError,

    /// Disk space or I/O error
    IOError,

    /// Timeout
    Timeout,

    /// Unknown or unclassified error
    Unknown,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompilationError => write!(f, "Compilation Error"),
            Self::MissingDependency => write!(f, "Missing Dependency"),
            Self::PermissionError => write!(f, "Permission Error"),
            Self::NetworkError => write!(f, "Network Error"),
            Self::IOError => write!(f, "I/O Error"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Unknown => write!(f, "Unknown Error"),
        }
    }
}

impl ExecutionLog {
    /// Create execution log from sandbox result
    pub fn from_sandbox_result(
        task_id: String,
        result: &SandboxResult,
        outputs: Vec<PathBuf>,
    ) -> Self {
        let outcome = if result.success() {
            ExecutionOutcome::Success
        } else if result.exit_code == -1 {
            ExecutionOutcome::Killed
        } else {
            ExecutionOutcome::Failed
        };

        let error = if !result.success() {
            Some(Self::analyze_error(result))
        } else {
            None
        };

        Self {
            task_id,
            outcome,
            exit_code: result.exit_code,
            duration_ms: result.duration_ms,
            stdout: result.stdout.clone(),
            stderr: result.stderr.clone(),
            outputs,
            error,
        }
    }

    /// Analyze stderr to categorize error
    fn analyze_error(result: &SandboxResult) -> ExecutionError {
        let stderr_lower = result.stderr.to_lowercase();

        let (category, message, suggestion) = if stderr_lower.contains("no such file")
            || stderr_lower.contains("cannot find")
        {
            (
                ErrorCategory::MissingDependency,
                "Missing file or dependency".to_string(),
                Some("Check that all required files and dependencies are available".to_string()),
            )
        } else if stderr_lower.contains("permission denied") {
            (
                ErrorCategory::PermissionError,
                "Permission denied".to_string(),
                Some("Check file permissions in the sandbox workspace".to_string()),
            )
        } else if stderr_lower.contains("error:") && stderr_lower.contains(".c:") {
            (
                ErrorCategory::CompilationError,
                "C/C++ compilation error".to_string(),
                Some("Review compiler error messages in stderr".to_string()),
            )
        } else if stderr_lower.contains("curl") || stderr_lower.contains("wget") {
            (
                ErrorCategory::NetworkError,
                "Network download failed".to_string(),
                Some("Check network connectivity and URL".to_string()),
            )
        } else if stderr_lower.contains("disk") || stderr_lower.contains("space") {
            (
                ErrorCategory::IOError,
                "Disk I/O error".to_string(),
                Some("Check available disk space".to_string()),
            )
        } else {
            (
                ErrorCategory::Unknown,
                format!("Task failed with exit code {}", result.exit_code),
                None,
            )
        };

        // Extract relevant log lines (lines containing "error", "fail", etc.)
        let related_logs: Vec<String> = result
            .stderr
            .lines()
            .filter(|line| {
                let lower = line.to_lowercase();
                lower.contains("error")
                    || lower.contains("fail")
                    || lower.contains("fatal")
                    || lower.contains("warning")
            })
            .take(10)
            .map(|s| s.to_string())
            .collect();

        ExecutionError {
            category,
            message,
            suggestion,
            related_logs,
        }
    }

    /// Format log for human-readable display
    pub fn format_display(&self) -> String {
        let mut output = String::new();

        output.push_str(&format!("╔════════════════════════════════════════════════════════════════\n"));
        output.push_str(&format!("║ Task: {}\n", self.task_id));
        output.push_str(&format!("║ Outcome: {}\n", self.outcome));
        output.push_str(&format!("║ Exit Code: {}\n", self.exit_code));
        output.push_str(&format!("║ Duration: {}ms\n", self.duration_ms));
        output.push_str(&format!("╚════════════════════════════════════════════════════════════════\n"));

        if !self.stdout.is_empty() {
            output.push_str("\n📋 STDOUT:\n");
            for line in self.stdout.lines() {
                output.push_str(&format!("  | {}\n", line));
            }
        }

        if !self.stderr.is_empty() {
            output.push_str("\n⚠️  STDERR:\n");
            for line in self.stderr.lines() {
                output.push_str(&format!("  ! {}\n", line));
            }
        }

        if let Some(error) = &self.error {
            output.push_str(&format!("\n❌ Error Details:\n"));
            output.push_str(&format!("  Category: {}\n", error.category));
            output.push_str(&format!("  Message:  {}\n", error.message));

            if let Some(suggestion) = &error.suggestion {
                output.push_str(&format!("  💡 Suggestion: {}\n", suggestion));
            }

            if !error.related_logs.is_empty() {
                output.push_str("\n  Related log lines:\n");
                for log in &error.related_logs {
                    output.push_str(&format!("    • {}\n", log));
                }
            }
        }

        if !self.outputs.is_empty() {
            output.push_str(&format!("\n📦 Outputs ({}):\n", self.outputs.len()));
            for path in &self.outputs {
                output.push_str(&format!("  ✓ {}\n", path.display()));
            }
        }

        output
    }

    /// Format log as JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Extract key metrics for monitoring
    pub fn metrics(&self) -> ExecutionMetrics {
        ExecutionMetrics {
            task_id: self.task_id.clone(),
            success: self.outcome == ExecutionOutcome::Success,
            duration_ms: self.duration_ms,
            stdout_lines: self.stdout.lines().count(),
            stderr_lines: self.stderr.lines().count(),
            output_files: self.outputs.len(),
        }
    }
}

/// Execution metrics for monitoring/statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    pub task_id: String,
    pub success: bool,
    pub duration_ms: u64,
    pub stdout_lines: usize,
    pub stderr_lines: usize,
    pub output_files: usize,
}

impl fmt::Display for ExecutionLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_log_success() {
        let result = SandboxResult {
            exit_code: 0,
            stdout: "Build completed successfully\n".to_string(),
            stderr: String::new(),
            duration_ms: 1234,
        };

        let log = ExecutionLog::from_sandbox_result(
            "test:compile".to_string(),
            &result,
            vec![PathBuf::from("/work/output.o")],
        );

        assert_eq!(log.outcome, ExecutionOutcome::Success);
        assert!(log.error.is_none());
        assert_eq!(log.outputs.len(), 1);
    }

    #[test]
    fn test_execution_log_failure() {
        let result = SandboxResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: "error: test.c:10: syntax error\n".to_string(),
            duration_ms: 500,
        };

        let log = ExecutionLog::from_sandbox_result(
            "test:compile".to_string(),
            &result,
            vec![],
        );

        assert_eq!(log.outcome, ExecutionOutcome::Failed);
        assert!(log.error.is_some());

        let error = log.error.unwrap();
        assert_eq!(error.category, ErrorCategory::CompilationError);
        assert!(!error.related_logs.is_empty());
    }

    #[test]
    fn test_error_categorization() {
        let test_cases = vec![
            ("error: No such file or directory", ErrorCategory::MissingDependency),
            ("Permission denied", ErrorCategory::PermissionError),
            ("error: test.c:10: syntax error", ErrorCategory::CompilationError),
            ("curl: failed to connect", ErrorCategory::NetworkError),
        ];

        for (stderr, expected_category) in test_cases {
            let result = SandboxResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: stderr.to_string(),
                duration_ms: 100,
            };

            let error = ExecutionLog::analyze_error(&result);
            assert_eq!(
                error.category, expected_category,
                "Failed to categorize: {}",
                stderr
            );
        }
    }

    #[test]
    fn test_log_formatting() {
        let result = SandboxResult {
            exit_code: 1,
            stdout: "Starting build...\n".to_string(),
            stderr: "error: compilation failed\n".to_string(),
            duration_ms: 456,
        };

        let log = ExecutionLog::from_sandbox_result(
            "example:compile".to_string(),
            &result,
            vec![],
        );

        let formatted = log.format_display();
        assert!(formatted.contains("example:compile"));
        assert!(formatted.contains("FAILED"));
        assert!(formatted.contains("Starting build"));
        assert!(formatted.contains("compilation failed"));
    }

    #[test]
    fn test_metrics() {
        let result = SandboxResult {
            exit_code: 0,
            stdout: "line1\nline2\nline3\n".to_string(),
            stderr: "warning\n".to_string(),
            duration_ms: 789,
        };

        let log = ExecutionLog::from_sandbox_result(
            "test:task".to_string(),
            &result,
            vec![PathBuf::from("/a"), PathBuf::from("/b")],
        );

        let metrics = log.metrics();
        assert!(metrics.success);
        assert_eq!(metrics.duration_ms, 789);
        assert_eq!(metrics.stdout_lines, 4); // 3 lines + empty line
        assert_eq!(metrics.stderr_lines, 2);
        assert_eq!(metrics.output_files, 2);
    }
}
