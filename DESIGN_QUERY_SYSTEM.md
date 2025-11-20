# Task Query System Design (Bazel-Inspired)

## Bazel's Three-Level Query System

Bazel provides three complementary query commands:

### 1. `bazel query` - Build Graph (Labels)
- Operates on **unconfigured** build graph
- Shows target relationships (deps, rdeps, etc.)
- Fast - doesn't need to analyze build configuration
- **Our equivalent**: `hitzeleiter query` ✅ (already implemented)

### 2. `bazel cquery` - Configured Graph
- Operates on **configured** targets (after configuration resolution)
- Shows actual build dependencies with specific configurations
- Answers "what will actually be built?"
- **Our equivalent**: `hitzeleiter tquery` ❌ (NEW - task-level queries)

### 3. `bazel aquery` - Action Graph
- Operates on **actions** (actual commands executed)
- Shows command lines, inputs, outputs
- Answers "what commands ran?" and "why did this fail?"
- **Our equivalent**: `hitzeleiter aquery` ❌ (NEW - execution analysis)

## Our Implementation Plan

### Phase 1: Task Query (tquery) - Core Infrastructure

**Purpose**: Query the task execution graph (like Bazel's cquery)

**Data Source**: `TaskGraph` from `BuildOrchestrator`

**Query Functions** (extending existing QueryExpr):

```rust
// Basic queries (reuse existing syntax)
tquery 'deps(busybox:install, 5)'           // Task dependencies
tquery 'rdeps(//..., glibc:populate_sysroot)' // What depends on this task
tquery 'somepath(busybox:install, linux-libc-headers:install)'

// Task-specific queries (NEW)
tquery 'kind("DirectRust", deps(busybox:install))' // Filter by execution mode
tquery 'attr(network, "Isolated", //...)'   // Filter by network policy
tquery 'script(busybox:configure)'          // Show task script
tquery 'inputs(busybox:compile)'            // Show task inputs
tquery 'outputs(busybox:install)'           // Show task outputs
tquery 'env(busybox:configure)'             // Show environment variables

// Analysis queries
tquery 'critical-path(busybox:install)'     // Show critical path
tquery 'parallel-depth(//...)'              // Show parallelization opportunities
```

**Output Formats**:
- `--format text` - Human-readable (default)
- `--format json` - Machine-readable
- `--format dot` - GraphViz visualization
- `--format script` - Just the script (for script() queries)
- `--format list` - Simple task list

**Implementation**:
```
convenient-bitbake/src/query/
├── task_query.rs          # TaskQueryEngine
├── expr.rs                # Extend QueryExpr with task-specific nodes
├── parser.rs              # Extend parser for new functions
└── output.rs              # Extend formatters

hitzeleiter/src/commands/
└── tquery.rs              # CLI command
```

### Phase 2: Action Query (aquery) - Execution Analysis

**Purpose**: Query execution history and debug failures

**Data Source**: Execution log (stored in `.hitzeleiter/exec-log.json`)

**Query Functions**:

```rust
// Execution status
aquery 'executed(//...)'               // All executed tasks
aquery 'cached(//...)'                 // Cache hits
aquery 'failed(//...)'                 // Failures
aquery 'pending(//...)'                // Not yet executed

// Analysis
aquery 'timeline(busybox:install)'     // Execution timeline
aquery 'duration(//...) > 1000'        // Tasks >1s
aquery 'last-failure()'                // Last failed task details

// Debug output
aquery 'stdout(kern-tools-native:configure)'  // Show stdout
aquery 'stderr(kern-tools-native:configure)'  // Show stderr
aquery 'exit-code(//...)'              // Show exit codes
```

**Execution Log Format**:
```json
{
  "version": "1.0",
  "build_id": "uuid",
  "start_time": "2025-11-20T05:29:50Z",
  "end_time": "2025-11-20T05:30:15Z",
  "tasks": [
    {
      "id": "busybox:configure",
      "recipe": "busybox",
      "task": "configure",
      "status": "success",
      "duration_ms": 1234,
      "cache_hit": false,
      "execution_mode": "Shell",
      "network_policy": "Isolated",
      "outputs": ["/work/outputs/configure.done"],
      "stdout": "NOTE: [PLACEHOLDER] configure\n",
      "stderr": "",
      "exit_code": 0,
      "dependencies": ["busybox:patch"],
      "start_time": "2025-11-20T05:29:51Z",
      "end_time": "2025-11-20T05:29:52Z"
    }
  ]
}
```

### Phase 3: Enhanced Output & Visualization

**GraphViz Task Graph**:
```bash
# Generate task dependency graph
hitzeleiter tquery 'deps(busybox:install, 3)' --format dot > task-graph.dot
dot -Tpng task-graph.dot -o task-graph.png

# Critical path visualization
hitzeleiter tquery 'critical-path(busybox:install)' --format dot > critical.dot
```

**Timeline Visualization**:
```bash
# HTML Gantt chart of execution
hitzeleiter aquery 'timeline(//...)' --format html > timeline.html

# CSV for analysis
hitzeleiter aquery 'duration(//...)' --format csv > timings.csv
```

## Extended Query Expression AST

```rust
pub enum QueryExpr {
    // Existing (from recipe query)
    Target(TargetPattern),
    Deps { expr, max_depth },
    ReverseDeps { universe, target },
    SomePath { from, to },
    AllPaths { from, to },
    Kind { pattern, expr },
    Filter { pattern, expr },
    Attr { name, value, expr },
    Intersect(Box, Box),
    Union(Box, Box),
    Except(Box, Box),

    // NEW: Task-specific queries
    Script(Box<QueryExpr>),              // Show script content
    Inputs(Box<QueryExpr>),              // Show task inputs
    Outputs(Box<QueryExpr>),             // Show task outputs
    Env(Box<QueryExpr>),                 // Show environment
    CriticalPath(Box<QueryExpr>),        // Critical path analysis

    // NEW: Execution queries (aquery)
    Executed(Box<QueryExpr>),            // Filter by executed
    Cached(Box<QueryExpr>),              // Filter by cached
    Failed(Box<QueryExpr>),              // Filter by failed
    Duration { expr, operator, millis }, // Filter by duration
    ExitCode { expr, code },             // Filter by exit code
    Stdout(Box<QueryExpr>),              // Show stdout
    Stderr(Box<QueryExpr>),              // Show stderr
    Timeline(Box<QueryExpr>),            // Show execution timeline
    LastFailure,                         // Special: last failure
}
```

## Target Pattern Extensions

Current patterns work for tasks already:
```
//...                          # All tasks in all layers
meta-core:...                  # All tasks in layer
meta-core:busybox              # All tasks for recipe
meta-core:busybox:configure    # Specific task
meta-core:busybox:*            # Explicit all-tasks wildcard
```

Add wildcard support:
```
*:busybox                      # Find busybox in any layer
*:busybox:configure            # Find busybox:configure in any layer
*:*:configure                  # All configure tasks
busy*:*                        # All tasks for recipes starting with "busy"
```

## Usage Examples

### Immediate Debugging Needs

```bash
# 1. Find all tasks needed for busybox:install
hitzeleiter tquery 'deps(*:busybox:install, 100)'

# 2. Show the script that's failing
hitzeleiter tquery 'script(*:kern-tools-native:configure)' --format script

# 3. Find why it failed
hitzeleiter aquery 'last-failure()'

# 4. Show all failed tasks
hitzeleiter aquery 'failed(//...)'

# 5. Critical path to busybox
hitzeleiter tquery 'critical-path(*:busybox:install)' --format dot | dot -Tpng > path.png
```

### Advanced Analysis

```bash
# Find all tasks using Shell execution mode
hitzeleiter tquery 'kind("Shell", //...)'

# Find slow tasks (>5s)
hitzeleiter aquery 'duration(//...) > 5000' --format csv

# Show execution timeline
hitzeleiter aquery 'timeline(//...)' --format html > timeline.html

# Find tasks with network access
hitzeleiter tquery 'attr(network, "FullNetwork", //...)'

# Compare two builds
hitzeleiter aquery 'failed(//...)' --build-id abc123
hitzeleiter aquery 'failed(//...)' --build-id def456
```

### CI/CD Integration

```bash
# Machine-readable output for CI
hitzeleiter aquery 'failed(//...)' --format json | jq -r '.[].task'

# Track cache hit rate over time
hitzeleiter aquery 'cached(//...)' --format json | \
  jq '{total: length, cached: [.[] | select(.cache_hit)] | length}'

# Generate build report
hitzeleiter aquery 'timeline(//...)' --format json > build-report.json
```

## Implementation Strategy

### Week 1: Core tquery

**Day 1-2: Infrastructure**
- [ ] Extend `QueryExpr` with task-specific nodes
- [ ] Extend `QueryParser` to parse new functions
- [ ] Create `TaskQueryEngine` (parallel to `RecipeQueryEngine`)
- [ ] Add wildcard pattern matching (`*:busybox`)

**Day 3: Basic Queries**
- [ ] Implement `deps()` on TaskGraph
- [ ] Implement `rdeps()` on TaskGraph
- [ ] Implement `script()` query
- [ ] Add `hitzeleiter tquery` command

**Day 4: Advanced Queries**
- [ ] Implement `kind()` (filter by execution mode)
- [ ] Implement `attr()` (filter by task attributes)
- [ ] Implement `inputs()` and `outputs()`
- [ ] Implement `env()` query

**Day 5: Testing & Docs**
- [ ] Integration tests
- [ ] Documentation
- [ ] Examples

### Week 2: aquery & Visualization

**Day 1-2: Execution Logging**
- [ ] Design execution log schema
- [ ] Implement log writer in TaskExecutor
- [ ] Add log reader

**Day 3-4: aquery Implementation**
- [ ] Create `ExecutionQueryEngine`
- [ ] Implement status filters (executed, cached, failed)
- [ ] Implement debug queries (stdout, stderr, exit-code)
- [ ] Add `hitzeleiter aquery` command

**Day 5: Visualization**
- [ ] Fix DOT output for task graphs
- [ ] Add HTML timeline generator
- [ ] Add critical path analysis

## Benefits

1. **Unified Query Language**: Single syntax for all queries
2. **Composability**: Combine queries with set operations
3. **Extensibility**: Easy to add new query functions
4. **Bazel Compatibility**: Familiar to Bazel users
5. **CI/CD Ready**: Machine-readable output for automation
6. **Incremental**: Build core first, extend as needed

## Next Steps

1. **Immediate**: Implement core tquery for debugging busybox
2. **This Week**: Complete tquery with script(), deps(), kind()
3. **Next Week**: Add aquery for failure analysis
4. **Ongoing**: Extend with new query functions as needs arise

---

**Decision**: Start with tquery implementation focusing on:
- Task dependency queries (deps, rdeps)
- Script inspection (script)
- Execution mode filtering (kind)
- Wildcard pattern matching (*:busybox)

This gives us everything needed to debug the current issue while building proper infrastructure.
