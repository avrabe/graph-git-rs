//! Security hardening for sandbox

/// Seccomp filter builder
pub struct SeccompFilter {
    allowed_syscalls: Vec<String>,
}

impl SeccompFilter {
    pub fn new() -> Self {
        Self {
            allowed_syscalls: vec![
                "read".to_string(),
                "write".to_string(),
                "open".to_string(),
                "close".to_string(),
                "stat".to_string(),
                "fstat".to_string(),
                "lstat".to_string(),
                "poll".to_string(),
                "brk".to_string(),
                "mmap".to_string(),
                "munmap".to_string(),
                "rt_sigaction".to_string(),
                "rt_sigprocmask".to_string(),
                "exit".to_string(),
                "exit_group".to_string(),
            ],
        }
    }

    /// Allow a syscall
    pub fn allow(&mut self, syscall: impl Into<String>) {
        self.allowed_syscalls.push(syscall.into());
    }

    /// Generate BPF program
    pub fn to_bpf(&self) -> Vec<u8> {
        // TODO: Generate actual BPF seccomp filter
        // This would use the seccomp crate
        vec![]
    }

    /// Apply filter to current process
    pub fn apply(&self) -> std::io::Result<()> {
        // TODO: Apply seccomp filter
        // This requires Linux-specific syscalls
        #[cfg(target_os = "linux")]
        {
            // Would use seccomp::SeccompFilter here
        }
        Ok(())
    }
}

impl Default for SeccompFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Landlock filesystem restrictions
pub struct LandlockRestrictions {
    allowed_paths: Vec<String>,
}

impl LandlockRestrictions {
    pub fn new() -> Self {
        Self {
            allowed_paths: Vec::new(),
        }
    }

    /// Allow read access to a path
    pub fn allow_read(&mut self, path: impl Into<String>) {
        self.allowed_paths.push(path.into());
    }

    /// Apply restrictions
    pub fn apply(&self) -> std::io::Result<()> {
        // TODO: Apply landlock restrictions
        // This requires Linux 5.13+ and landlock crate
        Ok(())
    }
}

impl Default for LandlockRestrictions {
    fn default() -> Self {
        Self::new()
    }
}

/// Capability dropping
pub fn drop_capabilities() -> std::io::Result<()> {
    // TODO: Drop all capabilities except needed ones
    // This would use the caps crate
    Ok(())
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
        let mut seccomp = SeccompFilter::new();
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
