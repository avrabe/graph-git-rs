//! Bitzel - Bazel-inspired build orchestrator for BitBake/Yocto projects
//!
//! Supports two modes of operation:
//! 1. KAS mode: Build using KAS configuration files
//! 2. Native BitBake mode: Build using standard BitBake configuration

mod commands;

use clap::Parser;
use commands::{Cli, Commands};
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

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║              BITZEL BUILD ORCHESTRATOR                 ║");
    println!("║  Layer-aware BitBake with override resolution         ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    // Dispatch to appropriate command
    match cli.command {
        Commands::Kas { config, builddir, target } => {
            println!("Mode: KAS Configuration");
            println!("Config: {:?}", config);
            println!();
            commands::kas::execute(&config, &builddir, target).await?;
        }
        Commands::Build { builddir, target } => {
            println!("Mode: Native BitBake");
            println!("Build directory: {:?}", builddir);
            println!("Target: {}", target);
            println!();
            commands::build::execute(&builddir, &target).await?;
        }
    }

    Ok(())
}
