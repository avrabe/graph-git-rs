# Bitzel Architecture Refactoring Proposal

## Current State Analysis

### Current Crate Structure
```
graph-git-rs/
├── convenient-bitbake/     # BitBake parsing + Bitzel executor
├── convenient-kas/         # Kas YAML + repository management
├── convenient-git/         # Git operations
├── convenient-repo/        # Repository utilities
├── graph-git/              # Graph database (Neo4j)
├── graph-git-cli/          # CLI tool
└── accuracy-measurement/   # Metrics
```

### Problems with Current Structure

1. **Mixed Concerns**: `convenient-bitbake` contains both:
   - BitBake parsing (library functionality)
   - Bitzel execution engine (application functionality)

2. **Unclear Dependencies**: The "application" (Bitzel) is embedded in library crates

3. **Graph Technology Duplication**: Graph concepts (DAGs, dependencies) appear in:
   - `convenient-bitbake/src/recipe_graph.rs`
   - `convenient-bitbake/src/task_graph.rs`
   - `convenient-kas/src/include_graph.rs`
   - `graph-git/` (Neo4j-specific)

4. **Versioning Complexity**: Can't version Bitzel independently from parsing libraries

## Proposed Architecture

### New Crate Structure

```
graph-git-rs/
├── bitzel/                          # NEW: Bitzel application
│   ├── src/
│   │   ├── main.rs                  # CLI entry point
│   │   ├── bootstrap.rs             # Kas → Bitzel bootstrap
│   │   ├── build.rs                 # Build orchestration
│   │   └── config.rs                # Bitzel configuration
│   └── Cargo.toml
│
├── convenient-graph/                # NEW: Generic graph library
│   ├── src/
│   │   ├── dag.rs                   # Generic DAG
│   │   ├── topological.rs           # Topological sorting
│   │   ├── dependency.rs            # Dependency resolution
│   │   ├── cache.rs                 # Graph-based caching
│   │   └── visualize.rs             # Graph visualization
│   └── Cargo.toml
│
├── convenient-bitbake/              # REFACTORED: Pure parsing library
│   ├── src/
│   │   ├── parser/                  # BitBake parsing only
│   │   ├── resolver/                # Recipe resolution
│   │   └── types.rs                 # BitBake types
│   └── Cargo.toml
│
├── bitzel-executor/                 # NEW: Execution engine library
│   ├── src/
│   │   ├── executor.rs              # Task execution
│   │   ├── sandbox.rs               # Sandboxing
│   │   ├── cache.rs                 # Local cache
│   │   ├── remote_cache.rs          # Bazel Remote Execution API
│   │   └── monitor.rs               # Execution monitoring
│   └── Cargo.toml
│
├── convenient-kas/                  # ENHANCED: Use convenient-graph
│   ├── src/
│   │   ├── parser.rs                # Kas YAML parsing
│   │   ├── include_graph.rs         # Uses convenient-graph
│   │   └── repository_manager.rs
│   └── Cargo.toml
│
├── convenient-git/                  # As-is: Git operations
├── convenient-repo/                 # As-is: Repository utilities
│
├── graph-git/                       # REFACTORED: Neo4j adapter
│   ├── src/
│   │   ├── adapter.rs               # Adapt convenient-graph to Neo4j
│   │   └── queries.rs               # Neo4j-specific queries
│   └── Cargo.toml
│
└── graph-git-cli/                   # As-is: Graph database CLI
```

### Dependency Flow

```
┌─────────────────────────────────────────────────────┐
│                      bitzel                         │
│              (Main Application Crate)               │
└────────────┬────────────────────────────────────────┘
             │
             ├───────────────────────┬──────────────────┬─────────────────┐
             │                       │                  │                 │
             ▼                       ▼                  ▼                 ▼
    ┌─────────────────┐    ┌─────────────────┐  ┌──────────────┐  ┌──────────────┐
    │ bitzel-executor │    │ convenient-kas  │  │ convenient-  │  │ convenient-  │
    │                 │    │                 │  │   bitbake    │  │     git      │
    └────────┬────────┘    └────────┬────────┘  └──────┬───────┘  └──────────────┘
             │                      │                   │
             │                      └───────┬───────────┘
             │                              │
             ▼                              ▼
    ┌─────────────────────────────────────────────┐
    │          convenient-graph                   │
    │     (Generic Graph Library)                 │
    └─────────────────────────────────────────────┘
```

## Migration Plan

### Phase 1: Extract Generic Graph Library

**Create `convenient-graph` crate:**

```rust
// convenient-graph/src/lib.rs

/// Generic directed acyclic graph
pub struct DAG<N, E> {
    nodes: HashMap<NodeId, Node<N>>,
    edges: HashMap<NodeId, Vec<Edge<E>>>,
}

impl<N, E> DAG<N, E> {
    /// Add node to graph
    pub fn add_node(&mut self, data: N) -> NodeId;

    /// Add edge between nodes
    pub fn add_edge(&mut self, from: NodeId, to: NodeId, data: E) -> Result<(), CycleError>;

    /// Topological sort (Kahn's algorithm)
    pub fn topological_sort(&self) -> Result<Vec<NodeId>, CycleError>;

    /// Find dependencies of a node
    pub fn dependencies(&self, node: NodeId) -> Vec<NodeId>;

    /// Find dependents of a node
    pub fn dependents(&self, node: NodeId) -> Vec<NodeId>;

    /// Detect cycles
    pub fn find_cycles(&self) -> Vec<Vec<NodeId>>;

    /// Calculate checksums for cache invalidation
    pub fn content_hash<H: Hasher>(&self, node: NodeId) -> ContentHash;
}

/// Graph-based caching
pub struct GraphCache<N, E> {
    graph: DAG<N, E>,
    cache: HashMap<NodeId, CachedResult>,
}

impl<N, E> GraphCache<N, E> {
    /// Invalidate node and all dependents
    pub fn invalidate(&mut self, node: NodeId);

    /// Check if node is cached and valid
    pub fn is_valid(&self, node: NodeId) -> bool;
}
```

**Migration:**
1. Extract common code from:
   - `convenient-bitbake/src/recipe_graph.rs` → `convenient-graph/src/recipe_dag.rs`
   - `convenient-bitbake/src/task_graph.rs` → `convenient-graph/src/task_dag.rs`
   - `convenient-kas/src/include_graph.rs` → Use `convenient-graph::DAG`

2. Specialize with type parameters:
   ```rust
   // In convenient-bitbake
   pub type RecipeGraph = DAG<RecipeNode, RecipeDependency>;
   pub type TaskGraph = DAG<TaskNode, TaskDependency>;

   // In convenient-kas
   pub type IncludeGraph = DAG<KasFile, IncludeDependency>;
   ```

### Phase 2: Extract Bitzel Executor

**Create `bitzel-executor` library:**

Move from `convenient-bitbake/src/executor/` → `bitzel-executor/src/`:
- `executor.rs`
- `sandbox.rs`
- `cache.rs` (renamed to `cache_manager.rs`)
- `remote_cache.rs`
- `monitor.rs`
- `types.rs`

**Benefits:**
- Clean separation: parsing vs. execution
- Independent versioning
- Easier to test execution without parsing
- Can reuse executor for other build systems

### Phase 3: Create Bitzel Application Crate

**Create `bitzel` main application:**

```rust
// bitzel/src/main.rs
use bitzel_executor::Executor;
use convenient_bitbake::Parser;
use convenient_kas::KasBootstrap;

#[tokio::main]
async fn main() -> Result<()> {
    // Bootstrap from kas
    let kas_config = KasBootstrap::load("kas.yml").await?;
    let repos = kas_config.setup_repositories().await?;
    let config = kas_config.generate_bitbake_config().await?;

    // Parse recipes
    let parser = Parser::new(&config);
    let recipes = parser.discover_recipes()?;
    let recipe_graph = parser.build_recipe_graph(recipes)?;

    // Execute build
    let executor = Executor::new(&config);
    executor.build_graph(&recipe_graph).await?;

    Ok(())
}

// bitzel/src/bootstrap.rs
pub struct BitzelBootstrap {
    kas: KasBootstrap,
    executor_config: ExecutorConfig,
}

impl BitzelBootstrap {
    pub async fn from_kas(path: impl AsRef<Path>) -> Result<Self>;
    pub async fn setup(&self) -> Result<BitzelContext>;
}

// bitzel/src/build.rs
pub struct BitzelBuilder {
    context: BitzelContext,
    executor: Executor,
}

impl BitzelBuilder {
    pub async fn build(&self, target: &str) -> Result<BuildResult>;
    pub async fn clean(&self) -> Result<()>;
}
```

### Phase 4: Refactor Graph-Git to Use Convenient-Graph

**Create adapter pattern:**

```rust
// graph-git/src/adapter.rs
use convenient_graph::DAG;
use neo4rs::Graph;

/// Adapter to sync convenient-graph DAG to Neo4j
pub struct Neo4jAdapter {
    graph: Graph,
}

impl Neo4jAdapter {
    /// Sync DAG to Neo4j
    pub async fn sync<N, E>(&self, dag: &DAG<N, E>) -> Result<()>
    where
        N: Serialize,
        E: Serialize,
    {
        // Convert DAG to Neo4j nodes and relationships
    }

    /// Query using Cypher and return as DAG
    pub async fn query_to_dag(&self, cypher: &str) -> Result<DAG<JsonValue, JsonValue>>;
}
```

## Benefits of This Architecture

### 1. Separation of Concerns
- **Libraries**: Pure, reusable functionality
- **Application**: Integration and orchestration
- **Adapters**: Bridge between libraries and specific implementations

### 2. Reusability
- `convenient-graph` can be used for:
  - Recipe dependencies
  - Task dependencies
  - Include dependencies
  - Package dependencies
  - Any DAG-based problem

### 3. Independent Versioning
- Library crates: semantic versioning based on API stability
- Bitzel application: versioning based on features and stability
- Can update libraries without breaking application

### 4. Testing Benefits
- Unit test libraries in isolation
- Integration test application with mocked libraries
- Test graph algorithms once, reuse everywhere

### 5. Performance
- Graph algorithms optimized once in `convenient-graph`
- Consistent caching strategy across all graphs
- Can add graph-specific optimizations (e.g., parallel traversal)

### 6. Extensibility
- Easy to add new graph-based features
- Can create different executors (Docker, Podman, bare metal)
- Can support other build systems (Buck2, Pants, etc.)

## Implementation Checklist

### Phase 1: Convenient-Graph (Week 1)
- [ ] Create `convenient-graph` crate
- [ ] Implement generic `DAG<N, E>`
- [ ] Implement topological sort (Kahn's algorithm)
- [ ] Implement cycle detection
- [ ] Implement graph-based caching
- [ ] Add comprehensive tests (aim for 90%+ coverage)
- [ ] Add graph visualization (DOT format)

### Phase 2: Extract Executor (Week 2)
- [ ] Create `bitzel-executor` crate
- [ ] Move executor code from `convenient-bitbake`
- [ ] Update imports in `convenient-bitbake`
- [ ] Ensure all tests pass
- [ ] Update documentation

### Phase 3: Refactor Existing Graphs (Week 2-3)
- [ ] Update `convenient-kas/include_graph.rs` to use `convenient-graph`
- [ ] Update `convenient-bitbake/recipe_graph.rs` to use `convenient-graph`
- [ ] Update `convenient-bitbake/task_graph.rs` to use `convenient-graph`
- [ ] Ensure 100% test coverage maintained

### Phase 4: Create Bitzel Application (Week 3)
- [ ] Create `bitzel` crate
- [ ] Implement bootstrap from kas
- [ ] Implement build orchestration
- [ ] Add CLI with clap
- [ ] Add comprehensive integration tests

### Phase 5: Neo4j Adapter (Week 4)
- [ ] Create adapter in `graph-git`
- [ ] Implement DAG ↔ Neo4j sync
- [ ] Add query builders
- [ ] Update `graph-git-cli` to use adapter

## Example: Using Convenient-Graph

### Before (Duplicated Code)

```rust
// In convenient-kas/src/include_graph.rs
impl KasIncludeGraph {
    fn build_recursive(...) -> Result<()> {
        // Custom topological sort
        // Custom cycle detection
        // Custom dependency tracking
    }
}

// In convenient-bitbake/src/task_graph.rs
impl TaskGraph {
    fn topological_sort(&self) -> Vec<TaskId> {
        // Duplicate topological sort logic
    }
}
```

### After (Reused Code)

```rust
// In convenient-graph/src/lib.rs
impl<N, E> DAG<N, E> {
    pub fn topological_sort(&self) -> Result<Vec<NodeId>, CycleError> {
        // Single, well-tested implementation
    }
}

// In convenient-kas/src/include_graph.rs
use convenient_graph::DAG;

pub struct KasIncludeGraph {
    graph: DAG<KasFile, IncludeDependency>,
}

impl KasIncludeGraph {
    pub fn topological_sort(&self) -> Result<Vec<&KasFile>, KasError> {
        let node_ids = self.graph.topological_sort()?;
        Ok(node_ids.iter().map(|id| self.graph.node(*id)).collect())
    }
}
```

## Conclusion

This refactoring provides:
1. ✅ **Bitzel as own crate** - Clean application/library separation
2. ✅ **Generic graph technology** - Reusable, well-tested, extensible
3. ✅ **Better architecture** - Following Rust best practices
4. ✅ **Easier maintenance** - Single source of truth for graph algorithms
5. ✅ **Future-proof** - Easy to add new features and build systems

**Recommendation**: Start with Phase 1 (convenient-graph) as it provides immediate value and can be done incrementally without breaking existing code.
