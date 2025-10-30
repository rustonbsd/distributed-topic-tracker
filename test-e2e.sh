#!/bin/bash

# End-to-end test script for distributed-topic-tracker
# This script runs multiple Docker containers and waits for them to discover each other

set -e

echo "Starting end-to-end test..."

# Clean up any existing containers
docker compose --file $COMPOSE_FILE down --remove-orphans || true

# Build and start the containers
echo "Building and starting containers..."
docker compose --file $COMPOSE_FILE up --build -d

# Function to check if a container has printed "Joined topic"
check_joined_topic() {
    local container_name=$1
    local timeout=${2:-60}
    local count=0
    
    echo "Waiting for $container_name to join topic..."
    
    while [ $count -lt $timeout ]; do
        if docker logs $container_name 2>&1 | grep -q "\[joined topic\]"; then
            echo "$container_name successfully joined topic"
            return 0
        fi
        sleep 1
        count=$((count + 1))
    done
    
    echo "$container_name failed to join topic within $timeout seconds"
    echo "Container logs:"
    docker logs $container_name
    return 1
}

# Function to check if a container has finished
check_finished() {
    local container_name=$1
    local timeout=${2:-60}
    local count=0
    
    echo "Waiting for $container_name to finish..."
    
    while [ $count -lt $timeout ]; do
        if docker logs $container_name 2>&1 | grep -q "\[finished\]"; then
            echo "$container_name finished successfully"
            return 0
        fi
        sleep 1
        count=$((count + 1))
    done
    
    echo "$container_name did not finish within $timeout seconds"
    return 1
}

# Wait for all nodes to join the topic
success=true

check_joined_topic "dtt-node1" 120 || success=false
check_joined_topic "dtt-node2" 120 || success=false
check_joined_topic "dtt-node3" 120 || success=false

# Wait for all nodes to finish their execution
echo ""
echo "Waiting for nodes to complete execution and discover each other..."
check_finished "dtt-node1" 120 || success=false
check_finished "dtt-node2" 120 || success=false
check_finished "dtt-node3" 120 || success=false

# Display all container logs
echo ""
echo "========================================="
echo "FULL LOGS FROM ALL NODES:"
echo "========================================="
echo ""
echo "--- Logs from dtt-node1 ---"
docker logs dtt-node1 2>&1
echo ""
echo "--- Logs from dtt-node2 ---"
docker logs dtt-node2 2>&1
echo ""
echo "--- Logs from dtt-node3 ---"
docker logs dtt-node3 2>&1
echo ""
echo "========================================="

# Clean up
echo "Cleaning up containers..."
docker compose --file $COMPOSE_FILE down 

if [ "$success" = true ]; then
    echo "End-to-end test PASSED: All nodes successfully joined the topic and completed execution"
    exit 0
else
    echo "End-to-end test FAILED: One or more nodes failed to join the topic or complete execution"
    exit 1
fi
