//! System resource monitoring
//!
//! Provides real-time monitoring of CPU, memory, and I/O usage
//! for build processes using /proc filesystem on Linux.

use std::fs;
use std::io;
use std::path::Path;
use std::time::Instant;

/// Resource usage snapshot
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceSnapshot {
    pub timestamp_ms: u64,
    pub cpu_percent: f64,
    pub memory_mb: u64,
    pub memory_rss_mb: u64,
    pub io_read_mb: u64,
    pub io_write_mb: u64,
}

/// CPU statistics from /proc/stat
#[derive(Debug, Clone, Copy, Default)]
struct CpuStats {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
}

impl CpuStats {
    /// Total CPU time
    fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq + self.steal
    }

    /// Active (non-idle) CPU time
    fn active(&self) -> u64 {
        self.total() - self.idle - self.iowait
    }
}

/// Parse /proc/stat to get CPU statistics
#[cfg(target_os = "linux")]
fn read_cpu_stats() -> io::Result<CpuStats> {
    let content = fs::read_to_string("/proc/stat")?;
    for line in content.lines() {
        if line.starts_with("cpu ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 8 {
                return Ok(CpuStats {
                    user: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
                    nice: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
                    system: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
                    idle: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
                    iowait: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
                    irq: parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0),
                    softirq: parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0),
                    steal: parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0),
                });
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "CPU stats not found in /proc/stat"))
}

#[cfg(not(target_os = "linux"))]
fn read_cpu_stats() -> io::Result<CpuStats> {
    Ok(CpuStats::default())
}

/// Memory info from /proc/meminfo
#[derive(Debug, Clone, Copy, Default)]
struct MemInfo {
    total_kb: u64,
    free_kb: u64,
    available_kb: u64,
    buffers_kb: u64,
    cached_kb: u64,
}

impl MemInfo {
    /// Used memory in KB
    fn used_kb(&self) -> u64 {
        if self.available_kb > 0 {
            self.total_kb.saturating_sub(self.available_kb)
        } else {
            self.total_kb.saturating_sub(self.free_kb + self.buffers_kb + self.cached_kb)
        }
    }

    /// Used memory in MB
    fn used_mb(&self) -> u64 {
        self.used_kb() / 1024
    }
}

/// Parse /proc/meminfo to get memory statistics
#[cfg(target_os = "linux")]
fn read_mem_info() -> io::Result<MemInfo> {
    let content = fs::read_to_string("/proc/meminfo")?;
    let mut info = MemInfo::default();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let value: u64 = parts[1].parse().unwrap_or(0);
            match parts[0] {
                "MemTotal:" => info.total_kb = value,
                "MemFree:" => info.free_kb = value,
                "MemAvailable:" => info.available_kb = value,
                "Buffers:" => info.buffers_kb = value,
                "Cached:" => info.cached_kb = value,
                _ => {}
            }
        }
    }

    Ok(info)
}

#[cfg(not(target_os = "linux"))]
fn read_mem_info() -> io::Result<MemInfo> {
    Ok(MemInfo::default())
}

/// Process memory from /proc/self/statm
#[derive(Debug, Clone, Copy, Default)]
struct ProcessMem {
    vsize_pages: u64,
    rss_pages: u64,
}

impl ProcessMem {
    /// Virtual memory in MB (assuming 4KB pages)
    fn vsize_mb(&self) -> u64 {
        (self.vsize_pages * 4) / 1024
    }

    /// RSS in MB (assuming 4KB pages)
    fn rss_mb(&self) -> u64 {
        (self.rss_pages * 4) / 1024
    }
}

/// Read process memory usage
#[cfg(target_os = "linux")]
fn read_process_mem(pid: Option<u32>) -> io::Result<ProcessMem> {
    let path = match pid {
        Some(p) => format!("/proc/{p}/statm"),
        None => "/proc/self/statm".to_string(),
    };

    let content = fs::read_to_string(path)?;
    let parts: Vec<&str> = content.split_whitespace().collect();

    Ok(ProcessMem {
        vsize_pages: parts.first().and_then(|s| s.parse().ok()).unwrap_or(0),
        rss_pages: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

#[cfg(not(target_os = "linux"))]
fn read_process_mem(_pid: Option<u32>) -> io::Result<ProcessMem> {
    Ok(ProcessMem::default())
}

/// I/O statistics from /proc/self/io
#[derive(Debug, Clone, Copy, Default)]
struct IoStats {
    read_bytes: u64,
    write_bytes: u64,
}

/// Read I/O statistics
#[cfg(target_os = "linux")]
fn read_io_stats(pid: Option<u32>) -> io::Result<IoStats> {
    let path = match pid {
        Some(p) => format!("/proc/{p}/io"),
        None => "/proc/self/io".to_string(),
    };

    // /proc/self/io might not be accessible depending on permissions
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(IoStats::default()),
    };

    let mut stats = IoStats::default();
    for line in content.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() == 2 {
            let value: u64 = parts[1].trim().parse().unwrap_or(0);
            match parts[0] {
                "read_bytes" => stats.read_bytes = value,
                "write_bytes" => stats.write_bytes = value,
                _ => {}
            }
        }
    }

    Ok(stats)
}

#[cfg(not(target_os = "linux"))]
fn read_io_stats(_pid: Option<u32>) -> io::Result<IoStats> {
    Ok(IoStats::default())
}

/// Resource monitor
pub struct ResourceMonitor {
    start_time: Instant,
    snapshots: Vec<ResourceSnapshot>,
    last_cpu_stats: Option<CpuStats>,
    pid: Option<u32>,
    initial_io: IoStats,
}

impl ResourceMonitor {
    /// Create a new resource monitor
    pub fn new() -> Self {
        let initial_io = read_io_stats(None).unwrap_or_default();
        Self {
            start_time: Instant::now(),
            snapshots: Vec::new(),
            last_cpu_stats: None,
            pid: None,
            initial_io,
        }
    }

    /// Create a resource monitor for a specific PID
    pub fn for_pid(pid: u32) -> Self {
        let initial_io = read_io_stats(Some(pid)).unwrap_or_default();
        Self {
            start_time: Instant::now(),
            snapshots: Vec::new(),
            last_cpu_stats: None,
            pid: Some(pid),
            initial_io,
        }
    }

    /// Take a resource sample
    pub fn sample(&mut self) {
        let timestamp_ms = self.start_time.elapsed().as_millis() as u64;

        // CPU usage calculation
        let cpu_percent = if let Ok(current_cpu) = read_cpu_stats() {
            let percent = if let Some(ref last) = self.last_cpu_stats {
                let total_diff = current_cpu.total().saturating_sub(last.total());
                let active_diff = current_cpu.active().saturating_sub(last.active());
                if total_diff > 0 {
                    (active_diff as f64 / total_diff as f64) * 100.0
                } else {
                    0.0
                }
            } else {
                0.0
            };
            self.last_cpu_stats = Some(current_cpu);
            percent
        } else {
            0.0
        };

        // Memory usage
        let mem_info = read_mem_info().unwrap_or_default();
        let process_mem = read_process_mem(self.pid).unwrap_or_default();

        // I/O usage
        let io_stats = read_io_stats(self.pid).unwrap_or_default();
        let io_read_bytes = io_stats.read_bytes.saturating_sub(self.initial_io.read_bytes);
        let io_write_bytes = io_stats.write_bytes.saturating_sub(self.initial_io.write_bytes);

        let snapshot = ResourceSnapshot {
            timestamp_ms,
            cpu_percent,
            memory_mb: mem_info.used_mb(),
            memory_rss_mb: process_mem.rss_mb(),
            io_read_mb: io_read_bytes / (1024 * 1024),
            io_write_mb: io_write_bytes / (1024 * 1024),
        };

        self.snapshots.push(snapshot);
    }

    /// Get peak memory usage in MB
    pub fn peak_memory(&self) -> u64 {
        self.snapshots.iter().map(|s| s.memory_mb).max().unwrap_or(0)
    }

    /// Get peak RSS memory usage in MB
    pub fn peak_rss(&self) -> u64 {
        self.snapshots.iter().map(|s| s.memory_rss_mb).max().unwrap_or(0)
    }

    /// Get average CPU usage
    pub fn avg_cpu(&self) -> f64 {
        if self.snapshots.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.snapshots.iter().map(|s| s.cpu_percent).sum();
        sum / self.snapshots.len() as f64
    }

    /// Get peak CPU usage
    pub fn peak_cpu(&self) -> f64 {
        self.snapshots
            .iter()
            .map(|s| s.cpu_percent)
            .fold(0.0, f64::max)
    }

    /// Get total I/O read in MB
    pub fn total_io_read_mb(&self) -> u64 {
        self.snapshots.last().map(|s| s.io_read_mb).unwrap_or(0)
    }

    /// Get total I/O write in MB
    pub fn total_io_write_mb(&self) -> u64 {
        self.snapshots.last().map(|s| s.io_write_mb).unwrap_or(0)
    }

    /// Get all snapshots
    pub fn snapshots(&self) -> &[ResourceSnapshot] {
        &self.snapshots
    }

    /// Get snapshot count
    pub fn sample_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Get elapsed time in seconds
    pub fn elapsed_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Reset the monitor
    pub fn reset(&mut self) {
        self.start_time = Instant::now();
        self.snapshots.clear();
        self.last_cpu_stats = None;
        self.initial_io = read_io_stats(self.pid).unwrap_or_default();
    }

    /// Generate a summary report
    pub fn summary(&self) -> ResourceSummary {
        ResourceSummary {
            elapsed_secs: self.elapsed_secs(),
            sample_count: self.sample_count(),
            avg_cpu_percent: self.avg_cpu(),
            peak_cpu_percent: self.peak_cpu(),
            peak_memory_mb: self.peak_memory(),
            peak_rss_mb: self.peak_rss(),
            total_read_mb: self.total_io_read_mb(),
            total_write_mb: self.total_io_write_mb(),
        }
    }
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of resource usage
#[derive(Debug, Clone)]
pub struct ResourceSummary {
    pub elapsed_secs: f64,
    pub sample_count: usize,
    pub avg_cpu_percent: f64,
    pub peak_cpu_percent: f64,
    pub peak_memory_mb: u64,
    pub peak_rss_mb: u64,
    pub total_read_mb: u64,
    pub total_write_mb: u64,
}

impl std::fmt::Display for ResourceSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Duration: {:.1}s | CPU: avg {:.1}%, peak {:.1}% | Memory: peak {}MB (RSS {}MB) | I/O: read {}MB, write {}MB",
            self.elapsed_secs,
            self.avg_cpu_percent,
            self.peak_cpu_percent,
            self.peak_memory_mb,
            self.peak_rss_mb,
            self.total_read_mb,
            self.total_write_mb
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_resource_monitor_basic() {
        let mut monitor = ResourceMonitor::new();

        // Take a few samples
        for _ in 0..3 {
            monitor.sample();
            thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(monitor.sample_count(), 3);
        assert!(monitor.elapsed_secs() > 0.0);
    }

    #[test]
    fn test_resource_monitor_summary() {
        let mut monitor = ResourceMonitor::new();
        monitor.sample();

        let summary = monitor.summary();
        assert_eq!(summary.sample_count, 1);
        // Other values depend on system state
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_read_mem_info() {
        let mem = read_mem_info().expect("Should read meminfo on Linux");
        assert!(mem.total_kb > 0, "System should have some memory");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_read_process_mem() {
        let mem = read_process_mem(None).expect("Should read self statm");
        // Process should have some memory
        assert!(mem.vsize_pages > 0 || mem.rss_pages > 0);
    }
}
