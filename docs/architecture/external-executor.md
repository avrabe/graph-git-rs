# External Executor Architecture

## Overview

The External Executor abstraction provides a channel-based execution framework designed for future WASM component integration. It decouples the host from the executor implementation through message passing, enabling:

- **In-process execution** (LocalExecutor) - current implementation
- **WASM component execution** (WasmExecutorHost) - future capability
- **Remote execution** - future capability

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                         Host Process                         │
│                                                              │
│  ┌────────────────────┐                                     │
│  │  ExecutorPool      │                                     │
│  │  ┌──────────────┐  │                                     │
│  │  │ Handle 1     │──┼──┐                                  │
│  │  ├──────────────┤  │  │                                  │
│  │  │ Handle 2     │──┼──┼──┐                               │
│  │  ├──────────────┤  │  │  │                               │
│  │  │ Handle 3     │──┼──┼──┼──┐                            │
│  │  └──────────────┘  │  │  │  │                            │
│  └────────────────────┘  │  │  │                            │
│                          │  │  │                            │
│  ┌───────────────────────┼──┼──┼────────────────────────┐  │
│  │  Channel Layer        │  │  │                        │  │
│  │                       ▼  ▼  ▼                        │  │
│  │  ┌────────┐  ┌────────┐  ┌────────┐                 │  │
│  │  │ mpsc   │  │ mpsc   │  │ mpsc   │                 │  │
│  │  │ tx/rx  │  │ tx/rx  │  │ tx/rx  │                 │  │
│  │  └────┬───┘  └────┬───┘  └────┬───┘                 │  │
│  └───────┼───────────┼───────────┼─────────────────────┘  │
│          │           │           │                         │
├──────────┼───────────┼───────────┼─────────────────────────┤
│          │           │           │                         │
│  ┌───────▼───────────▼───────────▼─────────────────────┐  │
│  │  Executor Backends                                   │  │
│  │                                                      │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌───────────┐ │  │
│  │  │LocalExecutor │  │WasmExecutor  │  │Remote     │ │  │
│  │  │              │  │Host          │  │Executor   │ │  │
│  │  │ ┌──────────┐ │  │ ┌──────────┐ │  │           │ │  │
│  │  │ │TaskExec  │ │  │ │ WASM     │ │  │ (Future)  │ │  │
│  │  │ │          │ │  │ │Component │ │  │           │ │  │
│  │  │ │(In-proc) │ │  │ │(Isolated)│ │  │           │ │  │
│  │  │ └──────────┘ │  │ └──────────┘ │  │           │ │  │
│  │  └──────────────┘  └──────────────┘  └───────────┘ │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## Message Protocol

### Message Types

All communication between host and executor uses serializable messages:

#### Host → Executor Messages (`ExecutorMessage`)

```rust
enum ExecutorMessage {
    ExecuteTask { request_id: u64, task: TaskSpec },
    GetStatus { request_id: u64 },
    Ping { request_id: u64 },
    Shutdown { request_id: u64 },
    GetCapabilities { request_id: u64 },
}
```

#### Executor → Host Responses (`ExecutorResponse`)

```rust
enum ExecutorResponse {
    TaskResult { request_id: u64, result: Result<TaskOutput, String> },
    Status { request_id: u64, status: ExecutorStatus },
    Pong { request_id: u64 },
    ShutdownAck { request_id: u64 },
    Capabilities { request_id: u64, capabilities: ExecutorCapabilities },
    Error { request_id: u64, error: String },
}
```

### Protocol Flow

```
Host                           Executor
 │                                │
 ├─ExecuteTask(id=1, task)───────▶│
 │                                ├─ Execute in sandbox
 │                                ├─ Compute signature
 │                                ├─ Check cache
 │                                └─ Run task
 │◀──TaskResult(id=1, output)────┤
 │                                │
 ├─GetStatus(id=2)───────────────▶│
 │◀──Status(id=2, status)─────────┤
 │                                │
 ├─Ping(id=3)────────────────────▶│
 │◀──Pong(id=3)───────────────────┤
 │                                │
 ├─Shutdown(id=4)────────────────▶│
 │◀──ShutdownAck(id=4)────────────┤
 │                                │
```

## Components

### 1. ExternalExecutor Trait

The core abstraction that all executor backends must implement:

```rust
#[async_trait::async_trait]
pub trait ExternalExecutor: Send + Sync {
    async fn start(&mut self) -> ExecutorResult<(
        mpsc::Sender<ExecutorMessage>,
        mpsc::Receiver<ExecutorResponse>,
    )>;

    async fn stop(&mut self) -> ExecutorResult<()>;
    fn name(&self) -> &str;
    async fn capabilities(&self) -> ExecutorCapabilities;
}
```

### 2. ExecutorHandle

High-level API for communicating with executors:

```rust
pub struct ExecutorHandle {
    name: String,
    tx: mpsc::Sender<ExecutorMessage>,
    rx: mpsc::Receiver<ExecutorResponse>,
    next_request_id: u64,
}

impl ExecutorHandle {
    pub async fn execute_task(&mut self, task: TaskSpec) -> ExecutorResult<TaskOutput>;
    pub async fn get_status(&mut self) -> ExecutorResult<ExecutorStatus>;
    pub async fn ping(&mut self) -> ExecutorResult<()>;
    pub async fn shutdown(&mut self) -> ExecutorResult<()>;
}
```

### 3. LocalExecutor

In-process executor that wraps the existing TaskExecutor:

```rust
pub struct LocalExecutor {
    name: String,
    cache_dir: PathBuf,
    channel_buffer_size: usize,
    executor: Option<Arc<Mutex<TaskExecutor>>>,
}
```

**Features:**
- Runs TaskExecutor in a separate async task
- Communicates via channels (same protocol as future WASM executor)
- Production-ready implementation
- Full caching and sandboxing support

### 4. WasmExecutorHost (Future)

Placeholder for WASM component-based execution:

```rust
pub struct WasmExecutorHost {
    name: String,
    component_path: PathBuf,
    engine: Option<WasmEngine>,
    instance: Option<WasmInstance>,
}
```

**Future Implementation:**
1. Load WASM component using Wasmtime
2. Instantiate with host imports (CAS access, logging)
3. Call component exports for task execution
4. Serialize TaskSpec → bytes → WASM → bytes → TaskOutput

### 5. ExecutorPool

Manages multiple executor instances for parallel execution:

```rust
pub struct ExecutorPool {
    config: ExecutorConfig,
    executors: Vec<Arc<Mutex<ExecutorHandle>>>,
    semaphore: Arc<Semaphore>,
}

impl ExecutorPool {
    pub async fn execute_task(&self, spec: TaskSpec) -> ExecutorResult<TaskOutput>;
    pub async fn get_all_status(&self) -> ExecutorResult<Vec<ExecutorStatus>>;
    pub async fn aggregate_stats(&self) -> ExecutorResult<AggregateStats>;
}
```

**Features:**
- Round-robin task distribution
- Semaphore-based concurrency control
- Health monitoring across all executors
- Aggregate statistics

## Usage Examples

### Basic Local Executor

```rust
use convenient_bitbake::executor::{
    LocalExecutor, ExecutorHandle, TaskSpec,
    NetworkPolicy, ResourceLimits,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create executor
    let mut executor = LocalExecutor::new(
        "worker-1".to_string(),
        PathBuf::from("/tmp/cache"),
        100, // channel buffer size
    );

    // Start executor and get communication channels
    let (tx, rx) = executor.start().await?;
    let mut handle = ExecutorHandle::new("worker-1".to_string(), tx, rx);

    // Create task specification
    let task = TaskSpec {
        name: "do_compile".to_string(),
        recipe: "busybox".to_string(),
        script: "make -j4".to_string(),
        workdir: PathBuf::from("/build/busybox"),
        env: HashMap::new(),
        outputs: vec![PathBuf::from("busybox")],
        timeout: Some(Duration::from_secs(300)),
        network_policy: NetworkPolicy::Isolated,
        resource_limits: ResourceLimits::default(),
    };

    // Execute task
    let result = handle.execute_task(task).await?;
    println!("Task completed: {:?}", result);

    // Cleanup
    handle.shutdown().await?;
    executor.stop().await?;

    Ok(())
}
```

### Using ExecutorPool

```rust
use convenient_bitbake::executor::{
    ExecutorPool, ExecutorConfig, ExecutorBackend, TaskSpec,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure pool
    let config = ExecutorConfig {
        backend: ExecutorBackend::Local,
        cache_dir: PathBuf::from("/tmp/cache"),
        max_parallel: 4, // 4 worker executors
        channel_buffer_size: 100,
        verbose: false,
    };

    // Create pool
    let pool = ExecutorPool::new(config).await?;

    // Execute tasks in parallel
    let tasks = vec![task1, task2, task3, task4];
    let mut handles = Vec::new();

    for task in tasks {
        let pool_clone = pool.clone();
        let handle = tokio::spawn(async move {
            pool_clone.execute_task(task).await
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        let result = handle.await??;
        println!("Task result: {:?}", result);
    }

    // Get statistics
    let stats = pool.aggregate_stats().await?;
    println!("Success rate: {:.2}%", stats.success_rate());

    // Cleanup
    pool.shutdown().await?;

    Ok(())
}
```

### Configuring Backend

```rust
// Local in-process execution
let config = ExecutorConfig {
    backend: ExecutorBackend::Local,
    cache_dir: PathBuf::from("/tmp/cache"),
    max_parallel: num_cpus::get(),
    channel_buffer_size: 100,
    verbose: false,
};

// Future: WASM component execution
let config = ExecutorConfig {
    backend: ExecutorBackend::Wasm {
        component_path: PathBuf::from("./executor.component.wasm"),
    },
    cache_dir: PathBuf::from("/tmp/cache"),
    max_parallel: 8,
    channel_buffer_size: 100,
    verbose: true,
};

// Future: Remote execution
let config = ExecutorConfig {
    backend: ExecutorBackend::Remote {
        endpoint: "http://build-cluster.example.com:8080".to_string(),
    },
    cache_dir: PathBuf::from("/tmp/cache"),
    max_parallel: 16,
    channel_buffer_size: 100,
    verbose: false,
};
```

## Benefits of Channel-Based Design

### 1. **WASM-Ready**

The channel-based protocol maps directly to WASM component model:

```
Host Process                    WASM Component
┌──────────────┐               ┌──────────────┐
│ ExecutorPool │               │  executor.   │
│              │               │  component.  │
│  serialize   │──TaskSpec────▶│  wasm        │
│  TaskSpec    │               │              │
│              │               │  TaskExecutor│
│              │◀──TaskOutput──│  (Rust code) │
│  deserialize │               │              │
│  TaskOutput  │               │              │
└──────────────┘               └──────────────┘
```

### 2. **Platform Independence**

- Same protocol works across platforms
- WASM components are portable (Linux, macOS, Windows)
- No platform-specific executor code in host

### 3. **Isolation**

- WASM provides sandboxing beyond Linux namespaces
- Host and executor cannot interfere with each other
- Resource limits enforced by WASM runtime

### 4. **Composability**

- Swap executor backends without changing host code
- Mix local and remote executors in same pool
- Test with local, deploy with WASM or remote

### 5. **Testability**

- Mock executors for unit tests
- Inject delays/failures for integration tests
- Validate protocol compliance

## Future: WASM Component Integration

### WIT Definition

```wit
// wit/executor.wit
package bitzel:executor@0.1.0;

interface types {
    record task-spec {
        name: string,
        recipe: string,
        script: string,
        workdir: string,
        env: list<tuple<string, string>>,
        outputs: list<string>,
        timeout-secs: option<u64>,
    }

    record task-output {
        signature: string,
        output-files: list<tuple<string, string>>,
        stdout: string,
        stderr: string,
        exit-code: s32,
        duration-ms: u64,
    }
}

interface executor {
    use types.{task-spec, task-output};

    execute-task: func(task: task-spec) -> result<task-output, string>;
    ping: func() -> bool;
}

interface host {
    fetch-file: func(hash: string) -> result<list<u8>, string>;
    store-file: func(content: list<u8>) -> result<string, string>;
    log: func(level: string, message: string);
}

world executor-component {
    export executor;
    import host;
}
```

### Building WASM Component

```bash
# 1. Install tools
rustup target add wasm32-wasi
cargo install wit-bindgen-cli wasm-tools

# 2. Generate bindings
wit-bindgen rust --out-dir src/bindings wit/executor.wit

# 3. Build component
cargo build --target wasm32-wasi --release --lib

# 4. Create component
wasm-tools component new \
    target/wasm32-wasi/release/libconvenient_bitbake.wasm \
    -o executor.component.wasm

# 5. Use in bitzel
bitzel build --executor-backend wasm \
    --executor-component ./executor.component.wasm
```

### Host Implementation

```rust
// Future WasmExecutorHost implementation
use wasmtime::component::*;
use wasmtime::*;

impl WasmExecutorHost {
    async fn initialize_wasm(&mut self) -> ExecutorResult<()> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.async_support(true);

        let engine = Engine::new(&config)?;
        let component = Component::from_file(&engine, &self.component_path)?;

        let mut linker = Linker::new(&engine);

        // Add host imports
        linker.func_wrap("host", "fetch-file", |hash: String| -> Vec<u8> {
            // Fetch from CAS
            self.cas.get(&hash)
        })?;

        linker.func_wrap("host", "store-file", |content: Vec<u8>| -> String {
            // Store in CAS
            self.cas.put(&content)
        })?;

        let mut store = Store::new(&engine, ());
        let instance = linker.instantiate_async(&mut store, &component).await?;

        self.instance = Some(instance);
        Ok(())
    }
}
```

## Migration Path

### Phase 1: Current (✅ Complete)

- ✅ LocalExecutor with channel-based protocol
- ✅ ExecutorPool for parallel execution
- ✅ ExecutorHandle API
- ✅ Message serialization/deserialization

### Phase 2: WASM Integration (Future)

1. Define WIT interface
2. Generate bindings
3. Implement WasmExecutorHost
4. Build executor WASM component
5. Test WASM executor with existing tasks
6. Add WASM backend selection to CLI

### Phase 3: Remote Execution (Future)

1. Add RemoteExecutor implementation
2. gRPC or HTTP API for task submission
3. Authentication and authorization
4. Distributed caching
5. Load balancing across remote workers

## Performance Considerations

### Channel Overhead

- **LocalExecutor**: Minimal overhead (~microseconds per message)
- **WasmExecutor**: Serialization + WASM call overhead (~milliseconds)
- **RemoteExecutor**: Network latency + serialization (~tens of milliseconds)

### When to Use Each Backend

| Backend       | Use Case                                    | Latency  | Isolation | Platform |
|---------------|---------------------------------------------|----------|-----------|----------|
| Local         | Single-machine builds, development          | Low      | Good      | Current  |
| WASM          | Multi-platform, enhanced isolation          | Medium   | Excellent | All      |
| Remote        | Distributed builds, CI/CD clusters          | High     | Good      | All      |

## Testing

### Unit Tests

```bash
# Test external executor abstraction
cargo test --package convenient-bitbake executor::external

# Test local executor
cargo test --package convenient-bitbake executor::local_executor

# Test executor pool
cargo test --package convenient-bitbake executor::executor_pool
```

### Integration Tests

```bash
# Test with real tasks
cargo test --package convenient-bitbake --test integration_tests
```

## File Locations

- **Core abstraction**: `convenient-bitbake/src/executor/external.rs`
- **Local executor**: `convenient-bitbake/src/executor/local_executor.rs`
- **WASM executor**: `convenient-bitbake/src/executor/wasm_executor.rs`
- **Executor pool**: `convenient-bitbake/src/executor/executor_pool.rs`
- **Types**: `convenient-bitbake/src/executor/types.rs`

## Summary

The External Executor abstraction provides a clean, channel-based interface for task execution that:

1. **Works today** with LocalExecutor for production builds
2. **Prepares for tomorrow** with WASM component integration
3. **Scales beyond** with remote execution support

The design prioritizes:
- **Isolation**: Executors cannot interfere with host or each other
- **Portability**: WASM components run on any platform
- **Composability**: Mix and match executor backends
- **Performance**: Minimal overhead for local execution

This architecture positions the project for future growth while maintaining current functionality.
