//! Executor Pool for Parallel Task Execution
//!
//! This module provides a pool of external executors that can execute tasks in parallel.
//! It manages multiple executor instances and distributes work across them using a
//! work-stealing or round-robin strategy.

use super::external::{
    ExecutorCapabilities, ExecutorConfig, ExecutorError, ExecutorHandle, ExecutorResult,
    ExecutorStatus, ExternalExecutor,
};
use super::local_executor::LocalExecutor;
use super::types::TaskSpec;
use super::wasm_executor::WasmExecutorHost;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, info, warn};

/// Pool of external executors for parallel task execution
pub struct ExecutorPool {
    /// Configuration
    config: ExecutorConfig,

    /// Executor handles
    executors: Vec<Arc<Mutex<ExecutorHandle>>>,

    /// Semaphore for limiting concurrent tasks
    semaphore: Arc<Semaphore>,

    /// Next executor to use (for round-robin)
    next_executor: Arc<Mutex<usize>>,
}

impl ExecutorPool {
    /// Create a new executor pool with the given configuration
    pub async fn new(config: ExecutorConfig) -> ExecutorResult<Self> {
        info!(
            "Creating executor pool with {} workers",
            config.max_parallel
        );

        let mut executors = Vec::new();

        // Create executor instances based on backend type
        for i in 0..config.max_parallel {
            let handle = Self::create_executor(&config, i).await?;
            executors.push(Arc::new(Mutex::new(handle)));
        }

        Ok(Self {
            semaphore: Arc::new(Semaphore::new(config.max_parallel)),
            config,
            executors,
            next_executor: Arc::new(Mutex::new(0)),
        })
    }

    /// Create a single executor instance
    async fn create_executor(config: &ExecutorConfig, index: usize) -> ExecutorResult<ExecutorHandle> {
        let name = format!("executor-{}", index);
        info!("Creating executor: {}", name);

        match &config.backend {
            super::external::ExecutorBackend::Local => {
                let mut executor = LocalExecutor::new(
                    name.clone(),
                    config.cache_dir.clone(),
                    config.channel_buffer_size,
                );

                let (tx, rx) = executor.start().await?;
                Ok(ExecutorHandle::new(name, tx, rx))
            }

            super::external::ExecutorBackend::Wasm { component_path } => {
                let mut executor = WasmExecutorHost::new(
                    name.clone(),
                    component_path.clone(),
                    config.channel_buffer_size,
                );

                let (tx, rx) = executor.start().await?;
                Ok(ExecutorHandle::new(name, tx, rx))
            }

            super::external::ExecutorBackend::Remote { endpoint } => {
                // Future: implement remote executor
                Err(ExecutorError::ExecutionFailed(format!(
                    "Remote executor not yet implemented (endpoint: {})",
                    endpoint
                )))
            }
        }
    }

    /// Execute a task using an available executor from the pool
    ///
    /// This will block if all executors are busy until one becomes available.
    pub async fn execute_task(
        &self,
        spec: TaskSpec,
    ) -> ExecutorResult<super::types::TaskOutput> {
        // Acquire semaphore permit (blocks if all executors busy)
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| ExecutorError::NotAvailable)?;

        // Get next executor (round-robin)
        let executor = self.get_next_executor().await;

        // Execute task
        debug!(
            "Executing task {}:{} on executor {}",
            spec.recipe,
            spec.name,
            executor.lock().await.name()
        );

        let result = executor.lock().await.execute_task(spec).await;

        // Permit is automatically released when dropped

        result
    }

    /// Get the next executor in round-robin fashion
    async fn get_next_executor(&self) -> Arc<Mutex<ExecutorHandle>> {
        let mut next = self.next_executor.lock().await;
        let index = *next;
        *next = (*next + 1) % self.executors.len();
        drop(next);

        self.executors[index].clone()
    }

    /// Get status of all executors in the pool
    pub async fn get_all_status(&self) -> ExecutorResult<Vec<ExecutorStatus>> {
        let mut statuses = Vec::new();

        for executor in &self.executors {
            let status = executor.lock().await.get_status().await?;
            statuses.push(status);
        }

        Ok(statuses)
    }

    /// Ping all executors to check health
    pub async fn ping_all(&self) -> ExecutorResult<Vec<bool>> {
        let mut results = Vec::new();

        for executor in &self.executors {
            match executor.lock().await.ping().await {
                Ok(()) => results.push(true),
                Err(_) => results.push(false),
            }
        }

        Ok(results)
    }

    /// Get capabilities of the first executor (all should be same)
    pub async fn get_capabilities(&self) -> ExecutorResult<ExecutorCapabilities> {
        if let Some(executor) = self.executors.first() {
            executor.lock().await.get_capabilities().await
        } else {
            Err(ExecutorError::NotAvailable)
        }
    }

    /// Shutdown all executors in the pool
    pub async fn shutdown(&self) -> ExecutorResult<()> {
        info!("Shutting down executor pool");

        for executor in &self.executors {
            match executor.lock().await.shutdown().await {
                Ok(()) => {}
                Err(e) => {
                    warn!("Error shutting down executor: {}", e);
                }
            }
        }

        info!("Executor pool shutdown complete");
        Ok(())
    }

    /// Get the number of executors in the pool
    pub fn size(&self) -> usize {
        self.executors.len()
    }

    /// Get the number of available executor slots
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Get aggregate statistics from all executors
    pub async fn aggregate_stats(&self) -> ExecutorResult<AggregateStats> {
        let statuses = self.get_all_status().await?;

        let mut stats = AggregateStats {
            total_executors: statuses.len(),
            healthy_executors: 0,
            total_executed: 0,
            successful: 0,
            failed: 0,
            active_tasks: 0,
        };

        for status in statuses {
            if status.healthy {
                stats.healthy_executors += 1;
            }
            stats.total_executed += status.total_executed;
            stats.successful += status.successful;
            stats.failed += status.failed;
            stats.active_tasks += status.active_tasks;
        }

        Ok(stats)
    }
}

/// Aggregate statistics across all executors
#[derive(Debug, Clone)]
pub struct AggregateStats {
    /// Total number of executors in pool
    pub total_executors: usize,

    /// Number of healthy executors
    pub healthy_executors: usize,

    /// Total tasks executed across all executors
    pub total_executed: u64,

    /// Total successful executions
    pub successful: u64,

    /// Total failed executions
    pub failed: u64,

    /// Currently active tasks
    pub active_tasks: usize,
}

impl AggregateStats {
    /// Calculate success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        if self.total_executed == 0 {
            100.0
        } else {
            (self.successful as f64 / self.total_executed as f64) * 100.0
        }
    }

    /// Check if the pool is healthy (all executors healthy)
    pub fn is_healthy(&self) -> bool {
        self.healthy_executors == self.total_executors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_executor_pool_creation() {
        let tmp = TempDir::new().unwrap();
        let config = ExecutorConfig {
            backend: super::super::external::ExecutorBackend::Local,
            cache_dir: tmp.path().to_path_buf(),
            max_parallel: 2,
            channel_buffer_size: 10,
            verbose: false,
        };

        let pool = ExecutorPool::new(config).await.unwrap();
        assert_eq!(pool.size(), 2);
        assert_eq!(pool.available(), 2);

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_executor_pool_ping_all() {
        let tmp = TempDir::new().unwrap();
        let config = ExecutorConfig {
            backend: super::super::external::ExecutorBackend::Local,
            cache_dir: tmp.path().to_path_buf(),
            max_parallel: 2,
            channel_buffer_size: 10,
            verbose: false,
        };

        let pool = ExecutorPool::new(config).await.unwrap();

        let results = pool.ping_all().await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|&x| x));

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_executor_pool_capabilities() {
        let tmp = TempDir::new().unwrap();
        let config = ExecutorConfig {
            backend: super::super::external::ExecutorBackend::Local,
            cache_dir: tmp.path().to_path_buf(),
            max_parallel: 2,
            channel_buffer_size: 10,
            verbose: false,
        };

        let pool = ExecutorPool::new(config).await.unwrap();

        let caps = pool.get_capabilities().await.unwrap();
        assert_eq!(caps.caching, true);

        pool.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_executor_pool_aggregate_stats() {
        let tmp = TempDir::new().unwrap();
        let config = ExecutorConfig {
            backend: super::super::external::ExecutorBackend::Local,
            cache_dir: tmp.path().to_path_buf(),
            max_parallel: 2,
            channel_buffer_size: 10,
            verbose: false,
        };

        let pool = ExecutorPool::new(config).await.unwrap();

        let stats = pool.aggregate_stats().await.unwrap();
        assert_eq!(stats.total_executors, 2);
        assert_eq!(stats.healthy_executors, 2);
        assert!(stats.is_healthy());

        pool.shutdown().await.unwrap();
    }
}
