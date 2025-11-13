//! Task execution engine with Bazel-inspired caching and sandboxing
//!
//! This module implements a hermetic, cached task executor that combines:
//! - BitBake's task model (do_fetch, do_compile, etc.)
//! - Bazel's content-addressable caching
//! - Linux namespace sandboxing for reproducibility

pub mod types;
pub mod cache;
pub mod sandbox;
pub mod executor;

pub use types::{
    TaskSignature, TaskOutput, TaskSpec, SandboxSpec,
    ContentHash, ExecutionResult,
};
pub use cache::{ContentAddressableStore, ActionCache};
pub use sandbox::SandboxManager;
pub use executor::TaskExecutor;
