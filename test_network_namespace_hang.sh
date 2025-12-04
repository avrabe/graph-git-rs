#!/bin/bash
# Test script to reproduce network namespace hang

echo "=== Test 1: Normal bash execution ==="
timeout 2 bash -c "echo 'Hello from bash'; echo \$\$"
echo "Exit code: $?"
echo

echo "=== Test 2: Bash in network namespace (no setup) ==="
timeout 2 unshare --net bash -c "echo 'Hello from netns'; echo \$\$"
echo "Exit code: $?"
echo

echo "=== Test 3: Bash in network namespace with loopback setup ==="
timeout 2 unshare --net bash -c "ip link set lo up; echo 'Hello with loopback'; echo \$\$; ip addr show lo"
echo "Exit code: $?"
echo

echo "=== Test 4: Check if loopback is needed for basic bash ==="
timeout 2 unshare --net bash -c "echo \$\$ > /tmp/test_pid.txt; cat /tmp/test_pid.txt"
echo "Exit code: $?"
echo

echo "=== Test 5: Simulate what the code does - no loopback setup ==="
timeout 2 unshare --net /bin/bash -c 'echo "Testing without loopback"; pwd'
echo "Exit code: $?"
