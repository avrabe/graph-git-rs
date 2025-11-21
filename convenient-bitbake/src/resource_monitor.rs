//! System resource monitoring

use std::time::Instant;

/// Resource usage snapshot
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceSnapshot {
    pub timestamp_ms: u64,
    pub cpu_percent: f64,
    pub memory_mb: u64,
    pub io_read_mb: u64,
    pub io_write_mb: u64,
}

/// Resource monitor
pub struct ResourceMonitor {
    start_time: Instant,
    snapshots: Vec<ResourceSnapshot>,
}

impl ResourceMonitor {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            snapshots: Vec::new(),
        }
    }

    pub fn sample(&mut self) {
        // TODO: Implement actual resource sampling
        let snapshot = ResourceSnapshot {
            timestamp_ms: self.start_time.elapsed().as_millis() as u64,
            cpu_percent: 0.0,
            memory_mb: 0,
            io_read_mb: 0,
            io_write_mb: 0,
        };
        self.snapshots.push(snapshot);
    }

    pub fn peak_memory(&self) -> u64 {
        self.snapshots.iter().map(|s| s.memory_mb).max().unwrap_or(0)
    }

    pub fn avg_cpu(&self) -> f64 {
        if self.snapshots.is_empty() { return 0.0; }
        let sum: f64 = self.snapshots.iter().map(|s| s.cpu_percent).sum();
        sum / self.snapshots.len() as f64
    }
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::new()
    }
}
