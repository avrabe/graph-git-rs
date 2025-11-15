//! Bitzel command-line interface
//!
//! Bitzel supports multiple modes of operation:
//! - `kas`: Build using KAS configuration files
//! - `build`: Build using native BitBake configuration (basic)
//! - `ferrari`: Full-featured build with all optimizations
//! - `clean`: Cache management
//! - `query`: Dependency exploration

use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub mod kas;
pub mod build;
pub mod build_ferrari;
pub mod clean;
pub mod query;

/// Bitzel - Bazel-inspired build orchestrator for BitBake/Yocto
#[derive(Parser)]
#[command(name = "bitzel")]
#[command(about = "Bazel-inspired build orchestrator for BitBake/Yocto projects")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Build using KAS configuration file
    Kas {
        /// Path to KAS configuration file
        #[arg(short, long, default_value = "kas.yml")]
        config: PathBuf,

        /// Build directory
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        /// Target recipe to build (optional)
        target: Option<String>,
    },

    /// Build using native BitBake configuration (basic mode)
    Build {
        /// Build directory (must contain conf/bblayers.conf and conf/local.conf)
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        /// Target recipe to build
        target: String,
    },

    /// 🏎️  Ferrari build - full-featured build with all optimizations
    Ferrari {
        /// Build directory (must contain conf/bblayers.conf and conf/local.conf)
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        /// Target recipe to build
        target: String,
    },

    /// Clean build cache
    Clean {
        /// Build directory
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        /// Remove everything (expunge)
        #[arg(long)]
        all: bool,
    },

    /// Cache management operations
    Cache {
        /// Build directory
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        #[command(subcommand)]
        operation: CacheOperation,
    },

    /// Query recipe dependencies
    Query {
        /// Build directory
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        /// Query expression (e.g., "deps(busybox, 2)")
        query: String,

        /// Output format: text, json, graph, label
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Show query help and examples
    QueryHelp,
}

#[derive(Subcommand)]
pub enum CacheOperation {
    /// Show cache information and statistics
    Info,

    /// Garbage collect unreferenced objects
    Gc,
}
