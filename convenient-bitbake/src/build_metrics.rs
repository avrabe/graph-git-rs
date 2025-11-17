//! Build metrics collection and analysis

use std::collections::HashMap;
use std::time::{Duration, Instant};
use serde::{Serialize, Deserialize};

/// Build metrics collector
#[derive(Debug, Clone, Default)]
pub struct BuildMetrics {
    /// Task execution times
    pub task_times: HashMap<String, Duration>,

    /// Cache statistics
    pub cache_hits: usize,
    pub cache_misses: usize,

    /// Parallel execution stats
    pub max_parallelism: usize,
    pub avg_parallelism: f64,

    /// Resource usage
    pub peak_memory_mb: u64,
    pub total_cpu_time_ms: u64,
    pub total_io_bytes: u64,

    /// Build phases
    pub parse_time: Duration,
    pub planning_time: Duration,
    pub execution_time: Duration,
    pub total_time: Duration,
}

impl BuildMetrics {
    /// Record task execution
    pub fn record_task(&mut self, name: String, duration: Duration) {
        self.task_times.insert(name, duration);
    }

    /// Record cache hit
    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    /// Record cache miss
    pub fn record_cache_miss(&mut self) {
        self.cache_misses += 1;
    }

    /// Get cache hit rate
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 { 0.0 } else { self.cache_hits as f64 / total as f64 * 100.0 }
    }

    /// Get total task time
    pub fn total_task_time(&self) -> Duration {
        self.task_times.values().sum()
    }

    /// Get slowest tasks
    pub fn slowest_tasks(&self, n: usize) -> Vec<(&String, &Duration)> {
        let mut tasks: Vec<_> = self.task_times.iter().collect();
        tasks.sort_by(|a, b| b.1.cmp(a.1));
        tasks.into_iter().take(n).collect()
    }
}

/// Metrics summary for reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub total_tasks: usize,
    pub cache_hit_rate: f64,
    pub avg_task_time_ms: u64,
    pub peak_parallelism: usize,
    pub total_build_time_s: f64,
}
