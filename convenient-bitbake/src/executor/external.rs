//! External Executor Abstraction
//!
//! This module provides a channel-based abstraction for task execution that can be
//! implemented by different backends:
//! - LocalExecutor: In-process execution (current implementation)
//! - WasmExecutorHost: WASM component-based execution (future)
//! - RemoteExecutor: Network-based execution (future)
//!
//! The abstraction uses message passing via channels to decouple the host from the
//! executor implementation, enabling future migration to WASM components where the
//! executor runs in a separate component and communicates via the component model.

use super::types::{TaskSpec, TaskOutput};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;
use tokio::sync::mpsc;

/// Errors that can occur in the external executor system
#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("Executor channel closed")]
    ChannelClosed,

    #[error("Executor failed to send response")]
    SendError,

    #[error("Executor failed to receive message")]
    ReceiveError,

    #[error("Task execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Executor not available")]
    NotAvailable,

    #[error("Timeout waiting for executor response")]
    Timeout,

    #[error("Invalid executor state: {0}")]
    InvalidState(String),
}

pub type ExecutorResult<T> = Result<T, ExecutorError>;

/// Messages sent from the host to the executor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutorMessage {
    /// Execute a task with the given specification
    ExecuteTask {
        /// Unique identifier for this execution request
        request_id: u64,
        /// The task to execute
        task: TaskSpec,
    },

    /// Get the current status of the executor
    GetStatus {
        request_id: u64,
    },

    /// Health check ping
    Ping {
        request_id: u64,
    },

    /// Request executor to shutdown gracefully
    Shutdown {
        request_id: u64,
    },

    /// Query executor capabilities
    GetCapabilities {
        request_id: u64,
    },
}

/// Responses sent from the executor to the host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutorResponse {
    /// Result of task execution
    TaskResult {
        request_id: u64,
        result: Result<TaskOutput, String>,
    },

    /// Current executor status
    Status {
        request_id: u64,
        status: ExecutorStatus,
    },

    /// Pong response to ping
    Pong {
        request_id: u64,
    },

    /// Acknowledgment of shutdown
    ShutdownAck {
        request_id: u64,
    },

    /// Executor capabilities
    Capabilities {
        request_id: u64,
        capabilities: ExecutorCapabilities,
    },

    /// Generic error response
    Error {
        request_id: u64,
        error: String,
    },
}

/// Current status of an executor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorStatus {
    /// Whether the executor is healthy and can accept tasks
    pub healthy: bool,

    /// Number of tasks currently executing
    pub active_tasks: usize,

    /// Total number of tasks executed since startup
    pub total_executed: u64,

    /// Number of successful task executions
    pub successful: u64,

    /// Number of failed task executions
    pub failed: u64,

    /// Executor uptime in seconds
    pub uptime_secs: u64,
}

/// Capabilities supported by an executor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorCapabilities {
    /// Whether the executor supports sandboxing
    pub sandboxing: bool,

    /// Whether the executor supports network isolation
    pub network_isolation: bool,

    /// Whether the executor supports caching
    pub caching: bool,

    /// Maximum number of parallel tasks (0 = unlimited)
    pub max_parallel_tasks: usize,

    /// Supported platforms
    pub platforms: Vec<String>,

    /// Executor version
    pub version: String,
}

/// Trait for external executor implementations
///
/// This trait abstracts the execution backend, allowing different implementations
/// to be plugged in (local in-process, WASM component, remote execution, etc.).
#[async_trait::async_trait]
pub trait ExternalExecutor: Send + Sync {
    /// Start the executor and return channels for communication
    ///
    /// Returns (sender, receiver) where:
    /// - sender: Used by host to send messages to executor
    /// - receiver: Used by host to receive responses from executor
    async fn start(
        &mut self,
    ) -> ExecutorResult<(
        mpsc::Sender<ExecutorMessage>,
        mpsc::Receiver<ExecutorResponse>,
    )>;

    /// Stop the executor gracefully
    async fn stop(&mut self) -> ExecutorResult<()>;

    /// Get executor name (for logging/debugging)
    fn name(&self) -> &str;

    /// Get executor capabilities
    async fn capabilities(&self) -> ExecutorCapabilities;
}

/// Handle for communicating with an executor instance
///
/// This provides a high-level API over the raw channels, handling request IDs
/// and response matching automatically.
pub struct ExecutorHandle {
    /// Name of the executor (for debugging)
    name: String,

    /// Channel for sending messages to the executor
    tx: mpsc::Sender<ExecutorMessage>,

    /// Channel for receiving responses from the executor
    rx: mpsc::Receiver<ExecutorResponse>,

    /// Next request ID to use
    next_request_id: u64,
}

impl ExecutorHandle {
    /// Create a new executor handle
    pub fn new(
        name: String,
        tx: mpsc::Sender<ExecutorMessage>,
        rx: mpsc::Receiver<ExecutorResponse>,
    ) -> Self {
        Self {
            name,
            tx,
            rx,
            next_request_id: 0,
        }
    }

    /// Get the next request ID
    fn next_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        id
    }

    /// Execute a task and wait for the result
    pub async fn execute_task(&mut self, task: TaskSpec) -> ExecutorResult<TaskOutput> {
        let request_id = self.next_id();

        // Send execute message
        self.tx
            .send(ExecutorMessage::ExecuteTask { request_id, task })
            .await
            .map_err(|_| ExecutorError::ChannelClosed)?;

        // Wait for response
        loop {
            match self.rx.recv().await {
                Some(ExecutorResponse::TaskResult {
                    request_id: resp_id,
                    result,
                }) if resp_id == request_id => {
                    return result.map_err(|e| ExecutorError::ExecutionFailed(e));
                }
                Some(ExecutorResponse::Error {
                    request_id: resp_id,
                    error,
                }) if resp_id == request_id => {
                    return Err(ExecutorError::ExecutionFailed(error));
                }
                Some(_) => {
                    // Response for different request, continue waiting
                    continue;
                }
                None => {
                    return Err(ExecutorError::ChannelClosed);
                }
            }
        }
    }

    /// Get executor status
    pub async fn get_status(&mut self) -> ExecutorResult<ExecutorStatus> {
        let request_id = self.next_id();

        self.tx
            .send(ExecutorMessage::GetStatus { request_id })
            .await
            .map_err(|_| ExecutorError::ChannelClosed)?;

        loop {
            match self.rx.recv().await {
                Some(ExecutorResponse::Status {
                    request_id: resp_id,
                    status,
                }) if resp_id == request_id => {
                    return Ok(status);
                }
                Some(ExecutorResponse::Error {
                    request_id: resp_id,
                    error,
                }) if resp_id == request_id => {
                    return Err(ExecutorError::ExecutionFailed(error));
                }
                Some(_) => continue,
                None => return Err(ExecutorError::ChannelClosed),
            }
        }
    }

    /// Ping the executor to check if it's alive
    pub async fn ping(&mut self) -> ExecutorResult<()> {
        let request_id = self.next_id();

        self.tx
            .send(ExecutorMessage::Ping { request_id })
            .await
            .map_err(|_| ExecutorError::ChannelClosed)?;

        loop {
            match self.rx.recv().await {
                Some(ExecutorResponse::Pong {
                    request_id: resp_id,
                }) if resp_id == request_id => {
                    return Ok(());
                }
                Some(_) => continue,
                None => return Err(ExecutorError::ChannelClosed),
            }
        }
    }

    /// Get executor capabilities
    pub async fn get_capabilities(&mut self) -> ExecutorResult<ExecutorCapabilities> {
        let request_id = self.next_id();

        self.tx
            .send(ExecutorMessage::GetCapabilities { request_id })
            .await
            .map_err(|_| ExecutorError::ChannelClosed)?;

        loop {
            match self.rx.recv().await {
                Some(ExecutorResponse::Capabilities {
                    request_id: resp_id,
                    capabilities,
                }) if resp_id == request_id => {
                    return Ok(capabilities);
                }
                Some(_) => continue,
                None => return Err(ExecutorError::ChannelClosed),
            }
        }
    }

    /// Shutdown the executor
    pub async fn shutdown(&mut self) -> ExecutorResult<()> {
        let request_id = self.next_id();

        self.tx
            .send(ExecutorMessage::Shutdown { request_id })
            .await
            .map_err(|_| ExecutorError::ChannelClosed)?;

        loop {
            match self.rx.recv().await {
                Some(ExecutorResponse::ShutdownAck {
                    request_id: resp_id,
                }) if resp_id == request_id => {
                    return Ok(());
                }
                Some(_) => continue,
                None => return Err(ExecutorError::ChannelClosed),
            }
        }
    }

    /// Get the executor name
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Configuration for external executors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorConfig {
    /// Type of executor backend to use
    pub backend: ExecutorBackend,

    /// Directory for caching task results
    pub cache_dir: PathBuf,

    /// Maximum number of parallel tasks
    pub max_parallel: usize,

    /// Channel buffer size for message passing
    pub channel_buffer_size: usize,

    /// Enable verbose logging
    pub verbose: bool,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            backend: ExecutorBackend::Local,
            cache_dir: PathBuf::from("/tmp/hitzeleiter-cache"),
            max_parallel: num_cpus::get(),
            channel_buffer_size: 100,
            verbose: false,
        }
    }
}

/// Executor backend type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutorBackend {
    /// In-process execution using the local TaskExecutor
    Local,

    /// WASM component-based execution (future)
    Wasm {
        /// Path to the WASM component
        component_path: PathBuf,
    },

    /// Remote execution via network
    Remote {
        /// Remote executor endpoint
        endpoint: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_config_default() {
        let config = ExecutorConfig::default();
        assert!(matches!(config.backend, ExecutorBackend::Local));
        assert!(config.max_parallel > 0);
        assert_eq!(config.channel_buffer_size, 100);
    }

    #[test]
    fn test_executor_message_serialization() {
        let msg = ExecutorMessage::Ping { request_id: 42 };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ExecutorMessage = serde_json::from_str(&json).unwrap();
        matches!(deserialized, ExecutorMessage::Ping { request_id: 42 });
    }

    #[test]
    fn test_executor_response_serialization() {
        let resp = ExecutorResponse::Pong { request_id: 42 };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: ExecutorResponse = serde_json::from_str(&json).unwrap();
        matches!(deserialized, ExecutorResponse::Pong { request_id: 42 });
    }
}
