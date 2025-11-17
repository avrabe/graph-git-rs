//! Performance benchmarking infrastructure

use std::time::{Duration, Instant};
use std::collections::HashMap;

/// Benchmark result
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub name: String,
    pub iterations: usize,
    pub total_time: Duration,
    pub min_time: Duration,
    pub max_time: Duration,
    pub avg_time: Duration,
}

impl BenchmarkResult {
    pub fn throughput(&self, items_per_iter: usize) -> f64 {
        let total_items = self.iterations * items_per_iter;
        let secs = self.total_time.as_secs_f64();
        if secs == 0.0 { 0.0 } else { total_items as f64 / secs }
    }
}

/// Benchmark suite
pub struct BenchmarkSuite {
    results: HashMap<String, BenchmarkResult>,
}

impl BenchmarkSuite {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
        }
    }

    /// Run a benchmark
    pub fn bench<F>(&mut self, name: &str, iterations: usize, mut f: F)
    where
        F: FnMut(),
    {
        let mut times = Vec::new();
        let start = Instant::now();

        for _ in 0..iterations {
            let iter_start = Instant::now();
            f();
            times.push(iter_start.elapsed());
        }

        let total_time = start.elapsed();
        let min_time = times.iter().min().copied().unwrap_or_default();
        let max_time = times.iter().max().copied().unwrap_or_default();
        let avg_time = total_time / iterations as u32;

        let result = BenchmarkResult {
            name: name.to_string(),
            iterations,
            total_time,
            min_time,
            max_time,
            avg_time,
        };

        self.results.insert(name.to_string(), result);
    }

    /// Print results
    pub fn print_results(&self) {
        println!("\nBenchmark Results:");
        println!("{:-<80}", "");
        println!("{:<30} {:>10} {:>12} {:>12} {:>12}",
            "Name", "Iterations", "Total (ms)", "Avg (µs)", "Ops/sec");
        println!("{:-<80}", "");

        for result in self.results.values() {
            let ops_per_sec = 1.0 / result.avg_time.as_secs_f64();
            println!("{:<30} {:>10} {:>12.2} {:>12.2} {:>12.0}",
                result.name,
                result.iterations,
                result.total_time.as_millis(),
                result.avg_time.as_micros(),
                ops_per_sec,
            );
        }
    }

    /// Get result
    pub fn get(&self, name: &str) -> Option<&BenchmarkResult> {
        self.results.get(name)
    }
}

impl Default for BenchmarkSuite {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark() {
        let mut suite = BenchmarkSuite::new();

        suite.bench("test", 100, || {
            let _x = 2 + 2;
        });

        let result = suite.get("test").unwrap();
        assert_eq!(result.iterations, 100);
        assert!(result.avg_time.as_nanos() > 0);
    }
}
