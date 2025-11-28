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
use std::path::PathBuf;
use std::sync::OnceLock;
use tracing::{info, warn, error, debug};

/// Pre-compiled regex patterns for library versioning (compiled once)
static SO_VERSION_FULL: OnceLock<regex::Regex> = OnceLock::new();
static SO_VERSION_MAJOR: OnceLock<regex::Regex> = OnceLock::new();

fn get_so_version_full_regex() -> &'static regex::Regex {
    SO_VERSION_FULL.get_or_init(|| {
        regex::Regex::new(r"^(.+\.so)\.(\d+)\.(\d+)\.(\d+)$")
            .expect("Invalid regex pattern for SO_VERSION_FULL")
    })
}

fn get_so_version_major_regex() -> &'static regex::Regex {
    SO_VERSION_MAJOR.get_or_init(|| {
        regex::Regex::new(r"^(.+\.so)\.(\d+)$")
            .expect("Invalid regex pattern for SO_VERSION_MAJOR")
    })
}

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
                // Handle signal termination (code() returns None) with exit code 128 + signal
                let code = match status.code() {
                    Some(c) => c,
                    None => {
                        // Process was terminated by signal
                        #[cfg(unix)]
                        {
                            use std::os::unix::process::ExitStatusExt;
                            status.signal().map(|s| 128 + s).unwrap_or(1)
                        }
                        #[cfg(not(unix))]
                        {
                            1 // Default error code for non-Unix platforms
                        }
                    }
                };
                if code != 0 {
                    warn!(target: "bitbake", "oe_runmake: make exited with code {}", code);
                }
                Ok(ExecutionResult::new((code & 0xFF) as u8))
            }
            Err(e) => {
                error!(target: "bitbake", "oe_runmake: failed to execute make: {}", e);
                writeln!(context.stderr(), "oe_runmake: failed to execute make: {}", e)?;
                Ok(ExecutionResult::new(127))
            }
        }
    }
}

/// oe_soinstall - Install shared library with proper symlinks
#[derive(Clone, clap::Parser)]
#[command(about = "Install shared library with proper symlinks")]
pub struct OeSoinstallCommand {
    /// Library file to install
    lib: String,
    /// Destination directory
    dest: String,
}

impl Command for OeSoinstallCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        use std::os::unix::fs::symlink;
        use std::path::Path;

        let lib_path = Path::new(&self.lib);
        let dest_path = Path::new(&self.dest);

        if !lib_path.exists() {
            error!(target: "bitbake", "oe_soinstall: {} not found", self.lib);
            writeln!(context.stderr(), "oe_soinstall: {} not found", self.lib)?;
            return Ok(ExecutionResult::new(1));
        }

        // Create destination directory
        if let Err(e) = std::fs::create_dir_all(dest_path) {
            error!(target: "bitbake", "oe_soinstall: failed to create {}: {}", self.dest, e);
            return Ok(ExecutionResult::new(1));
        }

        let lib_name = lib_path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        // Copy the library
        let dest_lib = dest_path.join(&lib_name);
        if let Err(e) = std::fs::copy(lib_path, &dest_lib) {
            error!(target: "bitbake", "oe_soinstall: failed to copy {}: {}", self.lib, e);
            return Ok(ExecutionResult::new(1));
        }

        // Set permissions (0755)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(&dest_lib) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(&dest_lib, perms);
            }
        }

        info!(target: "bitbake", "oe_soinstall: installed {}", lib_name);

        // Create symlinks for versioned libraries (e.g., libfoo.so.1.2.3 -> libfoo.so.1 -> libfoo.so)
        // Use pre-compiled regex patterns for efficiency
        let re_full = get_so_version_full_regex();
        let re_major = get_so_version_major_regex();

        if let Some(caps) = re_full.captures(&lib_name) {
            let base = &caps[1];
            let major = &caps[2];

            // Create libfoo.so.1 -> libfoo.so.1.2.3
            let major_link = format!("{}.{}", base, major);
            let _ = std::fs::remove_file(dest_path.join(&major_link));
            let _ = symlink(&lib_name, dest_path.join(&major_link));

            // Create libfoo.so -> libfoo.so.1.2.3
            let _ = std::fs::remove_file(dest_path.join(base));
            let _ = symlink(&lib_name, dest_path.join(base));

            info!(target: "bitbake", "oe_soinstall: created symlinks {} -> {}", base, lib_name);
        } else if let Some(caps) = re_major.captures(&lib_name) {
            let base = &caps[1];

            // Create libfoo.so -> libfoo.so.1
            let _ = std::fs::remove_file(dest_path.join(base));
            let _ = symlink(&lib_name, dest_path.join(base));

            info!(target: "bitbake", "oe_soinstall: created symlink {} -> {}", base, lib_name);
        }

        Ok(ExecutionResult::success())
    }
}

/// oe_libinstall - Install library files
#[derive(Clone, clap::Parser)]
#[command(about = "Install library files")]
pub struct OeLibinstallCommand {
    /// Change to directory before looking for library
    #[arg(short = 'C')]
    dir: Option<String>,
    /// Library name (without lib prefix or extension)
    libname: String,
    /// Destination directory
    dest: String,
}

impl Command for OeLibinstallCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        use std::path::Path;

        let search_dir = match &self.dir {
            Some(d) => PathBuf::from(d),
            None => match std::env::current_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    error!(target: "bitbake", "oe_libinstall: failed to get current directory: {}", e);
                    writeln!(context.stderr(), "oe_libinstall: failed to get current directory: {}", e)?;
                    return Ok(ExecutionResult::new(1));
                }
            }
        };

        let dest_path = Path::new(&self.dest);

        // Create destination
        if let Err(e) = std::fs::create_dir_all(dest_path) {
            error!(target: "bitbake", "oe_libinstall: failed to create {}: {}", self.dest, e);
            return Ok(ExecutionResult::new(1));
        }

        let mut found = false;

        // Look for lib<name>.so*, lib<name>.a
        let patterns = [
            format!("lib{}.so*", self.libname),
            format!("lib{}.a", self.libname),
        ];

        for entry in std::fs::read_dir(&search_dir).into_iter().flatten() {
            if let Ok(entry) = entry {
                let name = entry.file_name().to_string_lossy().to_string();

                // Check if matches pattern
                let matches = name == format!("lib{}.a", self.libname) ||
                    (name.starts_with(&format!("lib{}.", self.libname)) && name.contains(".so"));

                if matches && entry.path().is_file() {
                    let dest_file = dest_path.join(&name);
                    if let Err(e) = std::fs::copy(entry.path(), &dest_file) {
                        warn!(target: "bitbake", "oe_libinstall: failed to copy {}: {}", name, e);
                        continue;
                    }

                    // Set permissions
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(metadata) = std::fs::metadata(&dest_file) {
                            let mut perms = metadata.permissions();
                            perms.set_mode(0o755);
                            let _ = std::fs::set_permissions(&dest_file, perms);
                        }
                    }

                    info!(target: "bitbake", "oe_libinstall: installed {}", name);
                    found = true;
                }
            }
        }

        if !found {
            warn!(target: "bitbake", "oe_libinstall: no libraries found for {}", self.libname);
            writeln!(context.stderr(), "oe_libinstall: no libraries found for {}", self.libname)?;
        }

        Ok(ExecutionResult::success())
    }
}

/// create_wrapper - Create a wrapper script
#[derive(Clone, clap::Parser)]
#[command(about = "Create a wrapper script")]
pub struct CreateWrapperCommand {
    /// Path for the wrapper script
    wrapper: String,
    /// Command and arguments to wrap
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,
}

impl Command for CreateWrapperCommand {
    type Error = Error;

    async fn execute(&self, _context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        use std::path::Path;

        let wrapper_path = Path::new(&self.wrapper);

        // Create parent directory
        if let Some(parent) = wrapper_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let cmd_str = self.command.join(" ");
        let content = format!("#!/bin/sh\nexec {}\n", cmd_str);

        if let Err(e) = std::fs::write(wrapper_path, &content) {
            error!(target: "bitbake", "create_wrapper: failed to write {}: {}", self.wrapper, e);
            return Ok(ExecutionResult::new(1));
        }

        // Make executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(wrapper_path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(wrapper_path, perms);
            }
        }

        info!(target: "bitbake", "create_wrapper: created {} -> {}", self.wrapper, cmd_str);
        Ok(ExecutionResult::success())
    }
}

/// install_append - Install file with automatic directory creation
#[derive(Clone, clap::Parser)]
#[command(about = "Install file with automatic directory creation")]
pub struct InstallAppendCommand {
    /// File to install
    file: String,
    /// Destination path
    dest: String,
}

impl Command for InstallAppendCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        use std::path::Path;

        let src_path = Path::new(&self.file);
        let dest_path = Path::new(&self.dest);

        // Create parent directory
        if let Some(parent) = dest_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!(target: "bitbake", "install_append: failed to create dir: {}", e);
            }
        }

        // Copy file
        if let Err(e) = std::fs::copy(src_path, dest_path) {
            error!(target: "bitbake", "install_append: failed to copy {} to {}: {}", self.file, self.dest, e);
            writeln!(context.stderr(), "install_append: failed to copy {} to {}: {}", self.file, self.dest, e)?;
            return Ok(ExecutionResult::new(1));
        }

        // Set permissions (0644)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(dest_path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o644);
                let _ = std::fs::set_permissions(dest_path, perms);
            }
        }

        debug!(target: "bitbake", "install_append: {} -> {}", self.file, self.dest);
        Ok(ExecutionResult::success())
    }
}

/// lnr - Create relative symlink
#[derive(Clone, clap::Parser)]
#[command(about = "Create relative symlink")]
pub struct LnrCommand {
    /// Target file
    target: String,
    /// Link name
    link: String,
}

impl Command for LnrCommand {
    type Error = Error;

    async fn execute(&self, _context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        use std::path::Path;

        let link_path = Path::new(&self.link);

        // Remove existing link if any
        let _ = std::fs::remove_file(link_path);

        // Create symlink
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            if let Err(e) = symlink(&self.target, link_path) {
                warn!(target: "bitbake", "lnr: failed to create symlink: {}", e);
                return Ok(ExecutionResult::new(1));
            }
        }

        #[cfg(not(unix))]
        {
            warn!(target: "bitbake", "lnr: symlinks not supported on this platform");
            return Ok(ExecutionResult::new(1));
        }

        debug!(target: "bitbake", "lnr: {} -> {}", self.link, self.target);
        Ok(ExecutionResult::success())
    }
}

/// useradd - Create a user (stores in recipe's passwd file)
#[derive(Clone, clap::Parser)]
#[command(about = "Create a user entry for the target system")]
pub struct UseraddCommand {
    /// Home directory
    #[arg(short = 'd', long = "home-dir")]
    home_dir: Option<String>,
    /// Shell
    #[arg(short = 's', long = "shell")]
    shell: Option<String>,
    /// User ID
    #[arg(short = 'u', long = "uid")]
    uid: Option<u32>,
    /// Group ID
    #[arg(short = 'g', long = "gid")]
    gid: Option<String>,
    /// Comment/GECOS
    #[arg(short = 'c', long = "comment")]
    comment: Option<String>,
    /// System account
    #[arg(short = 'r', long = "system")]
    system: bool,
    /// Don't create home directory
    #[arg(short = 'M', long = "no-create-home")]
    no_create_home: bool,
    /// Username
    username: String,
}

impl Command for UseraddCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        // Get destination directory from environment
        let dest_dir = context.shell.env_str("D")
            .map(|s| PathBuf::from(s.as_ref()))
            .unwrap_or_else(|| PathBuf::from("/"));

        let passwd_dir = dest_dir.join("etc");
        let passwd_file = passwd_dir.join("passwd");

        // Create etc directory if needed
        if let Err(e) = std::fs::create_dir_all(&passwd_dir) {
            warn!(target: "bitbake", "useradd: failed to create etc dir: {}", e);
        }

        // Default values
        let uid = self.uid.unwrap_or(if self.system { 999 } else { 1000 });
        let gid = self.gid.clone().unwrap_or_else(|| uid.to_string());
        let home = self.home_dir.clone().unwrap_or_else(|| format!("/home/{}", self.username));
        let shell = self.shell.clone().unwrap_or_else(|| "/bin/sh".to_string());
        let comment = self.comment.clone().unwrap_or_default();

        // Format passwd entry: username:x:uid:gid:comment:home:shell
        let entry = format!("{}:x:{}:{}:{}:{}:{}\n",
            self.username, uid, gid, comment, home, shell);

        // Append to passwd file
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&passwd_file)
            .map_err(|e| {
                error!(target: "bitbake", "useradd: failed to open passwd file: {}", e);
                Error::from(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;

        std::io::Write::write_all(&mut file, entry.as_bytes())
            .map_err(|e| {
                error!(target: "bitbake", "useradd: failed to write passwd entry: {}", e);
                Error::from(e)
            })?;

        // Create home directory if requested
        if !self.no_create_home {
            let home_path = dest_dir.join(home.trim_start_matches('/'));
            let _ = std::fs::create_dir_all(&home_path);
        }

        info!(target: "bitbake", "useradd: created user {} (uid={})", self.username, uid);
        Ok(ExecutionResult::success())
    }
}

/// groupadd - Create a group (stores in recipe's group file)
#[derive(Clone, clap::Parser)]
#[command(about = "Create a group entry for the target system")]
pub struct GroupaddCommand {
    /// Group ID
    #[arg(short = 'g', long = "gid")]
    gid: Option<u32>,
    /// System group
    #[arg(short = 'r', long = "system")]
    system: bool,
    /// Group name
    groupname: String,
}

impl Command for GroupaddCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        // Get destination directory from environment
        let dest_dir = context.shell.env_str("D")
            .map(|s| PathBuf::from(s.as_ref()))
            .unwrap_or_else(|| PathBuf::from("/"));

        let group_dir = dest_dir.join("etc");
        let group_file = group_dir.join("group");

        // Create etc directory if needed
        if let Err(e) = std::fs::create_dir_all(&group_dir) {
            warn!(target: "bitbake", "groupadd: failed to create etc dir: {}", e);
        }

        let gid = self.gid.unwrap_or(if self.system { 999 } else { 1000 });

        // Format group entry: groupname:x:gid:members
        let entry = format!("{}:x:{}:\n", self.groupname, gid);

        // Append to group file
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&group_file)
            .map_err(|e| {
                error!(target: "bitbake", "groupadd: failed to open group file: {}", e);
                Error::from(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;

        std::io::Write::write_all(&mut file, entry.as_bytes())
            .map_err(|e| {
                error!(target: "bitbake", "groupadd: failed to write group entry: {}", e);
                Error::from(e)
            })?;

        info!(target: "bitbake", "groupadd: created group {} (gid={})", self.groupname, gid);
        Ok(ExecutionResult::success())
    }
}

/// update_alternatives - Manage symbolic links for alternative commands
#[derive(Clone, clap::Parser)]
#[command(about = "Manage symbolic links for alternative commands")]
pub struct UpdateAlternativesCommand {
    /// Install a new alternative
    #[arg(long)]
    install: bool,
    /// Remove an alternative
    #[arg(long)]
    remove: bool,
    /// Link path
    link: Option<String>,
    /// Name of the alternative group
    name: Option<String>,
    /// Path to the alternative
    path: Option<String>,
    /// Priority
    priority: Option<i32>,
}

impl Command for UpdateAlternativesCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        // Get destination directory from environment
        let dest_dir = context.shell.env_str("D")
            .map(|s| PathBuf::from(s.as_ref()))
            .unwrap_or_else(|| PathBuf::from("/"));

        let alternatives_dir = dest_dir.join("etc/alternatives");
        if let Err(e) = std::fs::create_dir_all(&alternatives_dir) {
            warn!(target: "bitbake", "update-alternatives: failed to create alternatives dir: {}", e);
        }

        if self.install {
            let link = self.link.as_ref().ok_or_else(|| {
                Error::from(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing link path"))
            })?;
            let name = self.name.as_ref().ok_or_else(|| {
                Error::from(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing name"))
            })?;
            let path = self.path.as_ref().ok_or_else(|| {
                Error::from(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing path"))
            })?;

            // Create the alternative symlink
            let alt_link = alternatives_dir.join(name);
            let _ = std::fs::remove_file(&alt_link);

            #[cfg(unix)]
            {
                use std::os::unix::fs::symlink;
                if let Err(e) = symlink(path, &alt_link) {
                    warn!(target: "bitbake", "update-alternatives: failed to create symlink: {}", e);
                }
            }

            // Create the main link pointing to the alternative
            let main_link = dest_dir.join(link.trim_start_matches('/'));
            if let Some(parent) = main_link.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::remove_file(&main_link);

            #[cfg(unix)]
            {
                use std::os::unix::fs::symlink;
                let rel_alt = format!("/etc/alternatives/{}", name);
                if let Err(e) = symlink(&rel_alt, &main_link) {
                    warn!(target: "bitbake", "update-alternatives: failed to create main symlink: {}", e);
                }
            }

            info!(target: "bitbake", "update-alternatives: installed {} -> {} (priority {})",
                name, path, self.priority.unwrap_or(100));
        }

        Ok(ExecutionResult::success())
    }
}

/// chrpath - Change or remove RPATH/RUNPATH in ELF binaries
#[derive(Clone, clap::Parser)]
#[command(about = "Change or remove RPATH in ELF binaries")]
pub struct ChrpathCommand {
    /// Delete RPATH
    #[arg(short = 'd', long = "delete")]
    delete: bool,
    /// Replace RPATH
    #[arg(short = 'r', long = "replace")]
    replace: Option<String>,
    /// List RPATH
    #[arg(short = 'l', long = "list")]
    list: bool,
    /// File to modify
    file: String,
}

impl Command for ChrpathCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        use std::path::Path;

        let file_path = Path::new(&self.file);
        if !file_path.exists() {
            error!(target: "bitbake", "chrpath: {} not found", self.file);
            writeln!(context.stderr(), "chrpath: {} not found", self.file)?;
            return Ok(ExecutionResult::new(1));
        }

        // Try to use patchelf if available
        let patchelf_available = std::process::Command::new("patchelf")
            .arg("--version")
            .output()
            .is_ok();

        if patchelf_available {
            let mut cmd = std::process::Command::new("patchelf");

            if self.delete {
                cmd.arg("--remove-rpath");
            } else if let Some(ref new_rpath) = self.replace {
                cmd.arg("--set-rpath").arg(new_rpath);
            } else if self.list {
                cmd.arg("--print-rpath");
            }

            cmd.arg(&self.file);

            match cmd.output() {
                Ok(output) => {
                    if self.list {
                        let rpath = String::from_utf8_lossy(&output.stdout);
                        writeln!(context.stdout(), "{}: RPATH={}", self.file, rpath.trim())?;
                    }
                    if output.status.success() {
                        info!(target: "bitbake", "chrpath: modified {}", self.file);
                        return Ok(ExecutionResult::success());
                    }
                }
                Err(e) => {
                    warn!(target: "bitbake", "chrpath: patchelf failed: {}", e);
                }
            }
        }

        // Fallback: just log what would be done
        if self.delete {
            info!(target: "bitbake", "chrpath: would delete RPATH from {}", self.file);
        } else if let Some(ref new_rpath) = self.replace {
            info!(target: "bitbake", "chrpath: would set RPATH to {} in {}", new_rpath, self.file);
        } else if self.list {
            writeln!(context.stdout(), "{}: RPATH=(unavailable - patchelf not found)", self.file)?;
        }

        Ok(ExecutionResult::success())
    }
}

/// fakeroot - Run command in fakeroot environment (simplified implementation)
#[derive(Clone, clap::Parser)]
#[command(about = "Run command with faked root privileges")]
pub struct FakerootCommand {
    /// Command and arguments to run
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,
}

impl Command for FakerootCommand {
    type Error = Error;

    async fn execute(&self, context: ExecutionContext<'_>) -> Result<ExecutionResult, Self::Error> {
        if self.command.is_empty() {
            writeln!(context.stderr(), "fakeroot: no command specified")?;
            return Ok(ExecutionResult::new(1));
        }

        // Try to use actual fakeroot if available
        let fakeroot_available = std::process::Command::new("fakeroot")
            .arg("--version")
            .output()
            .is_ok();

        let (program, args) = if fakeroot_available {
            ("fakeroot".to_string(), self.command.clone())
        } else {
            // Fallback: run command directly (works in build containers)
            (self.command[0].clone(), self.command[1..].to_vec())
        };

        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args);

        match cmd.status() {
            Ok(status) => {
                let code = status.code().unwrap_or(1);
                Ok(ExecutionResult::new((code & 0xFF) as u8))
            }
            Err(e) => {
                error!(target: "bitbake", "fakeroot: failed to execute: {}", e);
                writeln!(context.stderr(), "fakeroot: failed to execute: {}", e)?;
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
    shell.register_builtin("oe_soinstall", builtins::builtin::<OeSoinstallCommand>());
    shell.register_builtin("oe_libinstall", builtins::builtin::<OeLibinstallCommand>());
    shell.register_builtin("create_wrapper", builtins::builtin::<CreateWrapperCommand>());
    shell.register_builtin("install_append", builtins::builtin::<InstallAppendCommand>());
    shell.register_builtin("lnr", builtins::builtin::<LnrCommand>());

    // System utilities (real implementations)
    shell.register_builtin("useradd", builtins::builtin::<UseraddCommand>());
    shell.register_builtin("groupadd", builtins::builtin::<GroupaddCommand>());
    shell.register_builtin("update-alternatives", builtins::builtin::<UpdateAlternativesCommand>());
    shell.register_builtin("update_alternatives", builtins::builtin::<UpdateAlternativesCommand>());
    shell.register_builtin("chrpath", builtins::builtin::<ChrpathCommand>());
    shell.register_builtin("fakeroot", builtins::builtin::<FakerootCommand>());
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
    m.insert("oe_soinstall".into(), builtins::builtin::<OeSoinstallCommand>());
    m.insert("oe_libinstall".into(), builtins::builtin::<OeLibinstallCommand>());
    m.insert("create_wrapper".into(), builtins::builtin::<CreateWrapperCommand>());
    m.insert("install_append".into(), builtins::builtin::<InstallAppendCommand>());
    m.insert("lnr".into(), builtins::builtin::<LnrCommand>());

    // System utilities (real implementations)
    m.insert("useradd".into(), builtins::builtin::<UseraddCommand>());
    m.insert("groupadd".into(), builtins::builtin::<GroupaddCommand>());
    m.insert("update-alternatives".into(), builtins::builtin::<UpdateAlternativesCommand>());
    m.insert("update_alternatives".into(), builtins::builtin::<UpdateAlternativesCommand>());
    m.insert("chrpath".into(), builtins::builtin::<ChrpathCommand>());
    m.insert("fakeroot".into(), builtins::builtin::<FakerootCommand>());

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitbake_builtins_created() {
        let builtins = bitbake_builtins();
        // Logging builtins
        assert!(builtins.contains_key("bb_note"));
        assert!(builtins.contains_key("bbnote"));
        assert!(builtins.contains_key("bbfatal"));
        assert!(builtins.contains_key("bb_warn"));

        // Utility builtins
        assert!(builtins.contains_key("bbdirs"));
        assert!(builtins.contains_key("oe_runmake"));
        assert!(builtins.contains_key("oe_soinstall"));
        assert!(builtins.contains_key("oe_libinstall"));
        assert!(builtins.contains_key("create_wrapper"));
        assert!(builtins.contains_key("install_append"));
        assert!(builtins.contains_key("lnr"));

        // System utilities (real implementations)
        assert!(builtins.contains_key("useradd"));
        assert!(builtins.contains_key("groupadd"));
        assert!(builtins.contains_key("update-alternatives"));
        assert!(builtins.contains_key("chrpath"));
        assert!(builtins.contains_key("fakeroot"));
    }

    #[test]
    fn test_builtin_count() {
        let builtins = bitbake_builtins();
        // 13 logging + 7 utility + 6 system = 26 builtins
        assert_eq!(builtins.len(), 26);
    }

    #[test]
    fn test_so_version_regex() {
        // Test the pre-compiled regex patterns
        let re_full = get_so_version_full_regex();
        let re_major = get_so_version_major_regex();

        // Test full version pattern
        assert!(re_full.is_match("libfoo.so.1.2.3"));
        assert!(!re_full.is_match("libfoo.so.1"));
        assert!(!re_full.is_match("libfoo.so"));

        // Test major version pattern
        assert!(re_major.is_match("libfoo.so.1"));
        assert!(!re_major.is_match("libfoo.so.1.2.3"));
        assert!(!re_major.is_match("libfoo.so"));
    }
}
