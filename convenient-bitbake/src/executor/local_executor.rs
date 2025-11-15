//! Local in-process executor implementation
//!
//! This module provides a local executor that wraps the TaskExecutor and provides
//! channel-based communication. It runs in-process but uses the same message protocol
//! as the future WASM component executor, making it easy to swap implementations.

use super::executor::{ExecutionStats, TaskExecutor};
use super::external::{
    ExecutorCapabilities, ExecutorError, ExecutorMessage, ExecutorResponse, ExecutorResult,
    ExecutorStatus, ExternalExecutor,
};
use super::types::{TaskOutput, TaskSpec};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

/// Local in-process executor
///
/// This executor wraps the TaskExecutor and provides channel-based communication
/// while running in the same process. It serves as both the production implementation
/// and a reference for how future WASM component executors should behave.
pub struct LocalExecutor {
    /// Name of this executor instance
    name: String,

    /// Directory for caching
    cache_dir: PathBuf,

    /// Channel buffer size
    channel_buffer_size: usize,

    /// The wrapped task executor (created during start)
    executor: Option<Arc<Mutex<TaskExecutor>>>,

    /// Start time for uptime tracking
    start_time: Option<Instant>,

    /// Handle to the background task
    task_handle: Option<tokio::task::JoinHandle<()>>,

    /// Shutdown signal sender
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl LocalExecutor {
    /// Create a new local executor
    pub fn new(name: String, cache_dir: PathBuf, channel_buffer_size: usize) -> Self {
        Self {
            name,
            cache_dir,
            channel_buffer_size,
            executor: None,
            start_time: None,
            task_handle: None,
            shutdown_tx: None,
        }
    }

    /// Run the executor message loop
    async fn run_message_loop(
        executor: Arc<Mutex<TaskExecutor>>,
        mut msg_rx: mpsc::Receiver<ExecutorMessage>,
        resp_tx: mpsc::Sender<ExecutorResponse>,
        mut shutdown_rx: mpsc::Receiver<()>,
        start_time: Instant,
    ) {
        info!("Local executor message loop started");

        loop {
            tokio::select! {
                // Handle shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal");
                    break;
                }

                // Handle executor messages
                msg = msg_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            Self::handle_message(
                                executor.clone(),
                                msg,
                                &resp_tx,
                                start_time,
                            ).await;
                        }
                        None => {
                            info!("Message channel closed, shutting down");
                            break;
                        }
                    }
                }
            }
        }

        info!("Local executor message loop exiting");
    }

    /// Handle a single executor message
    async fn handle_message(
        executor: Arc<Mutex<TaskExecutor>>,
        msg: ExecutorMessage,
        resp_tx: &mpsc::Sender<ExecutorResponse>,
        start_time: Instant,
    ) {
        match msg {
            ExecutorMessage::ExecuteTask { request_id, task } => {
                debug!("Executing task: {}:{}", task.recipe, task.name);

                // Execute task in blocking thread pool since TaskExecutor is sync
                let executor_clone = executor.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut exec = executor_clone.blocking_lock();
                    exec.execute_task(task)
                })
                .await;

                let response = match result {
                    Ok(Ok(output)) => ExecutorResponse::TaskResult {
                        request_id,
                        result: Ok(output),
                    },
                    Ok(Err(e)) => ExecutorResponse::TaskResult {
                        request_id,
                        result: Err(format!("Task execution failed: {}", e)),
                    },
                    Err(e) => ExecutorResponse::Error {
                        request_id,
                        error: format!("Task execution panicked: {}", e),
                    },
                };

                if let Err(e) = resp_tx.send(response).await {
                    error!("Failed to send task result response: {}", e);
                }
            }

            ExecutorMessage::GetStatus { request_id } => {
                debug!("Getting executor status");

                let exec = executor.lock().await;
                let stats = exec.stats();

                let status = ExecutorStatus {
                    healthy: true,
                    active_tasks: 0, // We don't track concurrent tasks in single-threaded executor
                    total_executed: (stats.tasks_executed) as u64,
                    successful: (stats.tasks_executed) as u64, // Failures throw errors
                    failed: 0,
                    uptime_secs: start_time.elapsed().as_secs(),
                };

                let response = ExecutorResponse::Status {
                    request_id,
                    status,
                };

                if let Err(e) = resp_tx.send(response).await {
                    error!("Failed to send status response: {}", e);
                }
            }

            ExecutorMessage::Ping { request_id } => {
                debug!("Handling ping");

                let response = ExecutorResponse::Pong { request_id };

                if let Err(e) = resp_tx.send(response).await {
                    error!("Failed to send pong response: {}", e);
                }
            }

            ExecutorMessage::Shutdown { request_id } => {
                info!("Handling shutdown request");

                let response = ExecutorResponse::ShutdownAck { request_id };

                if let Err(e) = resp_tx.send(response).await {
                    error!("Failed to send shutdown ack: {}", e);
                }

                // The shutdown will be handled by the shutdown channel
            }

            ExecutorMessage::GetCapabilities { request_id } => {
                debug!("Getting capabilities");

                let capabilities = ExecutorCapabilities {
                    sandboxing: cfg!(target_os = "linux"),
                    network_isolation: cfg!(target_os = "linux"),
                    caching: true,
                    max_parallel_tasks: 1, // Single-threaded executor
                    platforms: vec![std::env::consts::OS.to_string()],
                    version: env!("CARGO_PKG_VERSION").to_string(),
                };

                let response = ExecutorResponse::Capabilities {
                    request_id,
                    capabilities,
                };

                if let Err(e) = resp_tx.send(response).await {
                    error!("Failed to send capabilities response: {}", e);
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl ExternalExecutor for LocalExecutor {
    async fn start(
        &mut self,
    ) -> ExecutorResult<(
        mpsc::Sender<ExecutorMessage>,
        mpsc::Receiver<ExecutorResponse>,
    )> {
        info!("Starting local executor: {}", self.name);

        // Create the underlying TaskExecutor
        let executor = TaskExecutor::new(&self.cache_dir)
            .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))?;

        let executor = Arc::new(Mutex::new(executor));
        self.executor = Some(executor.clone());
        self.start_time = Some(Instant::now());

        // Create channels for message passing
        let (msg_tx, msg_rx) = mpsc::channel::<ExecutorMessage>(self.channel_buffer_size);
        let (resp_tx, resp_rx) = mpsc::channel::<ExecutorResponse>(self.channel_buffer_size);

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Spawn the message loop
        let start_time = self.start_time.unwrap();
        let handle = tokio::spawn(async move {
            Self::run_message_loop(executor, msg_rx, resp_tx, shutdown_rx, start_time).await;
        });

        self.task_handle = Some(handle);

        info!("Local executor started successfully");

        Ok((msg_tx, resp_rx))
    }

    async fn stop(&mut self) -> ExecutorResult<()> {
        info!("Stopping local executor: {}", self.name);

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Wait for the task to complete
        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(()) => {
                    info!("Local executor stopped successfully");
                }
                Err(e) => {
                    warn!("Executor task join error: {}", e);
                }
            }
        }

        self.executor = None;
        self.start_time = None;

        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities {
            sandboxing: cfg!(target_os = "linux"),
            network_isolation: cfg!(target_os = "linux"),
            caching: true,
            max_parallel_tasks: 1,
            platforms: vec![std::env::consts::OS.to_string()],
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_local_executor_start_stop() {
        let tmp = TempDir::new().unwrap();
        let mut executor = LocalExecutor::new(
            "test-executor".to_string(),
            tmp.path().to_path_buf(),
            10,
        );

        let (tx, rx) = executor.start().await.unwrap();
        drop(tx);
        drop(rx);

        executor.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_local_executor_ping() {
        let tmp = TempDir::new().unwrap();
        let mut executor = LocalExecutor::new(
            "test-executor".to_string(),
            tmp.path().to_path_buf(),
            10,
        );

        let (tx, mut rx) = executor.start().await.unwrap();

        // Send ping
        tx.send(ExecutorMessage::Ping { request_id: 42 })
            .await
            .unwrap();

        // Receive pong
        match rx.recv().await {
            Some(ExecutorResponse::Pong { request_id: 42 }) => {}
            other => panic!("Expected Pong, got: {:?}", other),
        }

        executor.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_local_executor_capabilities() {
        let tmp = TempDir::new().unwrap();
        let mut executor = LocalExecutor::new(
            "test-executor".to_string(),
            tmp.path().to_path_buf(),
            10,
        );

        let (tx, mut rx) = executor.start().await.unwrap();

        // Request capabilities
        tx.send(ExecutorMessage::GetCapabilities { request_id: 1 })
            .await
            .unwrap();

        // Receive capabilities
        match rx.recv().await {
            Some(ExecutorResponse::Capabilities {
                request_id: 1,
                capabilities,
            }) => {
                assert_eq!(capabilities.caching, true);
                assert!(capabilities.platforms.len() > 0);
            }
            other => panic!("Expected Capabilities, got: {:?}", other),
        }

        executor.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_local_executor_status() {
        let tmp = TempDir::new().unwrap();
        let mut executor = LocalExecutor::new(
            "test-executor".to_string(),
            tmp.path().to_path_buf(),
            10,
        );

        let (tx, mut rx) = executor.start().await.unwrap();

        // Request status
        tx.send(ExecutorMessage::GetStatus { request_id: 1 })
            .await
            .unwrap();

        // Receive status
        match rx.recv().await {
            Some(ExecutorResponse::Status {
                request_id: 1,
                status,
            }) => {
                assert_eq!(status.healthy, true);
                assert_eq!(status.total_executed, 0);
            }
            other => panic!("Expected Status, got: {:?}", other),
        }

        executor.stop().await.unwrap();
    }
}
