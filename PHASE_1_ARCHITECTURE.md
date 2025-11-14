# Phase 1 Architecture: Critical Production Fixes
**Version:** 1.0
**Date:** 2025-11-14
**Status:** Proposal for Review

---

## Executive Summary

This document proposes the architecture for fixing 4 **critical production blockers** identified in the Bitzel code review:

1. **Garbage Collection** - Prevents unbounded disk usage
2. **Network Isolation** - Ensures hermetic execution
3. **Resource Limits** - Prevents resource exhaustion
4. **Cache Corruption Fix** - Ensures cache integrity

**Timeline:** 2-3 weeks
**Risk:** Low (well-understood patterns)
**Impact:** Makes Bitzel production-ready for local development

---

## Table of Contents

1. [Problem Analysis](#1-problem-analysis)
2. [Architecture Principles](#2-architecture-principles)
3. [Fix #1: Garbage Collection](#3-fix-1-garbage-collection)
4. [Fix #2: Network Isolation](#4-fix-2-network-isolation)
5. [Fix #3: Resource Limits](#5-fix-3-resource-limits)
6. [Fix #4: Cache Corruption](#6-fix-4-cache-corruption)
7. [Integration Strategy](#7-integration-strategy)
8. [Testing Strategy](#8-testing-strategy)
9. [Migration Plan](#9-migration-plan)
10. [Success Metrics](#10-success-metrics)

---

## 1. Problem Analysis

### Current Issues

| Issue | Severity | Impact | Affected Users |
|-------|----------|--------|----------------|
| No GC → disk fills up | **CRITICAL** | Production failure | 100% |
| No network isolation | **CRITICAL** | Non-hermetic builds | 100% |
| No resource limits | **CRITICAL** | System crashes | 100% |
| Cache corruption | **HIGH** | Data loss | 10-20% |

### Root Causes

1. **GC**: Designed for correctness first, deferred optimization
2. **Network**: Minimal namespace set for compatibility
3. **Resources**: No cgroup integration initially
4. **Corruption**: Race conditions + missing durability

---

## 2. Architecture Principles

### Design Goals

1. **Backward Compatibility** - Existing users unaffected
2. **Graceful Degradation** - Features degrade if unavailable (e.g., no cgroups)
3. **Configurability** - All limits configurable via API/config
4. **Observability** - Metrics for monitoring
5. **Testability** - Comprehensive test coverage

### Technology Choices

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| GC | Mark-and-sweep + LRU | Industry standard, proven |
| Network | Linux network namespaces | Zero overhead, kernel-level |
| Resources | cgroup v2 | Modern, unified interface |
| Durability | fsync + file locks | POSIX guarantees |

---

## 3. Fix #1: Garbage Collection

### Problem Statement

```rust
// Current implementation
pub fn gc(&mut self, _keep: &[ContentHash]) -> ExecutionResult<usize> {
    // TODO: Implement garbage collection
    Ok(0)  // ← Cache grows forever!
}
```

**Consequences:**
- Cache grows to fill disk (100% in days/weeks)
- No automatic cleanup
- Manual intervention required

### Proposed Architecture

#### Three-Tier GC Strategy

```
┌─────────────────────────────────────────────────┐
│ GC Strategy                                     │
├─────────────────────────────────────────────────┤
│                                                 │
│  Tier 1: Mark-and-Sweep (Correctness)          │
│  ├─ Traverse action cache                      │
│  ├─ Mark referenced objects                    │
│  └─ Sweep unreferenced objects                 │
│                                                 │
│  Tier 2: LRU Eviction (Performance)            │
│  ├─ Track access times                         │
│  ├─ Evict least recently used                  │
│  └─ Respect size limits                        │
│                                                 │
│  Tier 3: Size-Based Cleanup (Capacity)         │
│  ├─ Monitor total cache size                   │
│  ├─ Trigger GC at threshold (e.g., 80%)        │
│  └─ Target size after GC (e.g., 50%)           │
│                                                 │
└─────────────────────────────────────────────────┘
```

#### Data Structures

```rust
/// Garbage collector for content-addressable store
pub struct CacheGarbageCollector {
    /// CAS reference
    cas: Arc<Mutex<ContentAddressableStore>>,

    /// Action cache reference
    action_cache: Arc<Mutex<ActionCache>>,

    /// LRU tracker
    access_tracker: LruAccessTracker,

    /// Configuration
    config: GcConfig,

    /// Metrics
    metrics: GcMetrics,
}

pub struct GcConfig {
    /// Maximum cache size in bytes (0 = unlimited)
    pub max_cache_size: u64,

    /// Trigger GC when cache reaches this percentage
    pub high_water_mark: f32,  // e.g., 0.8 = 80%

    /// Target size after GC (as percentage of max)
    pub low_water_mark: f32,   // e.g., 0.5 = 50%

    /// Minimum age before eviction (seconds)
    pub min_age_seconds: u64,  // e.g., 3600 = 1 hour

    /// Enable automatic GC (vs manual only)
    pub auto_gc: bool,

    /// GC interval (seconds, if auto_gc enabled)
    pub gc_interval_seconds: u64,  // e.g., 3600 = 1 hour
}

/// Tracks object access for LRU eviction
struct LruAccessTracker {
    /// Map: hash → last access time
    access_times: HashMap<ContentHash, SystemTime>,

    /// Persistent storage (survives restarts)
    db_path: PathBuf,
}

pub struct GcMetrics {
    pub last_gc_time: Option<SystemTime>,
    pub last_gc_duration_ms: u64,
    pub objects_collected: u64,
    pub bytes_freed: u64,
    pub total_gc_runs: u64,
}
```

#### Algorithm: Mark-and-Sweep

```rust
impl CacheGarbageCollector {
    /// Run garbage collection
    pub fn gc(&mut self) -> Result<GcStats, GcError> {
        info!("Starting garbage collection");
        let start = Instant::now();

        // Phase 1: Mark reachable objects
        let reachable = self.mark_phase()?;
        info!("Mark phase complete: {} objects reachable", reachable.len());

        // Phase 2: Sweep unreachable objects
        let swept = self.sweep_phase(&reachable)?;
        info!("Sweep phase complete: {} objects removed", swept.count);

        // Phase 3: LRU eviction if still over limit
        let evicted = if self.over_size_limit()? {
            self.lru_evict_phase()?
        } else {
            GcStats::default()
        };

        let duration = start.elapsed();
        self.update_metrics(duration, &swept, &evicted);

        Ok(GcStats {
            objects_removed: swept.count + evicted.count,
            bytes_freed: swept.bytes + evicted.bytes,
            duration_ms: duration.as_millis() as u64,
        })
    }

    /// Phase 1: Mark all reachable objects
    fn mark_phase(&self) -> Result<HashSet<ContentHash>, GcError> {
        let mut reachable = HashSet::new();

        // Start from action cache roots
        let action_cache = self.action_cache.lock().unwrap();

        for (_signature, output) in action_cache.iter() {
            // Mark output files
            for (_path, hash) in &output.output_files {
                reachable.insert(hash.clone());
            }
        }

        // TODO: If we add dependency graphs, traverse those too

        Ok(reachable)
    }

    /// Phase 2: Sweep unreachable objects
    fn sweep_phase(&mut self, reachable: &HashSet<ContentHash>) -> Result<GcStats, GcError> {
        let mut stats = GcStats::default();
        let cas = self.cas.lock().unwrap();

        for (hash, path) in cas.index.iter() {
            if !reachable.contains(hash) {
                // Check minimum age
                if let Ok(metadata) = fs::metadata(path) {
                    if let Ok(modified) = metadata.modified() {
                        let age = SystemTime::now()
                            .duration_since(modified)
                            .unwrap_or_default();

                        if age.as_secs() < self.config.min_age_seconds {
                            continue;  // Too young, skip
                        }
                    }

                    // Safe to delete
                    let size = metadata.len();
                    if fs::remove_file(path).is_ok() {
                        stats.count += 1;
                        stats.bytes += size;
                        debug!("GC: Removed unreachable object: {}", hash);
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Phase 3: LRU eviction to reach target size
    fn lru_evict_phase(&mut self) -> Result<GcStats, GcError> {
        let mut stats = GcStats::default();
        let current_size = self.current_cache_size()?;
        let target_size = (self.config.max_cache_size as f32
                          * self.config.low_water_mark) as u64;

        if current_size <= target_size {
            return Ok(stats);  // Already under target
        }

        let to_free = current_size - target_size;

        // Sort objects by access time (oldest first)
        let mut objects: Vec<_> = self.access_tracker.access_times
            .iter()
            .collect();
        objects.sort_by_key(|(_, time)| *time);

        let mut freed = 0u64;
        for (hash, _) in objects {
            if freed >= to_free {
                break;
            }

            if let Some(path) = self.cas.lock().unwrap().index.get(hash) {
                if let Ok(metadata) = fs::metadata(path) {
                    let size = metadata.len();
                    if fs::remove_file(path).is_ok() {
                        stats.count += 1;
                        stats.bytes += size;
                        freed += size;
                        debug!("GC: Evicted LRU object: {}", hash);
                    }
                }
            }
        }

        Ok(stats)
    }
}
```

#### Access Tracking

```rust
impl LruAccessTracker {
    /// Record access to an object
    pub fn record_access(&mut self, hash: &ContentHash) {
        self.access_times.insert(hash.clone(), SystemTime::now());
        // Persist to disk asynchronously
        self.persist_async();
    }

    /// Persist access times to disk
    fn persist_async(&self) {
        // TODO: Use background thread to avoid blocking
        let db = sled::open(&self.db_path).unwrap();
        for (hash, time) in &self.access_times {
            let key = hash.to_hex();
            let value = time.duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_le_bytes();
            let _ = db.insert(key.as_bytes(), &value);
        }
    }

    /// Load access times from disk
    pub fn load_from_disk(&mut self) -> Result<(), Error> {
        let db = sled::open(&self.db_path)?;
        for item in db.iter() {
            let (key, value) = item?;
            let hash = ContentHash::from_hex(&String::from_utf8_lossy(&key));
            let timestamp = u64::from_le_bytes(value.as_ref().try_into().unwrap());
            let time = UNIX_EPOCH + Duration::from_secs(timestamp);
            self.access_times.insert(hash, time);
        }
        Ok(())
    }
}
```

#### Integration Points

```rust
// In ContentAddressableStore::get()
impl ContentAddressableStore {
    pub fn get(&self, hash: &ContentHash) -> ExecutionResult<Vec<u8>> {
        // Record access for LRU tracking
        self.gc.access_tracker.record_access(hash);

        let path = self.hash_to_path(hash);
        fs::read(&path).map_err(|e| {
            ExecutionError::CacheError(format!("Failed to read {}: {}", hash, e))
        })
    }
}

// Automatic GC trigger
impl TaskExecutor {
    pub fn execute_task(&mut self, spec: TaskSpec) -> ExecutionResult<TaskOutput> {
        // ... existing code ...

        // Check if GC needed (after task completion)
        if self.gc.should_run()? {
            info!("Cache size threshold reached, triggering GC");
            tokio::spawn(async move {
                let _ = self.gc.gc().await;
            });
        }

        Ok(task_output)
    }
}
```

#### Configuration Example

```toml
# bitzel.toml
[cache.gc]
max_cache_size = 107374182400  # 100 GB
high_water_mark = 0.8          # Trigger at 80%
low_water_mark = 0.5           # Clean to 50%
min_age_seconds = 3600         # Keep at least 1 hour
auto_gc = true                 # Auto trigger
gc_interval_seconds = 3600     # Check every hour
```

### Rationale

**Why Mark-and-Sweep?**
- ✅ **Correctness**: Never deletes referenced objects
- ✅ **Simple**: Easy to reason about
- ✅ **Standard**: Used by Git, Docker, etc.

**Why LRU?**
- ✅ **Performance**: Keeps hot objects in cache
- ✅ **Predictable**: Clear eviction policy
- ✅ **Tunable**: Adjustable cache size

**Why Size-Based?**
- ✅ **Capacity Planning**: Prevents disk full
- ✅ **Automatic**: No manual intervention
- ✅ **Configurable**: Per-environment limits

### Dependencies

```toml
[dependencies]
sled = "0.34"  # Embedded database for access tracking
```

### Estimated Effort

- Design: ✅ Complete (this document)
- Implementation: 4-5 days
- Testing: 2-3 days
- **Total: 1 week**

---

## 4. Fix #2: Network Isolation

### Problem Statement

```rust
// Current: No network isolation
unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID)
// Missing: CLONE_NEWNET
```

**Consequences:**
- Tasks can make arbitrary HTTP requests
- Can download malicious code
- Can exfiltrate data
- **NOT HERMETIC**

### Proposed Architecture

#### Network Isolation Strategy

```
┌─────────────────────────────────────────────────┐
│ Network Isolation Levels                        │
├─────────────────────────────────────────────────┤
│                                                 │
│  Level 1: FULL ISOLATION (default)             │
│  ├─ No network namespace                       │
│  ├─ No external access                         │
│  └─ Perfect hermeticity                        │
│                                                 │
│  Level 2: LOOPBACK ONLY (for do_fetch)         │
│  ├─ Network namespace with loopback            │
│  ├─ No external network                        │
│  └─ Can resolve localhost                      │
│                                                 │
│  Level 3: CONTROLLED ACCESS (opt-in)           │
│  ├─ Network namespace with veth pair           │
│  ├─ NAT to host network                        │
│  └─ Firewall rules (allow-list)                │
│                                                 │
└─────────────────────────────────────────────────┘
```

#### Implementation

```rust
/// Network isolation policy for sandbox
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    /// No network access (default, most hermetic)
    Isolated,

    /// Loopback only (127.0.0.1 accessible)
    LoopbackOnly,

    /// Controlled external access with firewall
    Controlled {
        /// Allow list of domains/IPs
        allow_list: Vec<String>,
    },
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        NetworkPolicy::Isolated  // Safe by default
    }
}

/// Network namespace configuration
pub struct NetworkNamespace {
    policy: NetworkPolicy,
}

impl NetworkNamespace {
    /// Create network namespace with given policy
    pub fn new(policy: NetworkPolicy) -> Self {
        Self { policy }
    }

    /// Setup network in child process
    pub fn setup(&self) -> Result<(), ExecutionError> {
        match self.policy {
            NetworkPolicy::Isolated => {
                // Just create network namespace, don't configure anything
                // This gives us a completely isolated network stack
                debug!("Network: Full isolation (no network access)");
                Ok(())
            }

            NetworkPolicy::LoopbackOnly => {
                // Bring up loopback interface
                self.setup_loopback()?;
                debug!("Network: Loopback only (127.0.0.1 accessible)");
                Ok(())
            }

            NetworkPolicy::Controlled { ref allow_list } => {
                // Setup veth pair + NAT + firewall
                self.setup_controlled_access(allow_list)?;
                debug!("Network: Controlled access ({} allowed)", allow_list.len());
                Ok(())
            }
        }
    }

    /// Setup loopback interface
    fn setup_loopback(&self) -> Result<(), ExecutionError> {
        use std::process::Command;

        // Bring up loopback
        Command::new("ip")
            .args(&["link", "set", "lo", "up"])
            .status()
            .map_err(|e| ExecutionError::SandboxError(
                format!("Failed to bring up loopback: {}", e)
            ))?;

        Ok(())
    }

    /// Setup controlled network access
    fn setup_controlled_access(&self, allow_list: &[String]) -> Result<(), ExecutionError> {
        // This is complex and should be optional
        // For now, return error - implement in Phase 2 if needed
        Err(ExecutionError::SandboxError(
            "Controlled network access not yet implemented".to_string()
        ))
    }
}
```

#### Integration with Sandbox

```rust
// In execute_child_without_userns()
fn execute_child_without_userns(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
    network_policy: NetworkPolicy,  // ← New parameter
) -> Result<i32, ExecutionError> {
    use std::fs::File;

    debug!("Child: creating mount, PID, and network namespaces");

    // Create mount, PID, AND network namespaces
    unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWNET)
        .map_err(|e| ExecutionError::SandboxError(format!("unshare failed: {}", e)))?;

    debug!("Child: namespaces created, setting up network");

    // Setup network according to policy
    let netns = NetworkNamespace::new(network_policy);
    netns.setup()?;

    // ... rest of sandbox setup
}
```

#### Task-Level Configuration

```rust
/// Task specification with network policy
pub struct TaskSpec {
    pub name: String,
    pub recipe: String,
    pub script: String,
    pub workdir: PathBuf,
    pub env: HashMap<String, String>,
    pub outputs: Vec<PathBuf>,
    pub timeout: Option<Duration>,

    /// Network policy for this task
    pub network_policy: NetworkPolicy,  // ← New field
}

impl TaskSpec {
    /// Default task spec with full network isolation
    pub fn new(name: String, recipe: String, script: String) -> Self {
        Self {
            name,
            recipe,
            script,
            workdir: PathBuf::new(),
            env: HashMap::new(),
            outputs: Vec::new(),
            timeout: None,
            network_policy: NetworkPolicy::Isolated,  // ← Safe default
        }
    }

    /// Allow network for fetch tasks
    pub fn with_loopback_network(mut self) -> Self {
        self.network_policy = NetworkPolicy::LoopbackOnly;
        self
    }
}
```

#### BitBake Integration

```rust
// In task extraction
impl TaskExtractor {
    pub fn extract_task(&self, recipe: &Recipe, task_name: &str) -> TaskSpec {
        let mut spec = TaskSpec::new(
            task_name.to_string(),
            recipe.name.clone(),
            self.extract_script(recipe, task_name),
        );

        // Allow network for fetch tasks only
        if task_name == "do_fetch" || task_name == "do_unpack" {
            spec = spec.with_loopback_network();
        }

        spec
    }
}
```

### Rationale

**Why CLONE_NEWNET?**
- ✅ **Hermetic**: Tasks cannot access network
- ✅ **Zero overhead**: Kernel-level isolation
- ✅ **No escape**: Cannot bypass via syscalls
- ✅ **Auditable**: strace shows no network syscalls

**Why Loopback for do_fetch?**
- ✅ **Practical**: Some fetchers need localhost
- ✅ **Safe**: No external access
- ✅ **Optional**: Can disable if not needed

**Why NOT full network access?**
- ❌ **Non-hermetic**: Can download different files each time
- ❌ **Security risk**: Can exfiltrate data
- ❌ **Hard to debug**: Network flakiness

### Testing

```rust
#[test]
fn test_network_isolation() {
    let spec = TaskSpec::new(
        "test".to_string(),
        "test".to_string(),
        "curl https://google.com".to_string(),
    );

    let result = executor.execute_task(spec);

    // Should fail - no network access
    assert!(result.is_err() || result.unwrap().exit_code != 0);
}

#[test]
fn test_loopback_network() {
    let spec = TaskSpec::new(
        "test".to_string(),
        "test".to_string(),
        "ping -c 1 127.0.0.1".to_string(),
    ).with_loopback_network();

    let result = executor.execute_task(spec).unwrap();

    // Should succeed - loopback accessible
    assert_eq!(result.exit_code, 0);
}
```

### Dependencies

None - uses Linux kernel features only.

### Estimated Effort

- Design: ✅ Complete
- Implementation: 1-2 days
- Testing: 1 day
- **Total: 2-3 days**

---

## 5. Fix #3: Resource Limits

### Problem Statement

```rust
// Current: No resource limits
// Tasks can:
// - Consume unlimited CPU
// - Consume unlimited RAM
// - Consume unlimited disk
// - Run forever (no timeout)
```

**Consequences:**
- Fork bombs kill the system
- OOM kills other processes
- Disk fills up from temp files
- Infinite loops never terminate

### Proposed Architecture

#### cgroup v2 Integration

```
┌─────────────────────────────────────────────────┐
│ Resource Limits (cgroup v2)                     │
├─────────────────────────────────────────────────┤
│                                                 │
│  CPU Limits:                                    │
│  ├─ cpu.max = "50000 100000"  (50% of 1 core)  │
│  └─ cpu.weight = 100  (fair scheduling)        │
│                                                 │
│  Memory Limits:                                 │
│  ├─ memory.max = 4GB                            │
│  ├─ memory.swap.max = 0  (no swap)             │
│  └─ memory.oom.group = 1  (kill all on OOM)    │
│                                                 │
│  I/O Limits:                                    │
│  ├─ io.max = "8:0 rbps=10485760"  (10 MB/s)    │
│  └─ io.weight = 100                             │
│                                                 │
│  Process Limits:                                │
│  ├─ pids.max = 1024  (prevent fork bombs)      │
│  └─ Timeout via tokio::time::timeout()         │
│                                                 │
└─────────────────────────────────────────────────┘
```

#### Data Structures

```rust
/// Resource limits for task execution
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// CPU quota (microseconds per 100ms period)
    /// e.g., 50000 = 50% of one core
    pub cpu_quota_us: Option<u64>,

    /// Memory limit in bytes
    /// e.g., 4294967296 = 4 GB
    pub memory_limit_bytes: Option<u64>,

    /// Disable swap (recommended for builds)
    pub disable_swap: bool,

    /// I/O bandwidth limit (bytes per second)
    /// e.g., 10485760 = 10 MB/s
    pub io_bps_limit: Option<u64>,

    /// Maximum number of processes (prevent fork bombs)
    /// e.g., 1024
    pub max_pids: Option<u64>,

    /// Wall-clock timeout
    /// e.g., Duration::from_secs(3600) = 1 hour
    pub timeout: Option<Duration>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            // Conservative defaults
            cpu_quota_us: Some(100_000),      // 100% of 1 core
            memory_limit_bytes: Some(8 * 1024 * 1024 * 1024),  // 8 GB
            disable_swap: true,               // No swap for builds
            io_bps_limit: None,               // Unlimited I/O
            max_pids: Some(1024),             // Prevent fork bombs
            timeout: Some(Duration::from_secs(7200)),  // 2 hours
        }
    }
}

/// cgroup v2 controller
pub struct CgroupController {
    /// cgroup root path (e.g., /sys/fs/cgroup/bitzel)
    root: PathBuf,

    /// Unique cgroup name for this task
    name: String,
}

impl CgroupController {
    /// Create new cgroup for task
    pub fn new(task_name: &str) -> Result<Self, CgroupError> {
        let name = format!("bitzel-{}-{}", task_name, uuid::Uuid::new_v4());
        let root = PathBuf::from("/sys/fs/cgroup/bitzel").join(&name);

        // Create cgroup directory
        fs::create_dir_all(&root)?;

        Ok(Self { root, name })
    }

    /// Apply resource limits to current process
    pub fn apply_limits(&self, limits: &ResourceLimits) -> Result<(), CgroupError> {
        // CPU limits
        if let Some(quota) = limits.cpu_quota_us {
            self.write_controller("cpu.max", &format!("{} 100000", quota))?;
        }

        // Memory limits
        if let Some(mem) = limits.memory_limit_bytes {
            self.write_controller("memory.max", &mem.to_string())?;
        }

        if limits.disable_swap {
            self.write_controller("memory.swap.max", "0")?;
        }

        // Kill all processes on OOM (not just one)
        self.write_controller("memory.oom.group", "1")?;

        // I/O limits (if specified)
        if let Some(bps) = limits.io_bps_limit {
            // Find root device (e.g., "8:0" for /dev/sda)
            if let Ok(dev) = self.find_root_device() {
                self.write_controller("io.max", &format!("{} rbps={} wbps={}", dev, bps, bps))?;
            }
        }

        // PID limits
        if let Some(max_pids) = limits.max_pids {
            self.write_controller("pids.max", &max_pids.to_string())?;
        }

        // Add current process to cgroup
        self.add_process(std::process::id())?;

        Ok(())
    }

    /// Write to cgroup controller file
    fn write_controller(&self, controller: &str, value: &str) -> Result<(), CgroupError> {
        let path = self.root.join(controller);
        fs::write(&path, value)
            .map_err(|e| CgroupError::WriteFailed(controller.to_string(), e))?;
        debug!("cgroup: {} = {}", controller, value);
        Ok(())
    }

    /// Add process to cgroup
    fn add_process(&self, pid: u32) -> Result<(), CgroupError> {
        let procs_path = self.root.join("cgroup.procs");
        fs::write(&procs_path, pid.to_string())
            .map_err(|e| CgroupError::AddProcessFailed(e))?;
        debug!("cgroup: Added PID {} to {}", pid, self.name);
        Ok(())
    }

    /// Find root device for I/O limits
    fn find_root_device(&self) -> Result<String, CgroupError> {
        // Read /proc/self/mountinfo to find root device
        let mounts = fs::read_to_string("/proc/self/mountinfo")?;
        for line in mounts.lines() {
            if line.contains(" / ") {
                // Extract device number (e.g., "8:0")
                if let Some(dev) = line.split_whitespace().nth(2) {
                    return Ok(dev.to_string());
                }
            }
        }
        Err(CgroupError::RootDeviceNotFound)
    }

    /// Cleanup cgroup (call after task completes)
    pub fn cleanup(self) -> Result<(), CgroupError> {
        // Kill all processes in cgroup
        let _ = fs::write(self.root.join("cgroup.kill"), "1");

        // Remove cgroup directory
        fs::remove_dir(&self.root)
            .map_err(|e| CgroupError::CleanupFailed(e))?;

        debug!("cgroup: Cleaned up {}", self.name);
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CgroupError {
    #[error("Failed to write {0}: {1}")]
    WriteFailed(String, std::io::Error),

    #[error("Failed to add process to cgroup: {0}")]
    AddProcessFailed(std::io::Error),

    #[error("Root device not found")]
    RootDeviceNotFound,

    #[error("Failed to cleanup cgroup: {0}")]
    CleanupFailed(std::io::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

#### Integration with Sandbox

```rust
// In execute_child_without_userns()
fn execute_child_without_userns(
    script: &str,
    work_dir: &Path,
    env: &std::collections::HashMap<String, String>,
    network_policy: NetworkPolicy,
    resource_limits: ResourceLimits,  // ← New parameter
) -> Result<i32, ExecutionError> {
    // ... namespace setup ...

    // Apply resource limits via cgroup
    let cgroup = CgroupController::new(&format!("task-{}", std::process::id()))?;
    cgroup.apply_limits(&resource_limits)?;

    // ... execute command ...

    // Cleanup cgroup before exit
    cgroup.cleanup()?;

    Ok(exit_code)
}

// Wrapper with timeout
pub fn execute_with_timeout(
    script: &str,
    work_dir: &Path,
    env: &HashMap<String, String>,
    network_policy: NetworkPolicy,
    resource_limits: ResourceLimits,
) -> Result<(i32, String, String), ExecutionError> {
    let timeout = resource_limits.timeout.unwrap_or(Duration::from_secs(7200));

    tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || {
            execute_child_without_userns(script, work_dir, env, network_policy, resource_limits)
        })
    )
    .await
    .map_err(|_| ExecutionError::Timeout)?
    .map_err(|e| ExecutionError::TaskFailed(format!("Task panicked: {}", e)))?
}
```

#### Configuration

```toml
# bitzel.toml
[sandbox.resources]
cpu_quota_us = 100000          # 100% of 1 core
memory_limit_gb = 8            # 8 GB RAM
disable_swap = true            # No swap
max_pids = 1024                # Prevent fork bombs
timeout_seconds = 7200         # 2 hours

# Per-task overrides
[sandbox.resources.do_compile]
cpu_quota_us = 400000          # 4 cores
memory_limit_gb = 16           # 16 GB

[sandbox.resources.do_fetch]
timeout_seconds = 1800         # 30 minutes
```

### Rationale

**Why cgroup v2?**
- ✅ **Unified interface**: Single hierarchy
- ✅ **Modern**: Replaces cgroup v1
- ✅ **Standard**: Used by systemd, Docker, Kubernetes
- ✅ **Comprehensive**: CPU, memory, I/O, PIDs

**Why these defaults?**
- **1 core CPU**: Prevents CPU monopolization
- **8 GB RAM**: Generous for most builds
- **No swap**: Predictable performance
- **1024 PIDs**: Prevents fork bombs but allows parallel builds
- **2 hour timeout**: Long enough for big compiles

**Why disable swap?**
- ✅ **Predictable**: No performance degradation
- ✅ **Fast OOM**: Quick failure vs slow swap death
- ✅ **Clear signals**: Memory exhaustion is obvious

### Graceful Degradation

```rust
impl CgroupController {
    /// Check if cgroup v2 is available
    pub fn is_available() -> bool {
        Path::new("/sys/fs/cgroup/cgroup.controllers").exists()
    }

    /// Apply limits with fallback
    pub fn apply_limits_or_warn(&self, limits: &ResourceLimits) -> Result<(), CgroupError> {
        if !Self::is_available() {
            warn!("cgroup v2 not available, resource limits will not be enforced");
            warn!("Consider upgrading kernel or enabling cgroup v2");
            return Ok(());  // Continue without limits
        }

        self.apply_limits(limits)
    }
}
```

### Testing

```rust
#[test]
fn test_cpu_limit() {
    let limits = ResourceLimits {
        cpu_quota_us: Some(50_000),  // 50% of 1 core
        ..Default::default()
    };

    let spec = TaskSpec::new(
        "cpu_hog".to_string(),
        "test".to_string(),
        "while true; do :; done".to_string(),  // Infinite loop
    ).with_limits(limits);

    let start = Instant::now();
    let _ = executor.execute_task(spec);
    let duration = start.elapsed();

    // Should be limited to ~50% CPU
    assert!(duration >= Duration::from_secs(2));  // Would be instant with full CPU
}

#[test]
fn test_memory_limit() {
    let limits = ResourceLimits {
        memory_limit_bytes: Some(100 * 1024 * 1024),  // 100 MB
        ..Default::default()
    };

    let spec = TaskSpec::new(
        "mem_hog".to_string(),
        "test".to_string(),
        "dd if=/dev/zero of=/tmp/big bs=1M count=200".to_string(),  // 200 MB
    ).with_limits(limits);

    let result = executor.execute_task(spec);

    // Should fail due to OOM
    assert!(result.is_err() || result.unwrap().exit_code != 0);
}

#[test]
fn test_fork_bomb_prevention() {
    let limits = ResourceLimits {
        max_pids: Some(100),
        ..Default::default()
    };

    let spec = TaskSpec::new(
        "fork_bomb".to_string(),
        "test".to_string(),
        ":(){ :|:& };:".to_string(),  // Fork bomb
    ).with_limits(limits);

    let result = executor.execute_task(spec);

    // Should fail, not kill system
    assert!(result.is_err() || result.unwrap().exit_code != 0);
}
```

### Dependencies

```toml
[dependencies]
uuid = { version = "1.0", features = ["v4"] }  # Already exists
tokio = { version = "1", features = ["time"] }  # Already exists
```

### Estimated Effort

- Design: ✅ Complete
- Implementation: 2-3 days
- Testing: 1-2 days
- **Total: 3-5 days**

---

## 6. Fix #4: Cache Corruption

### Problem Statement

```rust
// Current: Race condition + no durability
let temp_path = path.with_extension("tmp");
fs::write(&temp_path, content)?;
fs::rename(&temp_path, &path)?;  // ← No fsync! Race condition!
```

**Consequences:**
- System crash can corrupt cache entries
- Parallel builds can overwrite each other
- Data loss possible

### Proposed Architecture

#### Two-Phase Commit with File Locking

```
┌─────────────────────────────────────────────────┐
│ Atomic Cache Write                              │
├─────────────────────────────────────────────────┤
│                                                 │
│  Phase 1: Acquire Lock                          │
│  ├─ Create .lock file                           │
│  ├─ flock(LOCK_EX) - exclusive lock             │
│  └─ Block until acquired                        │
│                                                 │
│  Phase 2: Check Existence                       │
│  ├─ Double-check file doesn't exist             │
│  └─ Return early if exists (race condition)     │
│                                                 │
│  Phase 3: Write + Sync                          │
│  ├─ Write to temp file                          │
│  ├─ fsync(temp_file)  ← Force disk write        │
│  ├─ rename(temp, final)                         │
│  └─ fsync(parent_dir) ← Ensure rename durable   │
│                                                 │
│  Phase 4: Release Lock                          │
│  ├─ flock(LOCK_UN) - release lock               │
│  └─ Delete .lock file                           │
│                                                 │
└─────────────────────────────────────────────────┘
```

#### Implementation

```rust
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;

impl ContentAddressableStore {
    /// Store content with atomic write and locking
    pub fn put(&mut self, content: &[u8]) -> ExecutionResult<ContentHash> {
        let hash = ContentHash::from_bytes(content);
        let path = self.hash_to_path(&hash);

        // Fast path: already exists
        if path.exists() {
            return Ok(hash);
        }

        // Acquire lock to prevent race conditions
        let lock = self.acquire_lock(&hash)?;

        // Double-check after acquiring lock (TOCTOU prevention)
        if path.exists() {
            drop(lock);  // Release lock
            return Ok(hash);
        }

        // Write atomically with durability guarantees
        self.write_atomic(&path, content)?;

        // Update index
        self.index.insert(hash.clone(), path);

        // Lock released when dropped
        Ok(hash)
    }

    /// Acquire exclusive lock for hash
    fn acquire_lock(&self, hash: &ContentHash) -> ExecutionResult<FileLock> {
        let lock_path = self.hash_to_path(hash).with_extension("lock");

        // Create parent directories
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Open lock file
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)?;

        // Acquire exclusive lock (blocks until available)
        use fs2::FileExt;
        lock_file.lock_exclusive()
            .map_err(|e| ExecutionError::CacheError(format!("Lock failed: {}", e)))?;

        debug!("Acquired lock for {}", hash);

        Ok(FileLock {
            file: lock_file,
            path: lock_path,
        })
    }

    /// Write file atomically with fsync
    fn write_atomic(&self, path: &Path, content: &[u8]) -> ExecutionResult<()> {
        use std::os::unix::io::AsRawFd;

        // Create parent directories
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write to temporary file
        let temp_path = path.with_extension("tmp");
        let mut temp_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o644)
            .open(&temp_path)?;

        use std::io::Write;
        temp_file.write_all(content)?;

        // **CRITICAL**: Force write to disk before rename
        temp_file.sync_all()
            .map_err(|e| ExecutionError::CacheError(format!("fsync failed: {}", e)))?;

        drop(temp_file);  // Close file

        // Rename to final location
        fs::rename(&temp_path, path)?;

        // **CRITICAL**: Sync parent directory to make rename durable
        let parent = path.parent().unwrap();
        let parent_fd = fs::File::open(parent)?;
        parent_fd.sync_all()
            .map_err(|e| ExecutionError::CacheError(format!("parent fsync failed: {}", e)))?;

        debug!("Atomically wrote {}", path.display());

        Ok(())
    }
}

/// File lock with automatic cleanup
struct FileLock {
    file: fs::File,
    path: PathBuf,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        use fs2::FileExt;

        // Release lock
        let _ = self.file.unlock();

        // Delete lock file
        let _ = fs::remove_file(&self.path);

        debug!("Released lock: {}", self.path.display());
    }
}
```

#### Verification

```rust
impl ContentAddressableStore {
    /// Verify integrity of cache entry
    pub fn verify(&self, hash: &ContentHash) -> Result<bool, ExecutionError> {
        let path = self.hash_to_path(hash);

        if !path.exists() {
            return Ok(false);
        }

        // Read file and recompute hash
        let content = fs::read(&path)?;
        let computed = ContentHash::from_bytes(&content);

        if computed != *hash {
            warn!("Cache corruption detected: {} != {}", hash, computed);
            // Delete corrupted file
            let _ = fs::remove_file(&path);
            return Ok(false);
        }

        Ok(true)
    }

    /// Verify entire cache
    pub fn verify_all(&self) -> Result<CacheVerifyReport, ExecutionError> {
        let mut report = CacheVerifyReport::default();

        for (hash, path) in &self.index {
            match self.verify(hash) {
                Ok(true) => report.valid += 1,
                Ok(false) => report.corrupted += 1,
                Err(e) => {
                    warn!("Failed to verify {}: {}", hash, e);
                    report.errors += 1;
                }
            }
        }

        Ok(report)
    }
}

#[derive(Default, Debug)]
pub struct CacheVerifyReport {
    pub valid: usize,
    pub corrupted: usize,
    pub errors: usize,
}
```

### Rationale

**Why file locking?**
- ✅ **Prevents races**: Only one writer at a time
- ✅ **Cross-process**: Works across multiple Bitzel instances
- ✅ **Automatic cleanup**: Lock released on crash

**Why fsync?**
- ✅ **Durability**: Survives power loss
- ✅ **Correctness**: No torn writes
- ✅ **Standard**: POSIX guarantees

**Why double-check?**
- ✅ **TOCTOU prevention**: Time-of-check vs time-of-use
- ✅ **Efficiency**: Avoid redundant work
- ✅ **Correctness**: Idempotent

### Performance Impact

**Overhead per write:**
- Lock acquire: ~0.1ms (uncontended)
- fsync: ~5-10ms (depends on disk)
- **Total: ~5-10ms** (acceptable for cache writes)

**Mitigation:**
- Writes are rare (only on cache miss)
- Reads unaffected (no locking)
- Can batch writes if needed

### Testing

```rust
#[test]
fn test_concurrent_writes() {
    use std::sync::Arc;
    use std::thread;

    let cas = Arc::new(Mutex::new(ContentAddressableStore::new(tmp.path()).unwrap()));
    let content = b"test content";

    // Spawn 10 threads all writing same content
    let mut handles = vec![];
    for _ in 0..10 {
        let cas_clone = cas.clone();
        let handle = thread::spawn(move || {
            let mut cas = cas_clone.lock().unwrap();
            cas.put(content).unwrap()
        });
        handles.push(handle);
    }

    // All should succeed, returning same hash
    let mut hashes = vec![];
    for handle in handles {
        hashes.push(handle.join().unwrap());
    }

    // All hashes should be identical
    assert!(hashes.windows(2).all(|w| w[0] == w[1]));

    // File should exist and be correct
    let cas = cas.lock().unwrap();
    let hash = &hashes[0];
    assert!(cas.contains(hash));
    assert_eq!(cas.get(hash).unwrap(), content);
}

#[test]
fn test_crash_recovery() {
    // Simulate crash by killing process mid-write
    // (hard to test, but can verify manually)

    // After crash, verify should detect corruption
    let cas = ContentAddressableStore::new(tmp.path()).unwrap();
    let report = cas.verify_all().unwrap();

    // Should have 0 corrupted files
    assert_eq!(report.corrupted, 0);
}
```

### Dependencies

```toml
[dependencies]
fs2 = "0.4"  # File locking
```

### Estimated Effort

- Design: ✅ Complete
- Implementation: 1 day
- Testing: 1 day
- **Total: 2 days**

---

## 7. Integration Strategy

### Phased Rollout

```
Week 1: Foundation
├─ Day 1-2: Network isolation
├─ Day 3-4: Cache corruption fix
└─ Day 5: Integration testing

Week 2: Resource Management
├─ Day 1-3: cgroup integration
├─ Day 4-5: Testing + debugging

Week 3: Garbage Collection
├─ Day 1-4: GC implementation
├─ Day 5: Performance testing

Week 4: Polish
├─ Day 1-2: Documentation
├─ Day 3-4: Integration tests
└─ Day 5: Release preparation
```

### Configuration

```toml
# bitzel.toml - Complete configuration

[cache]
# Cache location
dir = "./bitzel-cache"

[cache.gc]
# Garbage collection
enabled = true
max_cache_size_gb = 100
high_water_mark = 0.8
low_water_mark = 0.5
min_age_hours = 1
auto_gc = true
gc_interval_hours = 1

[sandbox]
# Sandbox backend (auto-detect if not specified)
backend = "native"  # or "bubblewrap", "basic"

[sandbox.network]
# Network policy
default_policy = "isolated"  # or "loopback", "controlled"

# Per-task overrides
do_fetch = "loopback"
do_unpack = "loopback"

[sandbox.resources]
# Default resource limits
cpu_cores = 1.0
memory_gb = 8
disable_swap = true
max_pids = 1024
timeout_hours = 2

# Per-task overrides
[sandbox.resources.do_compile]
cpu_cores = 4.0
memory_gb = 16
timeout_hours = 4

[sandbox.resources.do_test]
timeout_hours = 1
```

### API Changes

```rust
// New TaskExecutor API
impl TaskExecutor {
    /// Create executor with configuration
    pub fn with_config(config: BitzelConfig) -> ExecutionResult<Self> {
        let cas_dir = config.cache.dir.join("cas");
        let action_cache_dir = config.cache.dir.join("action-cache");
        let sandbox_dir = config.cache.dir.join("sandboxes");

        let mut executor = Self {
            cas: ContentAddressableStore::new(cas_dir)?,
            action_cache: ActionCache::new(action_cache_dir)?,
            sandbox_manager: SandboxManager::new(sandbox_dir)?,
            gc: GarbageCollector::new(config.cache.gc)?,
            stats: ExecutionStats::default(),
        };

        // Start background GC if enabled
        if config.cache.gc.auto_gc {
            executor.start_gc_daemon()?;
        }

        Ok(executor)
    }
}
```

---

## 8. Testing Strategy

### Unit Tests

```rust
// GC tests
#[test] fn test_mark_and_sweep()
#[test] fn test_lru_eviction()
#[test] fn test_size_based_gc()

// Network tests
#[test] fn test_network_isolation()
#[test] fn test_loopback_access()

// Resource tests
#[test] fn test_cpu_limit()
#[test] fn test_memory_limit()
#[test] fn test_fork_bomb_prevention()

// Cache tests
#[test] fn test_concurrent_writes()
#[test] fn test_fsync_durability()
```

### Integration Tests

```rust
#[test]
fn test_full_build_with_limits() {
    let config = BitzelConfig::default();
    let mut executor = TaskExecutor::with_config(config)?;

    // Build busybox with all protections
    let result = executor.execute_task(TaskSpec {
        name: "do_compile".to_string(),
        recipe: "busybox".to_string(),
        script: "./configure && make".to_string(),
        network_policy: NetworkPolicy::Isolated,
        resource_limits: ResourceLimits::default(),
        // ...
    })?;

    assert_eq!(result.exit_code, 0);
}
```

### Stress Tests

```rust
#[test]
fn test_gc_under_load() {
    // Fill cache to 80%
    // Trigger GC
    // Verify cache shrinks to 50%
    // Verify no data loss
}

#[test]
fn test_concurrent_builds() {
    // Run 100 parallel builds
    // Verify no cache corruption
    // Verify resource limits enforced
}
```

---

## 9. Migration Plan

### For Existing Users

```bash
# Upgrade process
1. Backup cache: tar -czf bitzel-cache-backup.tar.gz bitzel-cache/
2. Upgrade Bitzel: cargo install bitzel --version 0.2.0
3. Run GC manually: bitzel gc --verify
4. Test build: bitzel build busybox
5. Monitor disk usage: df -h
```

### Rollback Plan

```bash
# If issues occur
1. Stop builds
2. Restore cache: tar -xzf bitzel-cache-backup.tar.gz
3. Downgrade: cargo install bitzel --version 0.1.0
4. Report issue on GitHub
```

---

## 10. Success Metrics

### Performance Metrics

| Metric | Target | How to Measure |
|--------|--------|----------------|
| GC overhead | < 5% of build time | Time GC runs |
| Cache hit rate | > 90% | Action cache stats |
| Disk usage | < 100 GB | Monitor cache size |
| Memory usage | < 8 GB per task | cgroup stats |
| Network isolation | 100% blocked | tcpdump test |

### Reliability Metrics

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Cache corruption | 0 per 1000 builds | Verify command |
| OOM crashes | 0 | Monitor logs |
| Timeout failures | < 1% | Task stats |

---

## Conclusion

This architecture provides a **comprehensive solution** to the 4 critical production blockers:

1. **GC**: Mark-and-sweep + LRU prevents disk fill-up
2. **Network**: Namespace isolation ensures hermeticity
3. **Resources**: cgroup limits prevent resource exhaustion
4. **Corruption**: fsync + locking ensures cache integrity

**Timeline:** 2-3 weeks
**Risk:** Low (proven patterns)
**Impact:** Production-ready Bitzel

**Next Steps:**
1. Review this architecture
2. Approve for implementation
3. Begin Week 1 (network + cache fixes)

---

**Questions? Feedback? Ready to implement?**
