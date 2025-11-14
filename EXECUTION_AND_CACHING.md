# Task Execution Scheduling and Caching Strategy

## The Core Question

**How do we know:**
1. Which tasks can run in parallel?
2. When to unblock tasks (dependency satisfaction)?
3. Whether to use cached outputs or re-execute?
4. Is this part of async executor or explicit scheduling?

## Answer: It's a Combination of Both

The execution strategy has **two layers**:

1. **TaskGraph** - Static dependency analysis (we build this upfront)
2. **Async Executor** - Dynamic scheduling with cache checking

## Part 1: Static Dependency Graph (TaskGraph)

We **already have this** in `task_graph.rs`:

```rust
pub struct ExecutableTask {
    pub task_id: TaskId,
    pub recipe_name: String,
    pub task_name: String,
    pub depends_on: Vec<TaskId>,      // ← Explicit dependencies
    pub dependents: Vec<TaskId>,       // ← Who depends on us
}

pub struct TaskGraph {
    pub tasks: HashMap<TaskId, ExecutableTask>,
    pub execution_order: Vec<TaskId>,  // ← Topological sort
    pub root_tasks: Vec<TaskId>,       // ← No dependencies
}
```

### Key Method: `get_ready_tasks()`

```rust
pub fn get_ready_tasks(&self, completed: &HashSet<TaskId>) -> Vec<TaskId> {
    self.tasks
        .iter()
        .filter(|(task_id, task)| {
            !completed.contains(task_id)                         // Not done yet
                && task.depends_on.iter().all(|dep| completed.contains(dep))  // All deps done
        })
        .map(|(task_id, _)| *task_id)
        .collect()
}
```

**This tells us which tasks can run NOW** based on completed dependencies.

## Part 2: Task Signatures (SignatureCache)

We **already have this** in `signature_cache.rs`:

```rust
pub struct EnhancedTaskSignature {
    pub recipe: String,
    pub task: String,
    pub recipe_hash: String,           // ← Recipe content
    pub task_code_hash: String,        // ← Task implementation
    pub dep_signatures: Vec<String>,   // ← Dependency signatures
    pub env_vars: HashMap<String, String>,
    pub machine: Option<String>,
    pub distro: Option<String>,
    pub signature: Option<String>,     // ← Final hash
}
```

### How Signature is Computed

```rust
signature = SHA256(
    recipe_name
    + task_name
    + recipe_hash
    + task_code_hash
    + dep_signatures (sorted)     // ← CRITICAL: includes dep outputs!
    + env_vars (sorted)
    + machine
    + distro
)
```

**Key insight**: Signature includes `dep_signatures`, so if ANY dependency changes, our signature changes!

## Part 3: Cache Checking (What We Need to Add)

Before executing a task, check if outputs exist:

```rust
pub struct ArtifactCache {
    cache_dir: PathBuf,  // e.g., ~/.bitzel-cache/artifacts/
}

impl ArtifactCache {
    /// Check if task outputs exist for this signature
    pub fn has_artifact(&self, recipe: &str, task: &str, signature: &str) -> bool {
        let artifact_dir = self.cache_dir
            .join(recipe)
            .join(format!("{}-{}", task, signature));

        // Check if artifact directory exists and has expected outputs
        artifact_dir.exists() &&
            artifact_dir.join("metadata.json").exists() &&
            artifact_dir.join("build").exists()
    }

    /// Get path to cached artifact
    pub fn artifact_path(&self, recipe: &str, task: &str, signature: &str) -> PathBuf {
        self.cache_dir
            .join(recipe)
            .join(format!("{}-{}", task, signature))
    }
}
```

## Part 4: Execution Flow (Combined Strategy)

Here's how it all works together:

### Step 1: Build Task Graph (Static)

```rust
// Parse recipes and build dependency graph
let task_graph = TaskGraphBuilder::build(&recipe_graph)?;

// This gives us:
// - All tasks with explicit dependencies
// - Topological ordering
// - Root tasks (no dependencies)

println!("Total tasks: {}", task_graph.tasks.len());
println!("Root tasks (can start immediately): {}", task_graph.root_tasks.len());
```

### Step 2: Compute Task Signatures (Static)

```rust
let mut signature_cache = SignatureCache::new(cache_dir);

// Compute signatures in topological order
for task_id in &task_graph.execution_order {
    let task = &task_graph.tasks[task_id];

    // Get dependency signatures (already computed)
    let dep_sigs: Vec<String> = task.depends_on
        .iter()
        .map(|dep_id| {
            let dep_task = &task_graph.tasks[dep_id];
            signature_cache.get_signature(&dep_task.recipe_name, &dep_task.task_name)
        })
        .collect();

    // Compute this task's signature
    let sig = EnhancedTaskSignature::new(
        task.recipe_name.clone(),
        task.task_name.clone(),
        recipe_hash,
        task_code,
        dep_sigs,        // ← Includes dependency outputs!
        env_vars,
        machine,
        distro,
    );

    signature_cache.store(sig);
}
```

**After this step**, we know the signature for every task, even if we haven't executed anything yet!

### Step 3: Execute with Cache Checking (Dynamic)

```rust
pub async fn execute_tasks(
    task_graph: &TaskGraph,
    signature_cache: &SignatureCache,
    artifact_cache: &ArtifactCache,
) -> Result<ExecutionStats> {

    let mut completed: HashSet<TaskId> = HashSet::new();
    let mut cached: HashSet<TaskId> = HashSet::new();
    let mut executed: HashSet<TaskId> = HashSet::new();

    loop {
        // Get tasks that are ready to run (dependencies satisfied)
        let ready_tasks = task_graph.get_ready_tasks(&completed);

        if ready_tasks.is_empty() {
            break;  // All done!
        }

        println!("Ready to run: {} tasks", ready_tasks.len());

        // Execute ready tasks IN PARALLEL
        let futures: Vec<_> = ready_tasks
            .iter()
            .map(|&task_id| {
                let task = &task_graph.tasks[&task_id];
                execute_or_fetch_cached(task, signature_cache, artifact_cache)
            })
            .collect();

        // Wait for all parallel tasks to complete
        let results = futures::future::join_all(futures).await;

        // Mark completed tasks
        for (task_id, result) in ready_tasks.iter().zip(results) {
            completed.insert(*task_id);

            match result {
                CacheResult::CacheHit => cached.insert(*task_id),
                CacheResult::Executed => executed.insert(*task_id),
            };
        }
    }

    Ok(ExecutionStats {
        total: task_graph.tasks.len(),
        cached: cached.len(),
        executed: executed.len(),
    })
}
```

### Step 4: Execute or Fetch (Per Task)

```rust
async fn execute_or_fetch_cached(
    task: &ExecutableTask,
    signature_cache: &SignatureCache,
    artifact_cache: &ArtifactCache,
) -> CacheResult {

    // Get task signature
    let sig = signature_cache.get_signature(&task.recipe_name, &task.task_name);

    // CHECK CACHE FIRST
    if artifact_cache.has_artifact(&task.recipe_name, &task.task_name, sig) {
        println!("✓ Cache HIT: {}:{} ({})", task.recipe_name, task.task_name, &sig[..8]);
        return CacheResult::CacheHit;
    }

    println!("⚙ Executing: {}:{} ({})", task.recipe_name, task.task_name, &sig[..8]);

    // EXECUTE TASK
    let sandbox = create_sandbox(task, sig, artifact_cache)?;

    sandbox.execute()?;

    // COLLECT OUTPUTS to artifact cache
    let outputs = sandbox.collect_outputs()?;

    artifact_cache.store(
        &task.recipe_name,
        &task.task_name,
        sig,
        outputs
    )?;

    CacheResult::Executed
}
```

## Complete Example: busybox Execution

Let's trace how busybox tasks execute:

### Initial State

```
Task Graph (3 tasks):
  busybox:do_prepare_config (task_id=1)
    depends_on: [busybox:do_patch]
  busybox:do_configure (task_id=2)
    depends_on: [busybox:do_prepare_config]
  busybox:do_compile (task_id=3)
    depends_on: [busybox:do_configure, libxcrypt:do_install, gcc-cross:do_install]

Completed: {}
```

### Iteration 1

```rust
ready_tasks = task_graph.get_ready_tasks(&completed);
// Returns: [] (nothing ready yet - dependencies not satisfied)
```

Wait, we need `busybox:do_patch` first! Let's say it completed:

```
Completed: {busybox:do_patch}
```

### Iteration 2

```rust
ready_tasks = task_graph.get_ready_tasks(&completed);
// Returns: [busybox:do_prepare_config]

// Check cache
sig = signature_cache.get("busybox", "do_prepare_config");
// sig = "a4f3e21..."

if artifact_cache.has_artifact("busybox", "do_prepare_config", sig) {
    // CACHE HIT! Just use existing outputs
    println!("✓ Cache HIT: busybox:do_prepare_config");
    completed.insert(busybox:do_prepare_config);
} else {
    // Execute the task
    execute_task(...);
    completed.insert(busybox:do_prepare_config);
}
```

### Iteration 3

```
Completed: {busybox:do_patch, busybox:do_prepare_config}

ready_tasks = task_graph.get_ready_tasks(&completed);
// Returns: [busybox:do_configure]

// Execute or fetch from cache...
```

### Iteration 4

```
Completed: {busybox:do_patch, busybox:do_prepare_config, busybox:do_configure,
            libxcrypt:do_install, gcc-cross:do_install}

ready_tasks = task_graph.get_ready_tasks(&completed);
// Returns: [busybox:do_compile]

// Now ALL dependencies are satisfied!
```

## Parallel Execution Example

Consider this dependency graph:

```
        busybox:do_fetch       glibc:do_fetch       zlib:do_fetch
              ↓                      ↓                    ↓
        busybox:do_unpack      glibc:do_unpack      zlib:do_unpack
              ↓                      ↓                    ↓
        busybox:do_patch       glibc:do_patch       zlib:do_patch
              ↓                      ↓                    ↓
                          glibc:do_compile       zlib:do_compile
                                 ↓                    ↓
                          glibc:do_install       zlib:do_install
                                      ↘             ↙
                                    busybox:do_compile
```

### Execution Timeline

**Wave 1**: (parallel)
```rust
ready_tasks = [busybox:do_fetch, glibc:do_fetch, zlib:do_fetch]

// Execute ALL THREE in parallel
tokio::spawn(execute_task(busybox:do_fetch))
tokio::spawn(execute_task(glibc:do_fetch))
tokio::spawn(execute_task(zlib:do_fetch))

await all...
```

**Wave 2**: (parallel)
```rust
ready_tasks = [busybox:do_unpack, glibc:do_unpack, zlib:do_unpack]
// Execute in parallel again
```

**Wave 3**: (parallel)
```rust
ready_tasks = [busybox:do_patch, glibc:do_patch, zlib:do_patch]
// Parallel!
```

**Wave 4**: (parallel - busybox waits)
```rust
ready_tasks = [glibc:do_compile, zlib:do_compile]
// busybox:do_compile NOT ready yet (needs glibc+zlib outputs)
// But glibc and zlib can compile in parallel!
```

**Wave 5**: (parallel)
```rust
ready_tasks = [glibc:do_install, zlib:do_install]
// Parallel!
```

**Wave 6**: (finally!)
```rust
ready_tasks = [busybox:do_compile]
// NOW busybox can compile with glibc+zlib sysroots
```

## Cache Invalidation

### Scenario 1: Recipe Changed

```rust
// Before
glibc.bb: CFLAGS = "-O2"
signature = SHA256(recipe_hash=abc123 + ...)
            = "xyz789"

// After changing recipe
glibc.bb: CFLAGS = "-O3"  // ← Changed!
signature = SHA256(recipe_hash=def456 + ...)  // Different recipe_hash
            = "qrs456"  // ← Different signature!

// Cache check
artifact_cache.has_artifact("glibc", "do_compile", "qrs456")
// → false (no artifact with this signature)

// MUST re-execute!
```

### Scenario 2: Dependency Changed

```rust
// glibc:do_compile changed (signature now "new123")

// busybox:do_compile signature computation:
signature = SHA256(
    ...
    + dep_signatures: ["new123", ...]  // ← glibc changed!
)
= "different789"  // ← OUR signature also changed!

// Cache check
artifact_cache.has_artifact("busybox", "do_compile", "different789")
// → false

// MUST re-execute busybox too!
```

**This is transitive dependency propagation!**

### Scenario 3: Nothing Changed

```bash
# Second build
$ bitzel build busybox

# All signatures same as before
✓ Cache HIT: glibc:do_compile (xyz789)
✓ Cache HIT: zlib:do_compile (abc456)
✓ Cache HIT: busybox:do_compile (qrs789)

Build completed in 0.5s (0 tasks executed, 3 from cache)
```

## Who's Responsible for What?

| Component | Responsibility |
|-----------|---------------|
| **TaskGraph** | Static dependency structure, topological ordering |
| **TaskGraph::get_ready_tasks()** | Determine which tasks CAN run now |
| **SignatureCache** | Compute content-addressable signatures |
| **ArtifactCache** | Check if outputs exist for signature |
| **Async Executor** | Dynamic scheduling, parallel execution |
| **SandboxBuilder** | Create sandbox with dependency outputs |

## Summary

### Q: How do we know which tasks can run in parallel?

**A**: `TaskGraph::get_ready_tasks()` returns all tasks whose dependencies are satisfied. Execute them all in parallel with `tokio::spawn()`.

### Q: When do we unblock tasks?

**A**: After each wave of parallel tasks completes, call `get_ready_tasks()` again with updated `completed` set. Newly unblocked tasks run in next wave.

### Q: How do we know to use cached vs re-execute?

**A**: Before executing, check `artifact_cache.has_artifact(recipe, task, signature)`. If true, skip execution and use existing outputs.

### Q: Is this async executor's job or explicit logic?

**A**: **Both!**
- TaskGraph provides dependency info (explicit)
- Async executor (tokio) handles parallel execution (implicit)
- Cache checking is explicit (before task execution)
- Dependency unblocking is explicit (`get_ready_tasks()`)

This is exactly how Bazel works:
1. Build action graph (static)
2. Compute action keys (signatures)
3. Execute with cache checking (dynamic)
4. Parallel execution of independent actions

We have all the pieces - just need to wire them together!
