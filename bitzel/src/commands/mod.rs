//! Bitzel command-line interface
//!
//! Bitzel supports multiple modes of operation:
//! - `kas`: Build using KAS configuration files (existing mode)
//! - `build`: Build using native BitBake configuration (new mode)

use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub mod kas;
pub mod build;

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

    /// Build using native BitBake configuration
    Build {
        /// Build directory (must contain conf/bblayers.conf and conf/local.conf)
        #[arg(short, long, default_value = "build")]
        builddir: PathBuf,

        /// Target recipe to build
        target: String,
    },
}
