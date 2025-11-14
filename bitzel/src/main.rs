//! Bitzel - Bazel-inspired build system for BitBake/Yocto projects

use bitzel::{BuildExecutor, CacheManager, TaskGraphBuilder};
use std::path::{Path, PathBuf};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bitzel=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║              BITZEL BUILD ORCHESTRATOR                 ║");
    println!("║          Bazel-inspired BitBake build system           ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    // Configuration
    let kas_file = Path::new("examples/busybox-qemux86-64.yml");
    let build_dir = PathBuf::from("build");
    let cache_url = std::env::var("BITZEL_CACHE_URL").ok();

    if !kas_file.exists() {
        eprintln!("❌ Kas file not found: {}", kas_file.display());
        eprintln!("   Run this from the repository root directory");
        std::process::exit(1);
    }

    // Step 1: Load kas configuration and build task graph
    let mut builder = TaskGraphBuilder::from_kas_file(kas_file, build_dir).await?;

    // Display configuration
    let config = builder.config();
    println!("📋 Configuration:");
    println!("  Machine:  {}", config.machine.as_deref().unwrap_or("unknown"));
    println!("  Distro:   {}", config.distro.as_deref().unwrap_or("unknown"));
    if let Some(targets) = &config.target {
        println!("  Targets:  {}", targets.join(", "));
    }
    println!();

    // Step 2: Fetch repositories
    builder.fetch_repos().await?;

    // Step 3: Parse recipes and extract tasks
    builder.parse_recipes()?;

    // Step 4: Build task dependency graph
    let dag = builder.build_graph()?;

    // Step 5: Execute build with caching
    let cache = CacheManager::new(cache_url.as_deref())?;
    let mut executor = BuildExecutor::new(cache);

    let stats = executor.execute_build(&dag).await?;

    // Display results
    stats.display();

    println!("\n✅ Build completed successfully!\n");

    Ok(())
}
