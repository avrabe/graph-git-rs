//! Task execution engine with Bazel-inspired caching and sandboxing
//!
//! This module implements a hermetic, cached task executor that combines:
//! - BitBake's task model (do_fetch, do_compile, etc.)
//! - Bazel's content-addressable caching
//! - Linux namespace sandboxing for reproducibility

pub mod types;
pub mod cache;
pub mod sandbox_backend;
pub mod native_sandbox;
pub mod sandbox;
pub mod executor;
pub mod execution_log;
pub mod cache_manager;
pub mod async_executor;
pub mod monitor;
pub mod interactive;
pub mod remote_cache;
pub mod script_analyzer;
pub mod direct_executor;
pub mod fetch_handler;

// External executor abstraction
pub mod external;
pub mod local_executor;
pub mod wasm_executor;
pub mod executor_pool;

pub use types::{
    TaskSignature, TaskOutput, TaskSpec, SandboxSpec,
    ContentHash, ExecutionResult, NetworkPolicy, ResourceLimits,
};
pub use cache::{ContentAddressableStore, ActionCache};
pub use sandbox::SandboxManager;
pub use sandbox_backend::SandboxBackend;
pub use executor::TaskExecutor;
pub use execution_log::{ExecutionLog, ExecutionOutcome, ExecutionError, ErrorCategory, ExecutionMetrics};
pub use cache_manager::{CacheManager, CacheQuery, CleanStats, ExpungeStats};
pub use async_executor::AsyncTaskExecutor;
pub use monitor::{TaskMonitor, TaskInfo, TaskState, BuildStats};
pub use interactive::{InteractiveExecutor, InteractiveOptions, ExecutionControlHandle};
pub use remote_cache::{RemoteCacheClient, RemoteCacheConfig, ActionResult, OutputFile, ExecutionMetadata};
pub use script_analyzer::{ScriptAnalysis, DirectAction, analyze_script};
pub use direct_executor::{execute_direct, DirectExecutionResult};
pub use fetch_handler::{fetch_source, FetchError, FetchResult};

// External executor types
pub use external::{
    ExternalExecutor, ExecutorHandle, ExecutorMessage, ExecutorResponse,
    ExecutorStatus, ExecutorCapabilities, ExecutorConfig, ExecutorBackend,
    ExecutorError, ExecutorResult,
};
pub use local_executor::LocalExecutor;
pub use wasm_executor::WasmExecutorHost;
pub use executor_pool::{ExecutorPool, AggregateStats};

#[cfg(target_os = "linux")]
pub use native_sandbox::execute_in_namespace;
