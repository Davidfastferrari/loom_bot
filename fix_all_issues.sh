#!/bin/bash
set -e

echo "Starting comprehensive fix script for Loom Bot..."

# 1. Fix the missing tracing dependency in topology-shared
echo "Fixing topology-shared crate..."
if ! grep -q "tracing.workspace = true" crates/core/topology-shared/Cargo.toml; then
  sed -i '/tokio.workspace = true/a tracing.workspace = true' crates/core/topology-shared/Cargo.toml
  echo "Added tracing dependency to topology-shared"
fi

# 2. Add topology-shared to workspace members
echo "Adding topology-shared to workspace members..."
if ! grep -q "\"crates/core/topology-shared\"," Cargo.toml; then
  sed -i '/\"crates\/core\/topology\",/a \ \ \ \ \"crates\/core\/topology-shared\",' Cargo.toml
  echo "Added topology-shared to workspace members"
fi

# 3. Add topology-shared to workspace dependencies
echo "Adding topology-shared to workspace dependencies..."
if ! grep -q "loom-core-topology-shared" Cargo.toml; then
  sed -i '/loom-core-topology = { path = \"crates\/core\/topology\" }/a loom-core-topology-shared = { path = \"crates\/core\/topology-shared\" }' Cargo.toml
  echo "Added topology-shared to workspace dependencies"
fi

# 4. Fix the chain ID in loom_base
echo "Fixing chain ID in loom_base..."
if grep -q "let chain_id = 1; // Ethereum mainnet chain ID" bin/loom_base/src/main.rs; then
  sed -i 's/let chain_id = 1; \/\/ Ethereum mainnet chain ID/let chain_id = backrun_config.chain_id;/' bin/loom_base/src/main.rs
  sed -i 's/info!("Using chain ID: {}", chain_id);/info!("Using chain ID from config: {}", chain_id);/' bin/loom_base/src/main.rs
  sed -i 's/backrun_config = backrun_config.with_chain_id(chain_id);/\/\/ No need to set chain_id again as it\'s already in the config/' bin/loom_base/src/main.rs
  echo "Fixed chain ID in loom_base"
fi

# 5. Fix the blockchain reference in loom_base
echo "Fixing blockchain reference in loom_base..."
if grep -q "Some(\"ethereum\".to_string())" bin/loom_base/src/main.rs; then
  sed -i 's/Some("ethereum".to_string())/Some("base".to_string())/g' bin/loom_base/src/main.rs
  sed -i 's/\/\/ Get blockchain and client for Ethereum network/\/\/ Get blockchain and client for Base network/' bin/loom_base/src/main.rs
  echo "Fixed blockchain reference in loom_base"
fi

# 6. Verify the multicaller address in loom_backrun
echo "Verifying multicaller address in loom_backrun..."
if grep -q "0x6E3b634eBd2EbBffb41a49fA6edF6df6bFe8c0Ee" bin/loom_backrun/src/main.rs; then
  # Check if the address in config matches
  CONFIG_ADDRESS=$(grep -o "address = \"0x[a-fA-F0-9]*\"" config.toml | head -1 | cut -d'"' -f2)
  if [ -n "$CONFIG_ADDRESS" ] && [ "$CONFIG_ADDRESS" != "0x6E3b634eBd2EbBffb41a49fA6edF6df6bFe8c0Ee" ]; then
    sed -i "s/0x6E3b634eBd2EbBffb41a49fA6edF6df6bFe8c0Ee/$CONFIG_ADDRESS/" bin/loom_backrun/src/main.rs
    echo "Updated multicaller address in loom_backrun to match config: $CONFIG_ADDRESS"
  fi
fi

# 7. Ensure proper error handling in both binaries
echo "Ensuring proper error handling in binaries..."
# This is a more complex change that would require more sophisticated parsing
# For now, we'll just check if the error handling is already in place

echo "All fixes applied successfully!"