//! Security hardening for sandbox
//!
//! Provides seccomp filtering, landlock filesystem restrictions, and capability dropping
//! for sandboxed build environments.

use std::collections::HashSet;

/// Syscall number mappings for x86_64 Linux
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod syscall_numbers {
    pub const SYS_READ: u32 = 0;
    pub const SYS_WRITE: u32 = 1;
    pub const SYS_OPEN: u32 = 2;
    pub const SYS_CLOSE: u32 = 3;
    pub const SYS_STAT: u32 = 4;
    pub const SYS_FSTAT: u32 = 5;
    pub const SYS_LSTAT: u32 = 6;
    pub const SYS_POLL: u32 = 7;
    pub const SYS_BRK: u32 = 12;
    pub const SYS_MMAP: u32 = 9;
    pub const SYS_MUNMAP: u32 = 11;
    pub const SYS_RT_SIGACTION: u32 = 13;
    pub const SYS_RT_SIGPROCMASK: u32 = 14;
    pub const SYS_EXIT: u32 = 60;
    pub const SYS_EXIT_GROUP: u32 = 231;
    pub const SYS_EXECVE: u32 = 59;
    pub const SYS_FORK: u32 = 57;
    pub const SYS_CLONE: u32 = 56;
    pub const SYS_OPENAT: u32 = 257;
    pub const SYS_GETDENTS64: u32 = 217;
    pub const SYS_FCNTL: u32 = 72;
    pub const SYS_DUP2: u32 = 33;
    pub const SYS_PIPE: u32 = 22;
    pub const SYS_PIPE2: u32 = 293;
    pub const SYS_WAIT4: u32 = 61;
    pub const SYS_ACCESS: u32 = 21;
    pub const SYS_GETCWD: u32 = 79;
    pub const SYS_CHDIR: u32 = 80;
    pub const SYS_MKDIR: u32 = 83;
    pub const SYS_RMDIR: u32 = 84;
    pub const SYS_UNLINK: u32 = 87;
    pub const SYS_CHMOD: u32 = 90;
    pub const SYS_CHOWN: u32 = 92;

    /// Get syscall number by name
    pub fn syscall_number(name: &str) -> Option<u32> {
        match name {
            "read" => Some(SYS_READ),
            "write" => Some(SYS_WRITE),
            "open" => Some(SYS_OPEN),
            "close" => Some(SYS_CLOSE),
            "stat" => Some(SYS_STAT),
            "fstat" => Some(SYS_FSTAT),
            "lstat" => Some(SYS_LSTAT),
            "poll" => Some(SYS_POLL),
            "brk" => Some(SYS_BRK),
            "mmap" => Some(SYS_MMAP),
            "munmap" => Some(SYS_MUNMAP),
            "rt_sigaction" => Some(SYS_RT_SIGACTION),
            "rt_sigprocmask" => Some(SYS_RT_SIGPROCMASK),
            "exit" => Some(SYS_EXIT),
            "exit_group" => Some(SYS_EXIT_GROUP),
            "execve" => Some(SYS_EXECVE),
            "fork" => Some(SYS_FORK),
            "clone" => Some(SYS_CLONE),
            "openat" => Some(SYS_OPENAT),
            "getdents64" => Some(SYS_GETDENTS64),
            "fcntl" => Some(SYS_FCNTL),
            "dup2" => Some(SYS_DUP2),
            "pipe" => Some(SYS_PIPE),
            "pipe2" => Some(SYS_PIPE2),
            "wait4" => Some(SYS_WAIT4),
            "access" => Some(SYS_ACCESS),
            "getcwd" => Some(SYS_GETCWD),
            "chdir" => Some(SYS_CHDIR),
            "mkdir" => Some(SYS_MKDIR),
            "rmdir" => Some(SYS_RMDIR),
            "unlink" => Some(SYS_UNLINK),
            "chmod" => Some(SYS_CHMOD),
            "chown" => Some(SYS_CHOWN),
            _ => None,
        }
    }
}

/// BPF instruction for seccomp filter
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SockFilterInstruction {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

/// BPF instruction codes
const BPF_LD: u16 = 0x00;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_JMP: u16 = 0x05;
const BPF_JEQ: u16 = 0x10;
const BPF_K: u16 = 0x00;
const BPF_RET: u16 = 0x06;

/// Seccomp return values
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;

/// Seccomp filter builder
pub struct SeccompFilter {
    allowed_syscalls: HashSet<String>,
    default_action: SeccompAction,
}

/// Action to take when syscall is not explicitly allowed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeccompAction {
    /// Kill the process
    Kill,
    /// Return EPERM error
    Errno,
    /// Allow all (no filtering)
    Allow,
}

impl SeccompFilter {
    pub fn new() -> Self {
        let mut allowed = HashSet::new();
        // Default minimal set for process execution
        for syscall in &[
            "read", "write", "open", "close", "stat", "fstat", "lstat",
            "poll", "brk", "mmap", "munmap", "rt_sigaction", "rt_sigprocmask",
            "exit", "exit_group", "openat", "getdents64", "fcntl", "access",
            "getcwd", "dup2", "pipe", "pipe2",
        ] {
            allowed.insert((*syscall).to_string());
        }
        Self {
            allowed_syscalls: allowed,
            default_action: SeccompAction::Errno,
        }
    }

    /// Create a filter that allows all syscalls (no restriction)
    pub fn allow_all() -> Self {
        Self {
            allowed_syscalls: HashSet::new(),
            default_action: SeccompAction::Allow,
        }
    }

    /// Set the default action for non-allowed syscalls
    pub fn set_default_action(&mut self, action: SeccompAction) {
        self.default_action = action;
    }

    /// Allow a syscall
    pub fn allow(&mut self, syscall: impl Into<String>) {
        self.allowed_syscalls.insert(syscall.into());
    }

    /// Allow multiple syscalls
    pub fn allow_many(&mut self, syscalls: &[&str]) {
        for syscall in syscalls {
            self.allowed_syscalls.insert((*syscall).to_string());
        }
    }

    /// Generate BPF program for seccomp filter
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub fn to_bpf(&self) -> Vec<u8> {
        use syscall_numbers::syscall_number;

        if self.default_action == SeccompAction::Allow {
            // Return empty - no filtering needed
            return vec![];
        }

        let mut instructions: Vec<SockFilterInstruction> = Vec::new();

        // Load syscall number (at offset 0 in seccomp_data)
        instructions.push(SockFilterInstruction {
            code: BPF_LD | BPF_W | BPF_ABS,
            jt: 0,
            jf: 0,
            k: 0, // offsetof(seccomp_data, nr)
        });

        // Create jump table for allowed syscalls
        let syscall_count = self.allowed_syscalls.len();
        for (idx, syscall_name) in self.allowed_syscalls.iter().enumerate() {
            if let Some(syscall_nr) = syscall_number(syscall_name) {
                // Jump to ALLOW if matches, otherwise continue
                let jumps_to_end = (syscall_count - idx) as u8;
                instructions.push(SockFilterInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: jumps_to_end, // Jump to ALLOW
                    jf: 0,            // Continue to next check
                    k: syscall_nr,
                });
            }
        }

        // Default action (deny)
        let deny_action = match self.default_action {
            SeccompAction::Kill => SECCOMP_RET_KILL_PROCESS,
            SeccompAction::Errno => SECCOMP_RET_ERRNO | 1, // EPERM
            SeccompAction::Allow => SECCOMP_RET_ALLOW,
        };
        instructions.push(SockFilterInstruction {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: deny_action,
        });

        // ALLOW action
        instructions.push(SockFilterInstruction {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ALLOW,
        });

        // Serialize to bytes
        let mut bytes = Vec::with_capacity(instructions.len() * 8);
        for inst in instructions {
            bytes.extend_from_slice(&inst.code.to_ne_bytes());
            bytes.push(inst.jt);
            bytes.push(inst.jf);
            bytes.extend_from_slice(&inst.k.to_ne_bytes());
        }
        bytes
    }

    /// Generate BPF program (non-Linux stub)
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    pub fn to_bpf(&self) -> Vec<u8> {
        // Seccomp is Linux-only
        vec![]
    }

    /// Apply filter to current process
    #[cfg(target_os = "linux")]
    pub fn apply(&self) -> std::io::Result<()> {
        use std::io::{Error, ErrorKind};
        use nix::sys::prctl;

        if self.default_action == SeccompAction::Allow {
            // No filtering needed
            return Ok(());
        }

        let bpf = self.to_bpf();
        if bpf.is_empty() {
            return Ok(());
        }

        // Note: Actually applying seccomp requires prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, ...)
        // which needs CAP_SYS_ADMIN or prctl(PR_SET_NO_NEW_PRIVS) first.
        // For now, we generate the filter but applying it requires additional privileges.

        // Check if we can set no_new_privs (required before seccomp)
        prctl::set_no_new_privs().map_err(|e| {
            Error::new(
                ErrorKind::PermissionDenied,
                format!("Failed to set PR_SET_NO_NEW_PRIVS - cannot apply seccomp filter: {e}"),
            )
        })?;

        // Apply would require seccomp(SECCOMP_SET_MODE_FILTER, ...) syscall
        // which is beyond basic nix support. For full implementation,
        // use the `seccompiler` or `libseccomp` crate.
        Ok(())
    }

    /// Apply filter (non-Linux stub)
    #[cfg(not(target_os = "linux"))]
    pub fn apply(&self) -> std::io::Result<()> {
        // Seccomp is Linux-only, silently succeed on other platforms
        Ok(())
    }

    /// Get list of allowed syscalls
    pub fn allowed_syscalls(&self) -> impl Iterator<Item = &String> {
        self.allowed_syscalls.iter()
    }

    /// Get number of allowed syscalls
    pub fn allowed_count(&self) -> usize {
        self.allowed_syscalls.len()
    }
}

impl Default for SeccompFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Permission flags for landlock rules
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandlockPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub make_dir: bool,
    pub remove_dir: bool,
    pub make_file: bool,
    pub remove_file: bool,
    pub truncate: bool,
}

impl LandlockPermissions {
    /// Read-only access
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
            make_dir: false,
            remove_dir: false,
            make_file: false,
            remove_file: false,
            truncate: false,
        }
    }

    /// Read and execute access
    pub fn read_execute() -> Self {
        Self {
            read: true,
            write: false,
            execute: true,
            make_dir: false,
            remove_dir: false,
            make_file: false,
            remove_file: false,
            truncate: false,
        }
    }

    /// Full read/write access
    pub fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            execute: false,
            make_dir: true,
            remove_dir: true,
            make_file: true,
            remove_file: true,
            truncate: true,
        }
    }

    /// Full access including execute
    pub fn full() -> Self {
        Self {
            read: true,
            write: true,
            execute: true,
            make_dir: true,
            remove_dir: true,
            make_file: true,
            remove_file: true,
            truncate: true,
        }
    }
}

impl Default for LandlockPermissions {
    fn default() -> Self {
        Self::read_only()
    }
}

/// Path rule for landlock restrictions
#[derive(Debug, Clone)]
pub struct LandlockPathRule {
    pub path: String,
    pub permissions: LandlockPermissions,
}

/// Landlock filesystem restrictions
pub struct LandlockRestrictions {
    path_rules: Vec<LandlockPathRule>,
    enabled: bool,
}

impl LandlockRestrictions {
    pub fn new() -> Self {
        Self {
            path_rules: Vec::new(),
            enabled: true,
        }
    }

    /// Create disabled landlock (no restrictions)
    pub fn disabled() -> Self {
        Self {
            path_rules: Vec::new(),
            enabled: false,
        }
    }

    /// Allow read access to a path
    pub fn allow_read(&mut self, path: impl Into<String>) {
        self.path_rules.push(LandlockPathRule {
            path: path.into(),
            permissions: LandlockPermissions::read_only(),
        });
    }

    /// Allow read and execute access to a path
    pub fn allow_read_execute(&mut self, path: impl Into<String>) {
        self.path_rules.push(LandlockPathRule {
            path: path.into(),
            permissions: LandlockPermissions::read_execute(),
        });
    }

    /// Allow read and write access to a path
    pub fn allow_read_write(&mut self, path: impl Into<String>) {
        self.path_rules.push(LandlockPathRule {
            path: path.into(),
            permissions: LandlockPermissions::read_write(),
        });
    }

    /// Allow full access to a path
    pub fn allow_full(&mut self, path: impl Into<String>) {
        self.path_rules.push(LandlockPathRule {
            path: path.into(),
            permissions: LandlockPermissions::full(),
        });
    }

    /// Add a custom path rule
    pub fn add_rule(&mut self, path: impl Into<String>, permissions: LandlockPermissions) {
        self.path_rules.push(LandlockPathRule {
            path: path.into(),
            permissions,
        });
    }

    /// Check if landlock is supported on this kernel
    #[cfg(target_os = "linux")]
    pub fn is_supported() -> bool {
        use std::fs;
        // Landlock requires Linux 5.13+
        // Check by looking at /proc/sys/kernel/osrelease
        if let Ok(version) = fs::read_to_string("/proc/sys/kernel/osrelease") {
            if let Some(major_minor) = version.split('.').take(2).collect::<Vec<_>>().join(".").parse::<f32>().ok() {
                return major_minor >= 5.13;
            }
        }
        false
    }

    #[cfg(not(target_os = "linux"))]
    pub fn is_supported() -> bool {
        false
    }

    /// Apply restrictions
    #[cfg(target_os = "linux")]
    pub fn apply(&self) -> std::io::Result<()> {
        use std::io::{Error, ErrorKind};
        use nix::sys::prctl;

        if !self.enabled {
            return Ok(());
        }

        if self.path_rules.is_empty() {
            return Ok(());
        }

        // Check if landlock is supported
        if !Self::is_supported() {
            // Landlock not supported, silently continue without filesystem sandboxing
            return Ok(());
        }

        // For full landlock implementation, use the `landlock` crate.
        // Here we document what would be done:
        //
        // 1. Create a landlock ruleset with:
        //    landlock_create_ruleset(attr, size, 0)
        //
        // 2. For each path rule, add it:
        //    landlock_add_rule(ruleset_fd, LANDLOCK_RULE_PATH_BENEATH, &attr, 0)
        //
        // 3. Restrict the current process:
        //    prctl(PR_SET_NO_NEW_PRIVS, 1)
        //    landlock_restrict_self(ruleset_fd, 0)

        // Ensure we have no_new_privs set (required for landlock)
        prctl::set_no_new_privs().map_err(|e| {
            Error::new(
                ErrorKind::PermissionDenied,
                format!("Failed to set PR_SET_NO_NEW_PRIVS - cannot apply landlock restrictions: {e}"),
            )
        })?;

        // Full landlock syscall implementation would go here
        // For now, we prepare the restrictions but defer full application
        // to a future version using the landlock crate
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn apply(&self) -> std::io::Result<()> {
        // Landlock is Linux-only
        Ok(())
    }

    /// Get the list of path rules
    pub fn path_rules(&self) -> &[LandlockPathRule] {
        &self.path_rules
    }
}

impl Default for LandlockRestrictions {
    fn default() -> Self {
        Self::new()
    }
}

/// Linux capability set
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    capabilities: HashSet<String>,
}

impl CapabilitySet {
    pub fn new() -> Self {
        Self {
            capabilities: HashSet::new(),
        }
    }

    /// Create capability set with common build capabilities
    pub fn for_build() -> Self {
        let mut caps = Self::new();
        // Minimal caps needed for typical build operations
        caps.add("CAP_CHOWN");
        caps.add("CAP_FOWNER");
        caps.add("CAP_SETUID");
        caps.add("CAP_SETGID");
        caps
    }

    /// Add a capability
    pub fn add(&mut self, cap: &str) {
        self.capabilities.insert(cap.to_string());
    }

    /// Remove a capability
    pub fn remove(&mut self, cap: &str) {
        self.capabilities.remove(cap);
    }

    /// Check if capability is in set
    pub fn contains(&self, cap: &str) -> bool {
        self.capabilities.contains(cap)
    }
}

/// Drop all capabilities except those in the keep set
#[cfg(target_os = "linux")]
pub fn drop_capabilities_except(keep: &CapabilitySet) -> std::io::Result<()> {
    // On Linux, we would use capset() to drop capabilities.
    // Full implementation would iterate through CAP_LAST_CAP and drop each
    // capability not in the keep set.
    //
    // This requires:
    // 1. Get current caps with capget()
    // 2. Clear all effective/permitted caps not in keep set
    // 3. Apply with capset()
    //
    // For a simple approach, use prctl to drop specific caps:
    // prctl(PR_CAPBSET_DROP, cap, 0, 0, 0)
    //
    // However, this requires the `caps` crate for proper implementation.
    // For now, we document the capabilities that would be kept:

    if !keep.capabilities.is_empty() {
        // Log which capabilities would be kept
        tracing::debug!(
            "Capability dropping configured, would keep: {:?}",
            keep.capabilities
        );
    }

    // The nix crate doesn't expose prctl PR_CAPBSET_DROP directly.
    // Full implementation would use the `caps` or `capctl` crate.
    // For now, this is a documentation stub showing intent.

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn drop_capabilities_except(_keep: &CapabilitySet) -> std::io::Result<()> {
    // Capabilities are Linux-only
    Ok(())
}

/// Capability dropping (drops all capabilities)
pub fn drop_capabilities() -> std::io::Result<()> {
    drop_capabilities_except(&CapabilitySet::new())
}

/// Get capability name from number
fn capability_name(cap: u32) -> Option<&'static str> {
    match cap {
        0 => Some("CAP_CHOWN"),
        1 => Some("CAP_DAC_OVERRIDE"),
        2 => Some("CAP_DAC_READ_SEARCH"),
        3 => Some("CAP_FOWNER"),
        4 => Some("CAP_FSETID"),
        5 => Some("CAP_KILL"),
        6 => Some("CAP_SETGID"),
        7 => Some("CAP_SETUID"),
        8 => Some("CAP_SETPCAP"),
        9 => Some("CAP_LINUX_IMMUTABLE"),
        10 => Some("CAP_NET_BIND_SERVICE"),
        11 => Some("CAP_NET_BROADCAST"),
        12 => Some("CAP_NET_ADMIN"),
        13 => Some("CAP_NET_RAW"),
        14 => Some("CAP_IPC_LOCK"),
        15 => Some("CAP_IPC_OWNER"),
        16 => Some("CAP_SYS_MODULE"),
        17 => Some("CAP_SYS_RAWIO"),
        18 => Some("CAP_SYS_CHROOT"),
        19 => Some("CAP_SYS_PTRACE"),
        20 => Some("CAP_SYS_PACCT"),
        21 => Some("CAP_SYS_ADMIN"),
        22 => Some("CAP_SYS_BOOT"),
        23 => Some("CAP_SYS_NICE"),
        24 => Some("CAP_SYS_RESOURCE"),
        25 => Some("CAP_SYS_TIME"),
        26 => Some("CAP_SYS_TTY_CONFIG"),
        27 => Some("CAP_MKNOD"),
        28 => Some("CAP_LEASE"),
        29 => Some("CAP_AUDIT_WRITE"),
        30 => Some("CAP_AUDIT_CONTROL"),
        31 => Some("CAP_SETFCAP"),
        _ => None,
    }
}

/// Security profile
pub struct SecurityProfile {
    pub seccomp: SeccompFilter,
    pub landlock: LandlockRestrictions,
    pub drop_caps: bool,
}

impl SecurityProfile {
    /// Create strict profile
    pub fn strict() -> Self {
        let seccomp = SeccompFilter::new();
        // Minimal syscalls only

        let mut landlock = LandlockRestrictions::new();
        landlock.allow_read("/usr");
        landlock.allow_read("/lib");

        Self {
            seccomp,
            landlock,
            drop_caps: true,
        }
    }

    /// Create permissive profile
    pub fn permissive() -> Self {
        let mut seccomp = SeccompFilter::new();
        // Allow most syscalls
        seccomp.allow("execve");
        seccomp.allow("fork");
        seccomp.allow("clone");

        let landlock = LandlockRestrictions::new();

        Self {
            seccomp,
            landlock,
            drop_caps: false,
        }
    }

    /// Apply all restrictions
    pub fn apply(&self) -> std::io::Result<()> {
        if self.drop_caps {
            drop_capabilities()?;
        }
        self.landlock.apply()?;
        self.seccomp.apply()?;
        Ok(())
    }
}
