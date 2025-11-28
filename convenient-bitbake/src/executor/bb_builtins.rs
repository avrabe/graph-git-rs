//! BitBake shell built-in commands for brush-shell integration
//!
//! Provides native Rust implementations of BitBake's logging and utility functions
//! as shell builtins that can be registered with brush-shell.
//!
//! All logging is also sent to the tracing framework, enabling centralized log
//! collection and post-hoc analysis of task execution.

use brush_core::builtins::{self, Command, Registration};
use brush_core::commands::ExecutionContext;
use brush_core::error::Error;
use brush_core::results::ExecutionResult;
use std::io::Write;
use tracing::{info, warn, error, debug};

/// Create a simple builtin that logs to stdout/stderr AND tracing
macro_rules! logging_builtin {
    ($name:ident, $prefix:literal, $use_stderr:expr, $trace_level:ident) => {
        #[derive(Clone, clap::Parser)]
        #[command(about = concat!("BitBake ", $prefix, " logging function"))]
        pub struct $name {
            /// Message to log
            #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
            message: Vec<String>,
        }

        impl Command for $name {
            type Error = Error;

            async fn execute(
                &self,
                context: ExecutionContext<'_>,
            ) -> Result<ExecutionResult, Self::Error> {
                let msg = self.message.join(" ");

                // Log to tracing framework for centralized logging
                $trace_level!(target: "bitbake", "{}: {}", $prefix, msg);

                // Also write to shell stdout/stderr
                if $use_stderr {
                    writeln!(context.stderr(), "{}: {}", $prefix, msg)?;
                } else {
                    writeln!(context.stdout(), "{}: {}", $prefix, msg)?;
                }
                Ok(ExecutionResult::success())
            }
        }
    };
}

// Define BitBake logging builtins with tracing integration
logging_builtin!(BbNoteCommand, "NOTE", false, info);
logging_builtin!(BbWarnCommand, "WARNING", true, warn);
logging_builtin!(BbErrorCommand, "ERROR", true, error);
logging_builtin!(BbDebugCommand, "DEBUG", true, debug);

/// bb_plain - Plain output without prefix
#[derive(Clone, clap::Parser)]
#[command(about = "BitBake plain output function")]
pub struct BbPlainCommand {
    /// Message to output
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    message: Vec<String>,
}

impl Command for BbPlainCommand {
    type Error = Error;

    async fn execute(
        &self,
        context: ExecutionContext<'_>,
    ) -> Result<ExecutionResult, Self::Error> {
        let msg = self.message.join(" ");
        info!(target: "bitbake", "{}", msg);
        writeln!(context.stdout(), "{}", msg)?;
        Ok(ExecutionResult::success())
    }
}

/// bb_fatal - Fatal error that exits the script
#[derive(Clone, clap::Parser)]
#[command(about = "BitBake fatal error function - logs and exits")]
pub struct BbFatalCommand {
    /// Message to log
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    message: Vec<String>,
}

impl Command for BbFatalCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        let msg = self.message.join(" ");
        // Log to tracing as error level
        error!(target: "bitbake", "FATAL: {}", msg);
        writeln!(context.stderr(), "FATAL: {}", msg)?;
        // Return exit code 1 to stop execution
        Ok(ExecutionResult::new(1))
    }
}

/// bbdirs - Create directories if they don't exist
#[derive(Clone, clap::Parser)]
#[command(about = "Create directories if they don't exist")]
pub struct BbDirsCommand {
    /// Directories to create
    #[arg(trailing_var_arg = true)]
    dirs: Vec<String>,
}

impl Command for BbDirsCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        for dir in &self.dirs {
            debug!(target: "bitbake", "bbdirs: creating directory {}", dir);
            if let Err(e) = std::fs::create_dir_all(dir) {
                error!(target: "bitbake", "bbdirs: failed to create {}: {}", dir, e);
                writeln!(context.stderr(), "bbdirs: failed to create {}: {}", dir, e)?;
                return Ok(ExecutionResult::new(1));
            }
        }
        Ok(ExecutionResult::success())
    }
}

/// oe_runmake - Run make with parallel jobs
#[derive(Clone, clap::Parser)]
#[command(about = "Run make with parallel jobs")]
pub struct OeRunmakeCommand {
    /// Arguments to pass to make
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

impl Command for OeRunmakeCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        // Get PARALLEL_MAKE from environment or default to -j4
        let parallel = context
            .shell
            .env_str("PARALLEL_MAKE")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "-j4".to_string());

        // Build make command
        let mut cmd = std::process::Command::new("make");

        // Add parallel flag
        for part in parallel.split_whitespace() {
            cmd.arg(part);
        }

        // Add user arguments
        for arg in &self.args {
            cmd.arg(arg);
        }

        info!(target: "bitbake", "oe_runmake: running make with {} args: {:?}", self.args.len(), self.args);

        // Execute make
        match cmd.status() {
            Ok(status) => {
                let code = status.code().unwrap_or(1);
                if code != 0 {
                    warn!(target: "bitbake", "oe_runmake: make exited with code {}", code);
                }
                Ok(ExecutionResult::new(code as u8))
            }
            Err(e) => {
                error!(target: "bitbake", "oe_runmake: failed to execute make: {}", e);
                writeln!(context.stderr(), "oe_runmake: failed to execute make: {}", e)?;
                Ok(ExecutionResult::new(127))
            }
        }
    }
}

/// Register all BitBake builtins with a brush shell instance
pub fn register_bitbake_builtins(shell: &mut brush_core::Shell) {
    // Logging functions
    shell.register_builtin("bb_note", builtins::builtin::<BbNoteCommand>());
    shell.register_builtin("bbnote", builtins::builtin::<BbNoteCommand>());
    shell.register_builtin("bb_warn", builtins::builtin::<BbWarnCommand>());
    shell.register_builtin("bbwarn", builtins::builtin::<BbWarnCommand>());
    shell.register_builtin("bb_error", builtins::builtin::<BbErrorCommand>());
    shell.register_builtin("bberror", builtins::builtin::<BbErrorCommand>());
    shell.register_builtin("bb_fatal", builtins::builtin::<BbFatalCommand>());
    shell.register_builtin("bbfatal", builtins::builtin::<BbFatalCommand>());
    shell.register_builtin("bbfatal_log", builtins::builtin::<BbFatalCommand>());
    shell.register_builtin("bb_debug", builtins::builtin::<BbDebugCommand>());
    shell.register_builtin("bbdebug", builtins::builtin::<BbDebugCommand>());
    shell.register_builtin("bb_plain", builtins::builtin::<BbPlainCommand>());
    shell.register_builtin("bbplain", builtins::builtin::<BbPlainCommand>());

    // Utility functions
    shell.register_builtin("bbdirs", builtins::builtin::<BbDirsCommand>());
    shell.register_builtin("oe_runmake", builtins::builtin::<OeRunmakeCommand>());
}

/// Get a HashMap of BitBake builtin registrations (for use with ShellBuilder)
pub fn bitbake_builtins() -> std::collections::HashMap<String, Registration> {
    let mut m = std::collections::HashMap::new();

    // Logging functions
    m.insert("bb_note".into(), builtins::builtin::<BbNoteCommand>());
    m.insert("bbnote".into(), builtins::builtin::<BbNoteCommand>());
    m.insert("bb_warn".into(), builtins::builtin::<BbWarnCommand>());
    m.insert("bbwarn".into(), builtins::builtin::<BbWarnCommand>());
    m.insert("bb_error".into(), builtins::builtin::<BbErrorCommand>());
    m.insert("bberror".into(), builtins::builtin::<BbErrorCommand>());
    m.insert("bb_fatal".into(), builtins::builtin::<BbFatalCommand>());
    m.insert("bbfatal".into(), builtins::builtin::<BbFatalCommand>());
    m.insert("bbfatal_log".into(), builtins::builtin::<BbFatalCommand>());
    m.insert("bb_debug".into(), builtins::builtin::<BbDebugCommand>());
    m.insert("bbdebug".into(), builtins::builtin::<BbDebugCommand>());
    m.insert("bb_plain".into(), builtins::builtin::<BbPlainCommand>());
    m.insert("bbplain".into(), builtins::builtin::<BbPlainCommand>());

    // Utility functions
    m.insert("bbdirs".into(), builtins::builtin::<BbDirsCommand>());
    m.insert("oe_runmake".into(), builtins::builtin::<OeRunmakeCommand>());

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitbake_builtins_created() {
        let builtins = bitbake_builtins();
        assert!(builtins.contains_key("bb_note"));
        assert!(builtins.contains_key("bbfatal"));
        assert!(builtins.contains_key("bbdirs"));
        assert!(builtins.contains_key("oe_runmake"));
    }
}
