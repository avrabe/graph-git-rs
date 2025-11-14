//! Build executor with caching support

use crate::cache::CacheManager;
use crate::task::BitBakeTask;
use crate::Result;
use convenient_graph::DAG;
use std::time::Instant;

/// Executes build tasks with caching
pub struct BuildExecutor {
    cache: CacheManager,
}

impl BuildExecutor {
    /// Create a new build executor
    pub fn new(cache: CacheManager) -> Self {
        Self { cache }
    }

    /// Execute a single task
    async fn execute_task(&mut self, task: &BitBakeTask, content_hash: &str) -> Result<bool> {
        // Check cache first
        if self.cache.has(content_hash).await? {
            tracing::info!(
                "✓ CACHE HIT  {} ({}...)",
                task.qualified_name(),
                &content_hash[..8]
            );
            return Ok(false); // Not executed, cached
        }

        // Execute task
        tracing::info!(
            "⚡ EXECUTING  {} ({}...)",
            task.qualified_name(),
            &content_hash[..8]
        );

        // Create workdir if needed
        std::fs::create_dir_all(&task.workdir)?;

        // Simulate task execution
        // In a real implementation, this would:
        // 1. Set up the build environment
        // 2. Run the actual BitBake task command
        // 3. Capture outputs
        // 4. Verify outputs were created
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Create stamp file
        std::fs::write(task.stamp_file(), content_hash)?;

        // Store in cache
        let output_data = format!(
            "{} completed at {}",
            task.qualified_name(),
            chrono::Utc::now()
        );
        self.cache.put(output_data.as_bytes()).await?;

        Ok(true) // Executed
    }

    /// Execute the build from a task DAG
    pub async fn execute_build(&mut self, dag: &DAG<BitBakeTask, ()>) -> Result<BuildStats> {
        tracing::info!("Executing build...");
        let start_time = Instant::now();

        let order = dag.topological_sort()?;
        let mut executed_count = 0;

        for node_id in order {
            let task = dag.node(node_id)?;
            let content_hash = dag.content_hash(node_id)?;

            if self.execute_task(task, &content_hash).await? {
                executed_count += 1;
            }
        }

        let (hits, misses, hit_rate) = self.cache.stats();
        let duration = start_time.elapsed();

        Ok(BuildStats {
            executed: executed_count,
            cached: hits,
            total: executed_count + hits,
            hit_rate,
            duration,
        })
    }
}

/// Build execution statistics
#[derive(Debug, Clone)]
pub struct BuildStats {
    /// Number of tasks executed
    pub executed: usize,

    /// Number of cache hits
    pub cached: usize,

    /// Total number of tasks
    pub total: usize,

    /// Cache hit rate percentage
    pub hit_rate: f64,

    /// Build duration
    pub duration: std::time::Duration,
}

impl BuildStats {
    /// Display build statistics
    pub fn display(&self) {
        println!("\n📊 Build Summary:");
        println!("  Tasks executed: {}", self.executed);
        println!("  Cache hits:     {}", self.cached);
        println!("  Total tasks:    {}", self.total);
        println!("  Cache hit rate: {:.1}%", self.hit_rate);
        println!("  Duration:       {:.2}s", self.duration.as_secs_f64());
    }
}
