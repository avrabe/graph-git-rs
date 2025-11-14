//! Bitzel - Bazel-inspired build orchestrator for BitBake/Yocto projects
//!
//! Bitzel is a thin orchestration layer that combines:
//! - **convenient-kas**: KAS YAML configuration parsing
//! - **convenient-bitbake**: Recipe parsing, dependency graphs, task execution
//! - **convenient-cache**: Bazel Remote Execution API v2 caching
//! - **convenient-graph**: Generic DAG library
//!
//! ## Architecture
//!
//! Bitzel does NOT reimplement BitBake functionality. Instead, it orchestrates
//! existing components:
//!
//! 1. **Recipe Parsing**: Uses `RecipeExtractor` from convenient-bitbake
//! 2. **Dependency Resolution**: Uses `RecipeGraph` with virtual provider support
//! 3. **Task Graphs**: Uses `TaskGraphBuilder` for executable task DAGs
//! 4. **Execution**: Uses `TaskExecutor` with hermetic sandboxing and caching
//!
//! ## Usage
//!
//! ```no_run
//! use bitzel::*;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // 1. Load KAS configuration
//! let kas = KasIncludeGraph::build("project.yml").await?;
//!
//! // 2. Parse recipes
//! let extractor = RecipeExtractor::new(ExtractionConfig::default());
//! let mut graph = RecipeGraph::new();
//!
//! // 3. Build dependency graph
//! // ... (see main.rs for complete example)
//! # Ok(())
//! # }
//! ```

// Re-export core types from convenient-bitbake
pub use convenient_bitbake::{
    // Recipe parsing
    RecipeExtractor,
    ExtractionConfig,
    BitbakeRecipe,

    // Dependency graph
    RecipeGraph,
    RecipeId,

    // Task graph
    TaskGraphBuilder,
    TaskId,

    // Execution
    TaskExecutor,
    TaskSpec,
    TaskOutput,
    ExecutionResult,
};

// Re-export KAS types
pub use convenient_kas::{
    KasConfig,
    KasIncludeGraph,
    KasFile,
};

// Re-export caching
pub use convenient_cache::{
    BazelRemoteCache,
    ContentHash,
};

// Re-export graph library
pub use convenient_graph::DAG;

// Sandboxing with Linux namespaces and OverlayFS
pub mod sandbox;
pub use sandbox::{
    Sandbox, SandboxBuilder, SandboxConfig, SandboxError,
    DependencyLayer, Result as SandboxResult,
};
