//! Cache cleaning and management commands

use convenient_bitbake::executor::CacheManager;
use std::path::Path;

/// Clean build cache (removes action cache, keeps CAS)
pub fn clean(
    build_dir: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🧹 Cleaning build cache...");
    println!();

    let cache_dir = build_dir.join("hitzeleiter-cache");
    let manager = CacheManager::new(&cache_dir);

    let stats = manager.clean()?;

    println!("✅ Cache cleaned successfully!");
    println!();
    println!("Removed:");
    println!("  Action cache entries: {}", stats.action_cache_entries);
    println!();
    println!("Kept:");
    println!("  CAS objects (for potential reuse)");
    println!();
    println!("💡 Use 'hitzeleiter clean --all' to remove everything");

    Ok(())
}

/// Expunge all cache data (CAS + action cache + sandboxes)
pub fn expunge(
    build_dir: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🗑️  Expunging all build cache...");
    println!();

    let cache_dir = build_dir.join("hitzeleiter-cache");
    let manager = CacheManager::new(&cache_dir);

    let stats = manager.expunge()?;

    println!("✅ Cache expunged successfully!");
    println!();
    println!("Removed:");
    println!("  CAS objects:          {} ({:.1} MB)",
        stats.cas_objects,
        stats.cas_bytes as f64 / 1_000_000.0
    );
    println!("  Action cache entries: {}", stats.action_cache_entries);
    println!("  Sandboxes:            {}", stats.sandboxes);
    println!();
    println!("Total space freed:      {:.1} MB", stats.cas_bytes as f64 / 1_000_000.0);

    Ok(())
}

/// Show cache information and statistics
pub fn info(
    build_dir: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("ℹ️  Cache Information");
    println!("╔════════════════════════════════════════════════════════╗");
    println!();

    let cache_dir = build_dir.join("hitzeleiter-cache");

    if !cache_dir.exists() {
        println!("No cache found at: {:?}", cache_dir);
        println!();
        println!("Run a build to create the cache.");
        return Ok(());
    }

    let manager = CacheManager::new(&cache_dir);
    let query = manager.query()?;

    println!("Cache directory: {:?}", query.cache_dir);
    println!();

    println!("📦 Content-Addressable Storage (CAS):");
    println!("  Objects:       {}", query.cas_objects);
    println!("  Total size:    {:.2} MB", query.cas_bytes as f64 / 1_000_000.0);
    println!("  Avg obj size:  {:.1} KB",
        if query.cas_objects > 0 {
            (query.cas_bytes as f64 / query.cas_objects as f64) / 1024.0
        } else {
            0.0
        }
    );
    println!();

    println!("🎯 Action Cache:");
    println!("  Cached tasks:  {}", query.action_cache_entries);
    println!();

    println!("🏖️  Sandboxes:");
    println!("  Active:        {}", query.active_sandboxes);
    println!();

    // Show disk usage
    let disk_mb = query.cas_bytes as f64 / 1_000_000.0;
    let bar_width = 50;
    let gb_scale = 10.0; // Show up to 10GB
    let filled = ((disk_mb / 1000.0 / gb_scale) * bar_width as f64).min(bar_width as f64) as usize;

    println!("💾 Disk Usage:");
    print!("  [");
    for i in 0..bar_width {
        if i < filled {
            print!("█");
        } else {
            print!("░");
        }
    }
    println!("] {:.1} GB / {:.0} GB", disk_mb / 1000.0, gb_scale);
    println!();

    println!("Available commands:");
    println!("  hitzeleiter clean       - Remove action cache (keeps CAS)");
    println!("  hitzeleiter clean --all - Remove everything (expunge)");
    println!("  hitzeleiter cache gc    - Garbage collect unused objects");

    Ok(())
}

/// Garbage collect unreferenced cache objects
pub fn gc(
    build_dir: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("♻️  Running garbage collection...");
    println!();

    let cache_dir = build_dir.join("hitzeleiter-cache");
    let manager = CacheManager::new(&cache_dir);

    let stats = manager.gc()?;

    println!("✅ Garbage collection complete!");
    println!();
    println!("Removed:");
    println!("  Objects:     {}", stats.objects_removed);
    println!("  Space freed: {:.1} MB", stats.bytes_freed as f64 / 1_000_000.0);

    Ok(())
}
