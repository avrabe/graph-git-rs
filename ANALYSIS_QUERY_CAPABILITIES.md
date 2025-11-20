# Query & Debug Capabilities Analysis

## Current State

### What We Have ✅

**1. Recipe-Level Query System** (`convenient-bitbake/src/query/`)
- **Bazel-inspired query language** with comprehensive AST
- **Query functions**:
  - `deps(target, max_depth)` - Find all dependencies
  - `rdeps(universe, target)` - Reverse dependencies
  - `somepath(from, to)` - Find dependency path
  - `allpaths(from, to)` - Find all paths
  - `kind(pattern, expr)` - Filter by type (e.g., `*-native`)
  - `filter(pattern, expr)` - Filter by name pattern
  - `attr(name, value, expr)` - Filter by attributes
  - Set operations: `intersect`, `union`, `except`

- **Output formats**:
  - `text` - Human-readable
  - `json` - Machine-readable
  - `graph`/`dot` - GraphViz visualization
  - `label` - Recipe names only

- **Target patterns** (Bazel-like):
  - `//...` - All recipes in all layers
  - `meta-core:...` - All recipes in layer
  - `meta-core:busybox` - Specific recipe
  - `meta-core:busybox:do_compile` - Specific task
  - `meta-core:busybox:*` - All tasks for recipe

**2. Build Statistics**
- Task completion tracking
- Cache hit/miss rates
- Execution timing
- Signature computation

**3. Logging Infrastructure**
- tracing-based structured logging
- Execution mode detection
- Sandbox setup/teardown tracking

### What's Missing ❌

**1. Task-Level Query System (tquery/aquery equivalent)**
- Cannot query task dependencies directly
- Cannot ask "what tasks are needed for busybox:install?"
- Cannot analyze task execution graph (vs recipe graph)
- No way to query task-level attributes (execution mode, network policy, etc.)

**2. Bare Recipe Name Support**
- Query requires `layer:recipe` format
- Can't use `deps(busybox, 5)` - must use `deps(meta-core:busybox, 5)`
- No fuzzy matching or auto-discovery

**3. Execution Analysis (aquery)**
- No post-build analysis of what actually executed
- Can't query "show me all tasks that hit cache"
- Can't query "show me all tasks that failed"
- No execution log queries

**4. Debug Output for Failed Tasks**
- Empty stdout/stderr with no indication why
- No "last N lines of failed task" display
- No automatic script dumping on failure
- No sandbox inspection helper

**5. Graph Visualization**
- GraphViz output exists but not tested
- No task graph visualization
- No critical path analysis
- No execution timeline

**6. Live Progress Tracking**
- No real-time task execution display
- No wave-based execution visualization
- No parallel execution monitoring

## Recommended Additions

### Priority 1: Task Query System 🔥

Create `TaskQueryEngine` parallel to `RecipeQueryEngine`:

```rust
// Query task dependencies
hitzeleiter tquery 'deps(busybox:install, 5)'

// Find what tasks depend on a task
hitzeleiter tquery 'rdeps(//..., glibc:populate_sysroot)'

// Show execution path
hitzeleiter tquery 'somepath(busybox:install, linux-libc-headers:install)'

// Filter by execution mode
hitzeleiter tquery 'kind("DirectRust", deps(busybox:install))'

// Show task attributes
hitzeleiter tquery 'attr(network_policy, "FullNetwork", //...)'
```

**Implementation**:
- Add `TaskQueryEngine` in `convenient-bitbake/src/query/task_query.rs`
- Operate on `TaskGraph` instead of `RecipeGraph`
- Support task-specific filters (execution mode, network policy, etc.)
- Add to `hitzeleiter/src/commands/` as `tquery` command

### Priority 2: Smart Recipe Resolution 🎯

Add fuzzy matching and wildcards:

```rust
// Auto-find layer for recipe
deps(busybox, 5)  // searches all layers

// Wildcard layer search
deps(*:busybox, 5)  // explicit wildcard

// Pattern matching
deps(busy*, 3)  // all recipes starting with "busy"
```

**Implementation**:
- Modify `TargetPattern::from_str` to handle single-part patterns
- Add `RecipeGraph::find_recipe_in_all_layers(name: &str)`
- Support glob patterns with `*` and `?`

### Priority 3: Execution Query (aquery) 📊

Bazel's "aquery" equivalent for post-build analysis:

```rust
// Show all executed tasks
hitzeleiter aquery 'executed(//...)'

// Show cache hits
hitzeleiter aquery 'cached(//...)'

// Show failures
hitzeleiter aquery 'failed(//...)'

// Show execution timeline
hitzeleiter aquery 'timeline(busybox:install)' --format gantt
```

**Implementation**:
- Store execution log in build dir (JSON format)
- Add `ExecutionQueryEngine` operating on logs
- Track: task, status, duration, cache hit, outputs

### Priority 4: Enhanced Debug Output 🐛

Better failure diagnostics:

```rust
// Show last failed task details
hitzeleiter debug last-failure

// Dump task script
hitzeleiter debug show-script busybox:configure

// Inspect sandbox
hitzeleiter debug inspect-sandbox <sandbox-id>

// Show task environment
hitzeleiter debug show-env busybox:configure
```

**Implementation**:
- Store last failure info in `.hitzeleiter/last-failure.json`
- Add sandbox persistence option for debugging
- Create `debug` subcommand with multiple actions

### Priority 5: Graph Visualization 📈

Better visual analysis:

```rust
// Generate dependency graph
hitzeleiter query 'deps(busybox:install, 3)' --format dot | dot -Tpng > graph.png

// Show critical path
hitzeleiter tquery 'critical-path(busybox:install)' --format gantt

// Execution timeline
hitzeleiter aquery 'timeline(//...)' --format html > timeline.html
```

**Implementation**:
- Test and fix existing DOT output
- Add task graph DOT generation
- Create HTML timeline generator
- Add critical path algorithm

## Immediate Next Steps for Debugging Current Issue

Given our kern-tools-native:configure failure:

```bash
# 1. Add bare recipe support to query
# 2. Query busybox dependencies
hitzeleiter tquery 'deps(*:busybox:install, 10)' --format text

# 3. Show task details for failed task
hitzeleiter debug show-task kern-tools-native:configure

# 4. Dump the actual script being executed
hitzeleiter debug show-script kern-tools-native:configure

# 5. Check if it's a sandbox issue
hitzeleiter debug test-sandbox --script "echo test"
```

## Architecture Recommendations

### Query Module Structure
```
convenient-bitbake/src/query/
├── mod.rs              # Public API
├── expr.rs             # AST types (extend for tasks)
├── parser.rs           # Parser (extend for wildcards)
├── recipe_query.rs     # Recipe queries (existing)
├── task_query.rs       # NEW: Task queries
├── exec_query.rs       # NEW: Execution log queries
└── output.rs           # Output formatting (extend)
```

### Debug Module Structure
```
hitzeleiter/src/commands/debug/
├── mod.rs              # Debug command dispatcher
├── last_failure.rs     # Show last failure
├── show_script.rs      # Dump task scripts
├── inspect_sandbox.rs  # Sandbox inspection
├── show_env.rs         # Environment display
└── test_sandbox.rs     # Sandbox testing
```

### Execution Log Format
```json
{
  "build_id": "uuid",
  "timestamp": "2025-11-20T05:29:50Z",
  "tasks": [
    {
      "id": "busybox:configure",
      "status": "success",
      "duration_ms": 1234,
      "cache_hit": false,
      "execution_mode": "Shell",
      "outputs": ["configure.done"],
      "dependencies": ["busybox:patch"]
    }
  ]
}
```

## Benefits

1. **Faster debugging**: Immediately see what's failing and why
2. **Better understanding**: Visualize dependency chains
3. **Cache optimization**: Identify cache misses
4. **Build optimization**: Find critical path, parallelize better
5. **Reproducibility**: Query past builds, compare executions
6. **CI/CD integration**: Machine-readable output for automation

## Effort Estimate

| Feature | Effort | Value | Priority |
|---------|--------|-------|----------|
| Task query system | 2-3 days | High | P1 |
| Smart recipe resolution | 1 day | High | P1 |
| Debug commands | 2 days | High | P1 |
| Execution query | 2-3 days | Medium | P2 |
| Graph visualization | 3-4 days | Medium | P2 |

**Total for P1**: ~5-6 days of focused work
**ROI**: Massive - will pay for itself immediately in debugging time saved
