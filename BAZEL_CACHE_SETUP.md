# Bazel Cache Server Setup for Bitzel Testing

This document describes how to set up and test Bitzel's Bazel Remote Execution API v2 cache implementation against real Bazel cache servers.

## Overview

Bitzel implements the Bazel Remote Execution API v2 for content-addressable storage (CAS) and action caching. This allows integration with existing Bazel remote cache infrastructure.

### API Components Implemented

- **Content-Addressable Storage (CAS)**: Stores blobs indexed by SHA256 digest
- **Action Cache (AC)**: Stores action results indexed by action digest
- **Byte Stream API**: (Future) For transferring large blobs

## Recommended Cache Servers

### 1. bazel-remote (Recommended for Testing)

**Repository**: https://github.com/buchgr/bazel-remote
**License**: Apache 2.0
**Language**: Go
**Status**: Production-ready, serving TBs/day since 2018

**Why bazel-remote?**
- Easy Docker setup for local testing
- Supports both HTTP/1.1 and gRPC
- Compatible with Bazel Remote Execution API v2
- Lightweight and fast
- Excellent for development and testing

**Quick Start with Docker:**

```bash
# Pull the latest image
docker pull buchgr/bazel-remote-cache

# Run with local storage (5GB max)
docker run -d \
  --name bazel-cache \
  -u 1000:1000 \
  -v $PWD/bazel-cache-data:/data \
  -p 9090:8080 \
  -p 9092:9092 \
  buchgr/bazel-remote-cache \
  --max_size 5 \
  --dir /data \
  --enable_endpoint_metrics
```

**Ports:**
- `9090`: HTTP endpoint
- `9092`: gRPC endpoint

**Testing connectivity:**

```bash
# HTTP health check
curl http://localhost:9090/status

# Test cache endpoint
curl http://localhost:9090/cas/<sha256-hash>
```

### 2. Buildfarm

**Repository**: https://github.com/bazelbuild/bazel-buildfarm
**License**: Apache 2.0
**Language**: Java
**Status**: Production-ready

Buildfarm is the reference implementation from Google, supporting full remote execution and caching. More complex to set up but suitable for production environments.

### 3. BuildGrid

**Repository**: https://buildgrid.build/
**License**: Apache 2.0
**Language**: Python
**Status**: Production-ready

BuildGrid is developed by Codethink and provides remote execution and caching capabilities.

## Testing Bitzel with bazel-remote

### 1. Start bazel-remote server

```bash
# Create cache directory
mkdir -p bazel-cache-data

# Start server
docker run -d \
  --name bazel-cache \
  -v $PWD/bazel-cache-data:/data \
  -p 9090:8080 \
  -p 9092:9092 \
  buchgr/bazel-remote-cache \
  --max_size 5 \
  --dir /data
```

### 2. Configure Bitzel to use remote cache

When implemented, Bitzel will support configuration like:

```yaml
# bitzel.toml
[cache]
type = "bazel-remote"
url = "grpc://localhost:9092"
# or for HTTP:
# url = "http://localhost:9090"

# Optional: instance name for multi-tenant caches
instance_name = "bitzel-dev"

# Local cache fallback
local_cache = ".bitzel-cache"

# Enable compression
compression = true
```

### 3. Test workflow

```bash
# Run a build with remote caching
bitzel build busybox

# Verify cache is populated
docker exec bazel-cache ls -lah /data/cas
docker exec bazel-cache ls -lah /data/ac

# Clean local cache
rm -rf .bitzel-cache

# Run build again - should use remote cache
bitzel build busybox  # Should be faster with cache hits
```

### 4. Monitor cache performance

```bash
# With metrics enabled, check cache stats
curl http://localhost:9090/metrics

# Look for:
# - bazel_remote_cache_hits
# - bazel_remote_cache_misses
# - bazel_remote_cas_blobs
```

## Implementation Status

### Current (Local Cache Only)

Bitzel currently implements:
- ✅ Local CAS with SHA256 sharding
- ✅ Local action cache
- ✅ ActionResult serialization/deserialization
- ✅ Compatible data structures with Bazel RE API v2

Location: `convenient-bitbake/src/executor/remote_cache.rs`

### Next Steps

To fully support remote caching:

1. **gRPC Client Implementation**
   - Add `tonic` dependency for gRPC
   - Implement ContentAddressableStorage service calls
   - Implement ActionCache service calls
   - Implement ByteStream for large blob transfers

2. **Protocol Buffers**
   - Add remote-apis protobuf definitions
   - Generate Rust code with prost
   - Align ActionResult structure with proto definitions

3. **Async I/O**
   - ✅ Convert file operations to tokio::fs (in progress)
   - Add async gRPC calls
   - Implement connection pooling

4. **Authentication & TLS**
   - Support TLS for gRPC connections
   - OAuth2 token authentication
   - mTLS for enterprise deployments

5. **Testing Infrastructure**
   - Integration tests against bazel-remote
   - Cache hit/miss ratio tracking
   - Performance benchmarks

## Bazel Remote Execution API v2 Resources

- **Protocol Definition**: https://github.com/bazelbuild/remote-apis/blob/main/build/bazel/remote/execution/v2/remote_execution.proto
- **Official Docs**: https://bazel.build/remote/caching
- **API Services**:
  - ContentAddressableStorage: Store and retrieve blobs
  - ActionCache: Store and retrieve action results
  - Capabilities: Discover server capabilities
  - ByteStream: Transfer large blobs

## Docker Compose for Development

Create `docker-compose.yml`:

```yaml
version: '3.8'

services:
  bazel-cache:
    image: buchgr/bazel-remote-cache
    container_name: bitzel-cache
    ports:
      - "9090:8080"  # HTTP
      - "9092:9092"  # gRPC
    volumes:
      - ./bazel-cache-data:/data
    command: >
      --max_size 5
      --dir /data
      --enable_endpoint_metrics
    restart: unless-stopped
```

Start with: `docker-compose up -d`

## Alternative: NativeLink (Rust Implementation)

For a Rust-native alternative, consider NativeLink:

**Repository**: https://github.com/TraceMachina/nativelink
**Language**: Rust
**Features**: Remote execution, caching, and analytics

This might be easier to integrate with Bitzel as both are written in Rust.

## Production Deployments

For production use, consider:

1. **Cloud Services**:
   - BuildBuddy (https://www.buildbuddy.io/)
   - EngFlow (https://www.engflow.com/)
   - Aspect Build (https://aspect.build/)

2. **Self-Hosted with S3/GCS**:
   - bazel-remote with S3 backend
   - Buildfarm with distributed storage
   - BuildGrid with cloud storage

3. **Kubernetes Deployment**:
   - Helm charts available for bazel-remote
   - Horizontal scaling for high throughput
   - Persistent volumes for cache storage

## Conclusion

bazel-remote is the recommended starting point for testing Bitzel's remote cache integration. Its simple Docker-based setup allows rapid development and testing of the Remote Execution API v2 implementation.

Next steps:
1. Set up local bazel-remote instance
2. Implement gRPC client in Bitzel
3. Test against real Yocto builds
4. Measure cache hit ratios and performance improvements
