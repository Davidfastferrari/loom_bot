#!/bin/bash
set -e

echo "Switching Loom Bot configuration from Base to Ethereum Mainnet..."

# Backup the original config file
cp config_base.toml config_base.toml.bak

# Copy the Ethereum config file to the main config file
cp config_ethereum.toml config.toml

# Update the blockchain references in the loom_base binary
sed -i 's/Some("base".to_string())/Some("ethereum".to_string())/g' bin/loom_base/src/main.rs
sed -i 's/\/\/ Get blockchain and client for Base network/\/\/ Get blockchain and client for Ethereum network/' bin/loom_base/src/main.rs

# Update the blockchain references in the loom_backrun binary
sed -i 's/topology.set_default_blockchain("base")/topology.set_default_blockchain("ethereum")/' bin/loom_backrun/src/main.rs
sed -i 's/let blockchain = topology.get_blockchain(Some(&"base".to_string()))?;/let blockchain = topology.get_blockchain(Some(&"ethereum".to_string()))?;/' bin/loom_backrun/src/main.rs
sed -i 's/let blockchain_state = topology.get_blockchain_state(Some(&"base".to_string()))?;/let blockchain_state = topology.get_blockchain_state(Some(&"ethereum".to_string()))?;/' bin/loom_backrun/src/main.rs
sed -i 's/let strategy = topology.get_strategy(Some(&"base".to_string()))?;/let strategy = topology.get_strategy(Some(&"ethereum".to_string()))?;/' bin/loom_backrun/src/main.rs

# Update the chain ID map in loom_backrun
sed -i 's/chain_id_map.insert("base".to_string(), 8453);/chain_id_map.insert("ethereum".to_string(), 1);/' bin/loom_backrun/src/main.rs

echo "Configuration updated successfully for Ethereum Mainnet!"
echo "Please rebuild the application with: cargo build --release"