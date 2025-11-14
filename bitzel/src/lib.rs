//! Bitzel - Bazel-inspired build system for BitBake/Yocto projects
//!
//! Bitzel provides:
//! - Content-based caching with Bazel Remote Execution API v2
//! - Incremental builds using DAG-based dependency tracking
//! - Kas configuration support for Yocto/OpenEmbedded
//! - Parallel task execution

pub mod task;
pub mod builder;
pub mod executor;
pub mod cache;

pub use task::BitBakeTask;
pub use builder::TaskGraphBuilder;
pub use executor::BuildExecutor;
pub use cache::CacheManager;

use std::error::Error as StdError;

/// Result type for Bitzel operations
pub type Result<T> = std::result::Result<T, Box<dyn StdError>>;
