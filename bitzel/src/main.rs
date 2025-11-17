//! Bitzel - Bazel-inspired build orchestrator for BitBake/Yocto projects
//!
//! Supports multiple modes of operation:
//! 1. KAS mode: Build using KAS configuration files
//! 2. Build mode: Basic native BitBake builds
//! 3. Ferrari mode: Full-featured builds with all optimizations
//! 4. Clean/Cache: Cache management
//! 5. Query: Dependency exploration

mod commands;

use clap::Parser;
use commands::{Cli, Commands, CacheOperation};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bitzel=info,convenient_bitbake=info,convenient_kas=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse command-line arguments
    let cli = Cli::parse();

    // Dispatch to appropriate command
    match cli.command {
        Commands::Kas { config, builddir, target } => {
            println!("\n╔════════════════════════════════════════════════════════╗");
            println!("║              BITZEL ENVIRONMENT SETUP                  ║");
            println!("║  KAS-based BitBake environment initialization         ║");
            println!("╚════════════════════════════════════════════════════════╝\n");
            println!("Mode: KAS Configuration");
            println!("Config: {:?}", config);
            println!();
            commands::kas::execute(&config, &builddir, target).await?;
        }
        Commands::Build { builddir, target } => {
            println!("\n╔════════════════════════════════════════════════════════╗");
            println!("║              BITZEL BUILD ORCHESTRATOR                 ║");
            println!("║  Task Graph Execution with Dependency Resolution      ║");
            println!("╚════════════════════════════════════════════════════════╝\n");
            println!("Build directory: {:?}", builddir);
            println!("Target: {}", target);
            println!();
            commands::build::execute(&builddir, &target).await?;
        }
        Commands::Clean { builddir, all } => {
            if all {
                commands::clean::expunge(&builddir)?;
            } else {
                commands::clean::clean(&builddir)?;
            }
        }
        Commands::Cache { builddir, operation } => {
            match operation {
                CacheOperation::Info => {
                    commands::clean::info(&builddir)?;
                }
                CacheOperation::Gc => {
                    commands::clean::gc(&builddir)?;
                }
            }
        }
        Commands::Query { builddir, query, format } => {
            commands::query::execute(&builddir, &query, &format).await?;
        }
        Commands::QueryHelp => {
            commands::query::help();
        }
    }

    Ok(())
}
