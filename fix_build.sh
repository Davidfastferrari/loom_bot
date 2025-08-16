#!/bin/bash
set -e

# Add tracing dependency to topology-shared
echo "Fixing topology-shared crate..."
if ! grep -q "tracing.workspace = true" crates/core/topology-shared/Cargo.toml; then
  sed -i '/tokio.workspace = true/a tracing.workspace = true' crates/core/topology-shared/Cargo.toml
  echo "Added tracing dependency to topology-shared"
fi

# Add topology-shared to workspace members
echo "Adding topology-shared to workspace members..."
if ! grep -q "\"crates/core/topology-shared\"," Cargo.toml; then
  sed -i '/\"crates\/core\/topology\",/a \ \ \ \ \"crates\/core\/topology-shared\",' Cargo.toml
  echo "Added topology-shared to workspace members"
fi

# Add topology-shared to workspace dependencies
echo "Adding topology-shared to workspace dependencies..."
if ! grep -q "loom-core-topology-shared" Cargo.toml; then
  sed -i '/loom-core-topology = { path = \"crates\/core\/topology\" }/a loom-core-topology-shared = { path = \"crates\/core\/topology-shared\" }' Cargo.toml
  echo "Added topology-shared to workspace dependencies"
fi

echo "All fixes applied successfully!"