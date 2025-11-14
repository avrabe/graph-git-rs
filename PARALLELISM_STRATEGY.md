# Parallelism Strategy: Async Runtime for BitBake Task Execution

## The Question

**Which async runtime should we use for parallel task execution?**

Options: tokio, wstd (WebAssembly), rayon, or std::thread?

## What Do We Actually Need?

For BitBake task execution:

1. **Spawn external processes** (gcc, make, tar, etc.)
2. **Wait for processes to complete**
3. **Run multiple independent tasks in parallel**
4. **Respect task dependencies** (wait for deps)
5. **Collect outputs** (file I/O)

## Key Insight: Tasks Are NOT Async I/O

**BitBake tasks spawn external processes**:

```rust
// A typical task execution
let output = Command::new("gcc")
    .args(&["-c", "busybox.c", "-o", "busybox.o"])
    .current_dir(&sandbox_dir)
    .output()?;  // ← Blocks waiting for gcc to finish
```

**This is blocking I/O**, not async I/O!

When we spawn gcc:
- We wait for it to finish (seconds to minutes)
- gcc is an external process doing its own work
- No benefit from async I/O here

## Four Options

### Option 1: wstd (WebAssembly Async Runtime) - SELECTED

**wstd** is a minimal async standard library for WebAssembly Components and WASI 0.2:

```rust
use wstd::runtime;

fn main() {
    runtime::block_on(async {
        execute_task_graph(&task_graph, &sig_cache, &art_cache).await
    });
}

async fn execute_task_graph(
    task_graph: &TaskGraph,
    signature_cache: &SignatureCache,
    artifact_cache: &ArtifactCache,
) -> Result<ExecutionStats> {
    let mut completed = HashSet::new();

    loop {
        let ready_tasks = task_graph.get_ready_tasks(&completed);
        if ready_tasks.is_empty() { break; }

        // Spawn parallel tasks with async
        let futures: Vec<_> = ready_tasks
            .into_iter()
            .map(|task_id| {
                let task = task_graph.tasks[&task_id].clone();
                async move {
                    execute_or_fetch_cached(&task, signature_cache, artifact_cache).await
                }
            })
            .collect();

        // Wait for all to complete
        let results = futures::future::join_all(futures).await;

        for (task_id, result) in results {
            completed.insert(task_id);
        }
    }

    Ok(ExecutionStats { /* ... */ })
}

async fn execute_or_fetch_cached(
    task: &ExecutableTask,
    sig_cache: &SignatureCache,
    art_cache: &ArtifactCache,
) -> Result<CacheResult> {
    let sig = sig_cache.get_signature(&task.recipe_name, &task.task_name);

    // Check cache
    if art_cache.has_artifact(&task.recipe_name, &task.task_name, &sig) {
        return Ok(CacheResult::CacheHit);
    }

    // Execute (blocks on Command::output(), but that's fine in async context)
    let output = Command::new("bash")
        .arg("-c")
        .arg(&task.script)
        .output()?;  // ← Blocks here

    Ok(CacheResult::Executed)
}
```

**Pros**:
- ✅ **Designed for WebAssembly and WASI 0.2**
- ✅ Async/await for coordinating parallel tasks
- ✅ Lightweight (minimal runtime)
- ✅ Works in Wasm Component environments
- ✅ Block-on function for executing async code
- ✅ Future-proof for Wasm-based builds

**Cons**:
- Temporary solution (until tokio/async-std support Wasm Components)
- Smaller ecosystem than tokio

**Why wstd?**
- Enables async orchestration in WebAssembly environments
- Provides spawn/join semantics for parallel execution
- Allows wave-based task scheduling with async coordination
- Individual tasks can still block on Command::output()

### Option 2: tokio (Traditional)

```rust
#[tokio::main]
async fn main() {
    let tasks = vec![task1, task2, task3];

    let futures: Vec<_> = tasks.iter()
        .map(|task| tokio::spawn(execute_task(task)))
        .collect();

    join_all(futures).await;
}

async fn execute_task(task: &Task) {
    // Still has to use blocking Command::output()!
    let output = Command::new("gcc").output()?;
}
```

**Pros**:
- Modern async/await syntax
- Good for mixed I/O workloads
- Large ecosystem

**Cons**:
- ❌ Adds dependency (tokio is large)
- ❌ Async complexity when we don't need it
- ❌ Still blocks on Command::output() anyway
- ❌ tokio::spawn() spawns OS threads under the hood anyway for blocking tasks

### Option 3: std::thread + channels

```rust
fn main() {
    let (sender, receiver) = mpsc::channel();

    let mut handles = vec![];
    for task in tasks {
        let sender = sender.clone();
        let handle = thread::spawn(move || {
            let result = execute_task(&task);
            sender.send(result).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

fn execute_task(task: &Task) -> TaskResult {
    let output = Command::new("gcc").output()?;
    // ...
}
```

**Pros**:
- ✅ No dependencies (pure std)
- ✅ Simple and explicit
- ✅ Each thread can block naturally

**Cons**:
- More verbose
- Manual thread management
- Need channels for communication

### Option 4: rayon (Alternative for non-Wasm)

```rust
fn main() {
    use rayon::prelude::*;

    let results: Vec<TaskResult> = tasks
        .par_iter()  // ← Parallel iterator
        .map(|task| execute_task(task))
        .collect();
}

fn execute_task(task: &Task) -> TaskResult {
    let output = Command::new("gcc").output()?;
    // ...
}
```

**Or with explicit job submission**:

```rust
fn execute_task_graph(task_graph: &TaskGraph) {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_cpus::get())
        .build()
        .unwrap();

    let mut completed = HashSet::new();

    loop {
        let ready_tasks = task_graph.get_ready_tasks(&completed);
        if ready_tasks.is_empty() { break; }

        // Execute ready tasks in parallel
        let results: Vec<_> = pool.install(|| {
            ready_tasks
                .par_iter()
                .map(|task_id| {
                    let task = &task_graph.tasks[task_id];
                    execute_task(task)
                })
                .collect()
        });

        // Mark completed
        for (task_id, result) in ready_tasks.iter().zip(results) {
            completed.insert(*task_id);
        }
    }
}
```

**Pros**:
- ✅ **Designed for parallel task execution**
- ✅ Work-stealing (efficient load balancing)
- ✅ Simple API (par_iter())
- ✅ Small dependency (~50KB)
- ✅ No async complexity
- ✅ Perfect for CPU-bound + I/O-blocking workloads
- ✅ Thread pool reuses threads (efficient)

**Cons**:
- One small dependency (but worth it)

## Recommendation: Use wstd (WebAssembly Async Runtime)

**Use wstd for async task orchestration** because:

1. **Target environment is WebAssembly/WASI** - wstd is designed for this
2. **Async coordination needed** - Multiple tasks run in parallel waves
3. **Lightweight runtime** - Minimal overhead for Wasm environments
4. **Async/await semantics** - Clean coordination of parallel execution
5. **Blocking tasks are fine** - Command::output() blocks within async context
6. **Future-proof** - Bridge until major runtimes support Wasm Components

**Key insight**: We need async for **orchestrating** parallel tasks, not for the task execution itself. Individual tasks block on process execution (gcc, make), but we use async to coordinate which tasks run in parallel and when dependencies are satisfied.

## Implementation with wstd

### Cargo.toml

```toml
[dependencies]
wstd = "0.1"  # WebAssembly async runtime
futures = "0.3"  # For join_all
```

### Execution Code with Concurrency Limiting

**CRITICAL**: Limit concurrent tasks to avoid overwhelming the system!

```rust
use wstd::runtime;
use futures::future::join_all;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

fn main() -> Result<()> {
    let task_graph = TaskGraphBuilder::build()?;
    let signature_cache = SignatureCache::new()?;
    let artifact_cache = ArtifactCache::new()?;

    // Limit to number of CPU cores (or user-specified)
    let max_parallel = num_cpus::get();

    // Run async execution with wstd runtime
    runtime::block_on(async {
        execute_task_graph(&task_graph, &signature_cache, &artifact_cache, max_parallel).await
    })
}

pub async fn execute_task_graph(
    task_graph: &TaskGraph,
    signature_cache: &SignatureCache,
    artifact_cache: &ArtifactCache,
    max_parallel: usize,
) -> Result<ExecutionStats> {

    // Thread-safe completed set
    let completed = Arc::new(Mutex::new(HashSet::new()));
    let cached = Arc::new(Mutex::new(HashSet::new()));
    let executed = Arc::new(Mutex::new(HashSet::new()));
    let failed = Arc::new(Mutex::new(HashSet::new()));

    loop {
        // Get ready tasks (dependencies satisfied)
        let ready_tasks = {
            let completed = completed.lock().unwrap();
            let failed = failed.lock().unwrap();
            task_graph.get_ready_tasks(&completed)
                .into_iter()
                .filter(|task_id| !failed.contains(task_id))
                .collect::<Vec<_>>()
        };

        if ready_tasks.is_empty() {
            break;  // All done!
        }

        println!("Wave: {} tasks ready, processing in batches of {}", ready_tasks.len(), max_parallel);

        // Process ready tasks in batches to limit concurrency
        let mut queue: VecDeque<_> = ready_tasks.into_iter().collect();

        while !queue.is_empty() {
            // Take up to max_parallel tasks from queue
            let batch_size = queue.len().min(max_parallel);
            let batch: Vec<_> = queue.drain(..batch_size).collect();

            println!("  Batch: {} tasks executing in parallel", batch.len());

            // Spawn parallel async tasks for this batch
            let futures: Vec<_> = batch
                .into_iter()
                .map(|task_id| {
                    let task = task_graph.tasks[&task_id].clone();
                    let sig_cache = signature_cache.clone();
                    let art_cache = artifact_cache.clone();

                    async move {
                        // Check cache
                        let sig = sig_cache.get_signature(&task.recipe_name, &task.task_name);

                        if art_cache.has_artifact(&task.recipe_name, &task.task_name, &sig) {
                            println!("✓ Cache HIT: {}:{}", task.recipe_name, task.task_name);
                            (task_id, CacheResult::CacheHit)
                        } else {
                            println!("⚙ Executing: {}:{}", task.recipe_name, task.task_name);

                            // Execute task (blocks waiting for process, but that's OK in async)
                            match execute_task_in_sandbox(&task, &sig, &art_cache) {
                                Ok(_) => (task_id, CacheResult::Executed),
                                Err(e) => {
                                    eprintln!("❌ Failed: {}:{} - {}", task.recipe_name, task.task_name, e);
                                    (task_id, CacheResult::Failed)
                                }
                            }
                        }
                    }
                })
                .collect();

            // Wait for this batch to complete before starting next batch
            let results = join_all(futures).await;

            // Update completed set
            {
                let mut completed = completed.lock().unwrap();
                let mut cached_set = cached.lock().unwrap();
                let mut executed_set = executed.lock().unwrap();
                let mut failed_set = failed.lock().unwrap();

                for (task_id, result) in results {
                    completed.insert(task_id);

                    match result {
                        CacheResult::CacheHit => { cached_set.insert(task_id); }
                        CacheResult::Executed => { executed_set.insert(task_id); }
                        CacheResult::Failed => { failed_set.insert(task_id); }
                    }
                }
            }
        }
    }

    let failed_count = failed.lock().unwrap().len();
    if failed_count > 0 {
        return Err(format!("{} tasks failed", failed_count).into());
    }

    Ok(ExecutionStats {
        total: task_graph.tasks.len(),
        cached: cached.lock().unwrap().len(),
        executed: executed.lock().unwrap().len(),
    })
}

fn execute_task_in_sandbox(
    task: &ExecutableTask,
    signature: &str,
    artifact_cache: &ArtifactCache,
) -> Result<()> {
    // Create sandbox
    let sandbox = SandboxBuilder::new()
        .with_task(task)
        .with_signature(signature)
        .with_artifact_cache(artifact_cache)
        .build()?;

    // Execute (blocks waiting for gcc/make/etc - this is fine in async context!)
    let output = Command::new("bash")
        .arg("-c")
        .arg(&task.script)
        .current_dir(&sandbox.work_dir)
        .envs(&task.env_vars)
        .output()?;  // ← BLOCKS HERE (but we're in async, so other tasks can run)

    if !output.status.success() {
        return Err(format!("Task failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }

    // Collect outputs
    sandbox.collect_outputs_to_cache(artifact_cache)?;

    Ok(())
}
```

### Why This Works Beautifully

1. **wstd provides async runtime** - Designed for WebAssembly/WASI environments
2. **Async coordination** - Clean wave-based scheduling with async/await
3. **Concurrency limiting** - Process tasks in batches to avoid overwhelming the system
4. **Blocks are fine** - Tasks can block on Command::output() within async context
5. **Parallel execution** - Multiple tasks run concurrently via futures (up to max_parallel)
6. **Dependency awareness** - get_ready_tasks() ensures proper ordering

### Concurrency Limiting Strategy

**Problem**: If 1000 tasks are ready, spawning them all simultaneously would overwhelm the system.

**Solution**: Batch processing with configurable parallelism

```
Wave 1: 1000 tasks ready, processing in batches of 8
  Batch 1: 8 tasks executing in parallel
  Batch 2: 8 tasks executing in parallel
  ... (125 batches total)
  Batch 125: 8 tasks executing in parallel

Wave 2: 500 tasks ready (unblocked after wave 1)
  Batch 1: 8 tasks executing in parallel
  ...
```

**Key points**:
- `max_parallel` defaults to `num_cpus::get()` but can be user-configured
- Each wave processes all ready tasks in batches
- Next wave only starts after current wave completes
- Failed tasks don't block dependent tasks (filtered out)
- System load stays bounded at `max_parallel` concurrent processes

## Comparison: Real Execution

### With wstd (clean async coordination)

```rust
use wstd::runtime;

fn main() {
    runtime::block_on(async {
        let futures: Vec<_> = tasks.iter()
            .map(|task| async move {
                // Execute task - blocks are OK in async context
                Command::new("gcc").output()
            })
            .collect();

        futures::future::join_all(futures).await;
    });
}
```

wstd provides async coordination while allowing blocking operations!

### With tokio (requires spawn_blocking)

```rust
#[tokio::main]
async fn main() {
    let futures: Vec<_> = tasks.iter()
        .map(|task| tokio::spawn(async move {
            // Need spawn_blocking wrapper for blocking operations
            tokio::task::spawn_blocking(|| {
                Command::new("gcc").output()  // Blocks
            }).await
        }))
        .collect();

    join_all(futures).await;
}
```

tokio requires `spawn_blocking()` wrapper for blocking operations.

### With rayon (non-async alternative)

```rust
fn main() {
    let results: Vec<_> = tasks
        .par_iter()
        .map(|task| {
            Command::new("gcc").output()  // Just block naturally
        })
        .collect();
}
```

rayon works great for native environments, but doesn't support WebAssembly.

## Real-World Usage

**wstd is designed for**:
- WebAssembly Components and WASI 0.2
- Async applications before tokio/async-std support Wasm Components
- Lightweight async runtime for Wasm environments
- Bridge solution until major runtimes support Wasm

**Our use case**: Parallel BitBake task execution in WebAssembly environments with async coordination.

## Migration Path

If migrating from tokio or synchronous code:

1. **Add wstd to Cargo.toml**
   ```toml
   [dependencies]
   wstd = "0.1"
   futures = "0.3"
   ```

2. **Replace synchronous main**:
   - `fn main()` → `fn main() { runtime::block_on(async { ... }) }`
   - Add `async` to task execution functions
   - Use `futures::future::join_all()` for parallel execution

3. **Keep blocking operations**:
   - `Command::output()` can block in async context
   - No need for `spawn_blocking()` wrapper
   - Async is for coordination, not I/O

4. **Wave-based scheduling**:
   - Use `get_ready_tasks()` to determine parallelism
   - Create futures for ready tasks
   - `join_all().await` to execute wave
   - Repeat until all tasks complete

**Result**: Async coordination for WebAssembly with clean dependency-aware execution!

## Conclusion

**Use wstd (WebAssembly async runtime)** because:

✅ **Target environment**: WebAssembly Components and WASI 0.2
✅ **Async coordination**: Clean wave-based task scheduling with async/await
✅ **Blocking operations**: Command::output() can block within async context
✅ **Parallel execution**: Multiple independent tasks via futures
✅ **Dependency awareness**: get_ready_tasks() ensures proper ordering
✅ **Lightweight**: Minimal runtime designed for Wasm
✅ **Future-proof**: Bridge until major runtimes support Wasm Components

### Key Design Principle

**We need async for orchestrating parallel tasks, not for I/O operations.**

- Individual BitBake tasks spawn processes (gcc, make) and block on Command::output()
- Async is used to coordinate which tasks run in parallel waves
- wstd provides the async runtime for WebAssembly environments
- get_ready_tasks() determines parallelism based on dependency satisfaction
- Signature-based caching avoids re-executing unchanged tasks

This gives us:
- Clean async/await syntax for coordination
- Parallel execution of independent tasks
- Proper dependency ordering
- Content-addressable caching
- WebAssembly compatibility

**Alternative**: For native-only (non-Wasm) builds, rayon provides excellent parallel task execution without async complexity.
