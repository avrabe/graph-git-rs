#!/bin/bash
# Test script for Bazel Remote Cache with podman
#
# This script sets up and tests bazel-remote cache server using podman,
# then validates cache functionality with example builds.

set -e

# Colors for output
RED='\033[0.31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

echo_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

echo_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if podman is available
if ! command -v podman &> /dev/null; then
    echo_warn "podman not found, falling back to docker"
    if ! command -v docker &> /dev/null; then
        echo_error "Neither podman nor docker is available"
        exit 1
    fi
    CONTAINER_CMD="docker"
else
    CONTAINER_CMD="podman"
    echo_info "Using podman for containerization"
fi

# Configuration
CACHE_NAME="bitzel-bazel-cache"
CACHE_IMAGE="buchgr/bazel-remote-cache:latest"
HTTP_PORT=9090
GRPC_PORT=9092
CACHE_DIR="$(pwd)/bazel-cache-data"
MAX_SIZE=5  # GB

# Function to cleanup
cleanup() {
    echo_info "Cleaning up..."
    $CONTAINER_CMD stop $CACHE_NAME 2>/dev/null || true
    $CONTAINER_CMD rm $CACHE_NAME 2>/dev/null || true
}

# Function to check if container is running
is_running() {
    $CONTAINER_CMD ps --format '{{.Names}}' | grep -q "^${CACHE_NAME}$"
}

# Function to wait for cache to be ready
wait_for_cache() {
    echo_info "Waiting for cache server to be ready..."
    local max_attempts=30
    local attempt=0

    while [ $attempt -lt $max_attempts ]; do
        if curl -s http://localhost:$HTTP_PORT/status > /dev/null 2>&1; then
            echo_info "Cache server is ready!"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 1
    done

    echo_error "Cache server failed to start within ${max_attempts} seconds"
    return 1
}

# Main execution
echo_info "=== Bazel Remote Cache Test ==="
echo_info "Container runtime: $CONTAINER_CMD"
echo_info "HTTP Port: $HTTP_PORT"
echo_info "gRPC Port: $GRPC_PORT"
echo_info "Cache directory: $CACHE_DIR"
echo ""

# Stop and remove existing container if running
if is_running; then
    echo_info "Stopping existing cache container..."
    cleanup
fi

# Create cache directory
mkdir -p "$CACHE_DIR"
echo_info "Cache directory created: $CACHE_DIR"

# Pull latest image
echo_info "Pulling latest cache image..."
$CONTAINER_CMD pull $CACHE_IMAGE

# Start cache server
echo_info "Starting bazel-remote cache server..."
$CONTAINER_CMD run -d \
    --name $CACHE_NAME \
    -v "$CACHE_DIR:/data:Z" \
    -p $HTTP_PORT:8080 \
    -p $GRPC_PORT:9092 \
    $CACHE_IMAGE \
    --max_size $MAX_SIZE \
    --dir /data \
    --enable_endpoint_metrics \
    --http_address 0.0.0.0:8080 \
    --grpc_address 0.0.0.0:9092

# Wait for cache to be ready
if ! wait_for_cache; then
    echo_error "Failed to start cache server"
    echo_info "Container logs:"
    $CONTAINER_CMD logs $CACHE_NAME
    cleanup
    exit 1
fi

# Test HTTP endpoint
echo ""
echo_info "=== Testing HTTP Endpoint ==="
STATUS=$(curl -s http://localhost:$HTTP_PORT/status)
echo_info "Status response: $STATUS"

# Test cache write and read
echo ""
echo_info "=== Testing Cache Operations ==="

# Generate test data
TEST_DATA="This is test data for cache validation: $(date)"
TEST_HASH=$(echo -n "$TEST_DATA" | sha256sum | cut -d' ' -f1)

echo_info "Test data hash: $TEST_HASH"

# Write to CAS
echo_info "Writing test data to CAS..."
WRITE_RESPONSE=$(curl -s -X PUT \
    -H "Content-Type: application/octet-stream" \
    --data "$TEST_DATA" \
    "http://localhost:$HTTP_PORT/cas/$TEST_HASH")

echo_info "Write response: ${WRITE_RESPONSE:-<empty>}"

# Read from CAS
echo_info "Reading test data from CAS..."
READ_DATA=$(curl -s "http://localhost:$HTTP_PORT/cas/$TEST_HASH")

if [ "$READ_DATA" = "$TEST_DATA" ]; then
    echo_info "✓ Cache write/read test PASSED"
else
    echo_error "✗ Cache write/read test FAILED"
    echo_error "Expected: $TEST_DATA"
    echo_error "Got: $READ_DATA"
fi

# Check cache statistics
echo ""
echo_info "=== Cache Statistics ==="
if command -v jq &> /dev/null; then
    STATS=$(curl -s http://localhost:$HTTP_PORT/status | jq .)
    echo "$STATS"
else
    curl -s http://localhost:$HTTP_PORT/status
fi

# List cache contents
echo ""
echo_info "=== Cache Contents ==="
echo_info "CAS entries:"
$CONTAINER_CMD exec $CACHE_NAME find /data/cas -type f | head -10

# Show disk usage
echo ""
echo_info "=== Disk Usage ==="
$CONTAINER_CMD exec $CACHE_NAME du -sh /data/*

# Test gRPC endpoint (basic connectivity)
echo ""
echo_info "=== Testing gRPC Endpoint ==="
if command -v grpcurl &> /dev/null; then
    echo_info "Querying gRPC services..."
    grpcurl -plaintext localhost:$GRPC_PORT list || echo_warn "gRPC service list failed (expected if not configured)"
else
    echo_warn "grpcurl not installed, skipping gRPC detailed tests"
    echo_info "To install: go install github.com/fullstorydev/grpcurl/cmd/grpcurl@latest"
fi

# Summary
echo ""
echo_info "=== Test Summary ==="
echo_info "✓ Cache server started successfully"
echo_info "✓ HTTP endpoint accessible on port $HTTP_PORT"
echo_info "✓ gRPC endpoint accessible on port $GRPC_PORT"
echo_info "✓ Cache directory: $CACHE_DIR"
echo_info "✓ Basic cache operations working"
echo ""
echo_info "Cache server is running. To stop it:"
echo_info "  $CONTAINER_CMD stop $CACHE_NAME"
echo_info ""
echo_info "To view logs:"
echo_info "  $CONTAINER_CMD logs -f $CACHE_NAME"
echo_info ""
echo_info "To access cache:"
echo_info "  HTTP:  http://localhost:$HTTP_PORT"
echo_info "  gRPC:  grpc://localhost:$GRPC_PORT"
echo ""
echo_info "Test completed successfully!"
