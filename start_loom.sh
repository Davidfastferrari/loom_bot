#!/bin/sh

# Set working directory
cd /app

# Check if config files exist
echo "Checking configuration files..."
if [ ! -f /app/config.toml ]; then
    echo "ERROR: config.toml not found"
    ls -la /app/config*
    exit 1
fi

echo "Configuration files found:"
ls -la /app/config*

# Set environment variables for better logging
export RUST_LOG=info
export RUST_BACKTRACE=1

echo "Starting loom applications..."

# Start loom_backrun in background with full path
echo "Starting loom_backrun..."
/app/loom_backrun &
LOOM_BACKRUN_PID=$!

# Start loom_base in background with full path
echo "Starting loom_base..."
/app/loom_base &
LOOM_BASE_PID=$!

# Function to handle signals and cleanup
cleanup() {
    echo "Shutting down..."
    kill $LOOM_BACKRUN_PID $LOOM_BASE_PID 2>/dev/null
    exit 0
}

# Set up signal handlers
trap cleanup SIGTERM SIGINT

# Wait for processes
wait $LOOM_BACKRUN_PID $LOOM_BASE_PID
