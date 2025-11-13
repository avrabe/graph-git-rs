//! Cache Management Example - Bazel-inspired cache commands
//!
//! Demonstrates: clean, expunge, query operations on Bitzel cache

use convenient_bitbake::{CacheManager, ExecutionResult};
use tempfile::TempDir;

fn main() -> ExecutionResult<()> {
    println!("=== Bitzel Cache Management Demo ===\n");

    // Create a temporary cache directory for demo
    let tmp = TempDir::new()?;
    let cache_dir = tmp.path().join("bitzel-cache");

    // Simulate some cache content
    std::fs::create_dir_all(cache_dir.join("cas/sha256/ab/cd"))?;
    std::fs::write(cache_dir.join("cas/sha256/ab/cd/abcd1234"), b"cached data 1")?;
    std::fs::write(cache_dir.join("cas/sha256/ab/cd/abcd5678"), b"cached data 2")?;

    std::fs::create_dir_all(cache_dir.join("action-cache/12"))?;
    std::fs::write(cache_dir.join("action-cache/12/1234.json"), b"{}")?;
    std::fs::write(cache_dir.join("action-cache/12/5678.json"), b"{}")?;

    std::fs::create_dir_all(cache_dir.join("sandboxes/old-sandbox"))?;

    let manager = CacheManager::new(&cache_dir);

    println!("📁 Cache directory: {}\n", cache_dir.display());

    // QUERY - Show cache status
    println!("=== Query Cache ===");
    let query = manager.query()?;
    println!("Cache location:      {}", query.cache_dir.display());
    println!("CAS objects:         {}", query.cas_objects);
    println!("CAS size:            {} bytes", query.cas_bytes);
    println!("Action cache entries: {}", query.action_cache_entries);
    println!("Active sandboxes:    {}", query.active_sandboxes);
    println!();

    // CLEAN - Remove action cache only
    println!("=== Clean (remove action cache) ===");
    let clean_stats = manager.clean()?;
    println!("Removed {} action cache entries", clean_stats.action_cache_entries);
    println!("✓ CAS preserved (can restore builds from signatures)");
    println!();

    // Query again to show CAS is preserved
    let query_after_clean = manager.query()?;
    println!("After clean:");
    println!("  CAS objects:         {}", query_after_clean.cas_objects);
    println!("  Action cache entries: {}", query_after_clean.action_cache_entries);
    println!();

    // EXPUNGE - Remove everything
    println!("=== Expunge (remove all cache) ===");
    let expunge_stats = manager.expunge()?;
    println!("Removed {} CAS objects ({} bytes)",
             expunge_stats.cas_objects,
             expunge_stats.cas_bytes);
    println!("Removed {} action cache entries", expunge_stats.action_cache_entries);
    println!("Removed {} sandboxes", expunge_stats.sandboxes);
    println!("✓ Complete cache purge (fresh start)");
    println!();

    // Query to verify everything is gone
    let query_after_expunge = manager.query()?;
    println!("After expunge:");
    println!("  CAS objects:         {}", query_after_expunge.cas_objects);
    println!("  Action cache entries: {}", query_after_expunge.action_cache_entries);
    println!();

    println!("✨ Cache management demo complete!\n");

    println!("Usage in real projects:");
    println!("  bitzel clean        - Remove action cache (force rebuild, keep CAS)");
    println!("  bitzel expunge      - Remove all cache (complete clean slate)");
    println!("  bitzel query        - Show cache statistics");
    println!("  bitzel gc           - Garbage collect unused CAS objects (future)");

    Ok(())
}
