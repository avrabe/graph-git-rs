//! Query engine for recipe and task graphs
//!
//! Inspired by Bazel's query, cquery, and aquery commands, this module provides
//! a powerful query language for exploring build dependencies and execution state.
//!
//! # Query Types
//!
//! - **Recipe Queries**: Fast queries on the recipe dependency graph
//! - **Task Queries**: Queries on configured task execution graph
//! - **Execution Queries**: Queries on execution logs and artifacts
//!
//! # Query Functions
//!
//! - `deps(target, max_depth)` - All dependencies of a target
//! - `rdeps(universe, target)` - Reverse dependencies (what depends on target)
//! - `somepath(from, to)` - Find a dependency path between targets
//! - `allpaths(from, to)` - Find all dependency paths
//! - `kind(pattern, expr)` - Filter by target type
//! - `filter(pattern, expr)` - Filter by label pattern
//!
//! # Example
//!
//! ```rust,ignore
//! use convenient_bitbake::query::{Query, RecipeQuery};
//! use convenient_bitbake::RecipeGraph;
//!
//! let graph = RecipeGraph::new();
//! // ... populate graph ...
//!
//! // Find all dependencies of busybox
//! let query = Query::parse("deps(meta-core:busybox)")?;
//! let results = query.execute(&graph)?;
//!
//! // Find reverse dependencies (what depends on glibc)
//! let query = Query::parse("rdeps(//..., meta-core:glibc)")?;
//! let results = query.execute(&graph)?;
//! ```

pub mod parser;
pub mod expr;
pub mod recipe_query;
pub mod output;

pub use parser::QueryParser;
pub use expr::{QueryExpr, TargetPattern};
pub use recipe_query::RecipeQueryEngine;
pub use output::{OutputFormat, QueryResult, format_results};
