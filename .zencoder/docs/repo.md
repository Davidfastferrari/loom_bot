# Loom Bot Information

## Summary
Loom Bot is a Rust-based blockchain trading bot designed for real-time analysis of blockchain data to identify and execute profitable trading strategies, including backrunning and arbitrage opportunities. The system monitors blockchain transactions, analyzes market conditions, and executes trades automatically.

## Structure
The project follows a modular architecture with a core actor-based system:
- **bin/**: Executable applications including the main `loom_base` bot
- **crates/**: Core functionality organized by domain
  - **broadcast/**: Transaction broadcasting components
  - **core/**: Actor system, blockchain interaction, and topology
  - **defi/**: DeFi-specific components (pools, pricing, market analysis)
  - **node/**: Node interaction, JSON-RPC, and state management
  - **strategy/**: Trading strategies (backrunning, arbitrage)
  - **types/**: Data structures and type definitions

## Language & Runtime
**Language**: Rust
**Version**: 2021 edition, Rust 1.84+
**Build System**: Cargo
**Package Manager**: Cargo

## Dependencies
**Main Dependencies**:
- **alloy**: Ethereum interaction library (v0.11.1)
- **revm**: Ethereum VM implementation (v19.5.0)
- **tokio**: Async runtime (v1.41.0)
- **reth**: Ethereum client implementation
- **diesel**: Database ORM (v2.2.4)
- **tracing**: Logging and diagnostics

## Build & Installation
```bash
cargo build --release
cargo run --bin loom_base
```

## Main Components

### Actor System
The project uses an actor-based architecture for concurrent processing:
- **Broadcaster**: Channel-based message passing system
- **Workers**: Process specific blockchain data (blocks, logs, state)
- **Actors**: Coordinate system components and manage state

### Trading Strategies
- **Backrunning**: Analyzes pending transactions to execute profitable follow-up trades
- **Arbitrage**: Identifies price differences across DEXes for profitable trades
- **Merger**: Combines multiple trading opportunities for optimal execution

## Runtime Error Analysis
The errors in the logs indicate issues with the broadcaster channels being closed prematurely:

```
[ERROR loom_node_json_rpc::node_block_logs_worker] Broadcaster error channel closed
[ERROR loom_node_json_rpc::node_block_state_worker] Broadcaster error channel closed
```

### Root Causes:
1. **Channel Closure**: The broadcast channels in the JSON-RPC workers are being closed unexpectedly
2. **Subscriber Disconnection**: When all subscribers to a broadcast channel disconnect, the channel closes
3. **Error Handling**: The workers continue trying to send messages to closed channels

### Applied Fixes:
1. **Enhanced Broadcaster Implementation**: Added reconnection capability and subscriber tracking to prevent premature channel closure
2. **Improved Worker Error Handling**: Updated workers to handle channel errors and attempt reconnection
3. **Base-Specific Transaction Support**: Added custom transaction deserializer to handle Base-specific transaction formats (0x7e variant)
4. **Proper Shutdown Sequence**: Implemented graceful shutdown with task monitoring and coordination
5. **Keep-Alive Mechanism**: Added periodic health checks for critical channels to maintain connectivity

These fixes address the core issues causing the runtime errors and should significantly improve the stability and reliability of the Loom Bot system.