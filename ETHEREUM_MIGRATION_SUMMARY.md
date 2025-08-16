# Ethereum Mainnet Migration Summary

## Overview

This document summarizes the changes made to migrate the Loom Bot from Base network to Ethereum mainnet. The migration includes updating token addresses, RPC endpoints, chain IDs, and blockchain references throughout the codebase.

## Files Created

1. **`config_ethereum.toml`**: Complete configuration file for Ethereum mainnet
2. **`switch_to_ethereum.sh`**: Script to automate the migration process
3. **`Dockerfile.ethereum`**: Docker configuration for building with Ethereum mainnet settings
4. **`ETHEREUM_SETUP.md`**: Documentation for the Ethereum mainnet setup

## Key Changes

### 1. Network Configuration

| Setting | Base Network | Ethereum Mainnet |
|---------|--------------|------------------|
| Chain ID | 8453 | 1 |
| RPC URL | base-mainnet.g.alchemy.com | eth-mainnet.g.alchemy.com |
| Blockchain Name | "base" | "ethereum" |

### 2. Token Addresses

| Token | Base Network | Ethereum Mainnet |
|-------|--------------|------------------|
| WETH | 0x4200000000000000000000000000000000000006 | 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2 |
| USDT | 0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2 | 0xdAC17F958D2ee523a2206206994597C13D831ec7 |
| DAI | 0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb | 0x6B175474E89094C44Da98b954EedeAC495271d0F |
| WBTC | 0x0555E30da8f98308EdB960aa94C0Db47230d2B9c | 0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599 |
| rETH | 0xB6fe221Fe9EeF5aBa221c348bA20A1Bf5e73624c | 0xae78736Cd615f374D3085123A210448E74Fc6393 |
| wstETH | 0xc1CBa3fCea344f92D9239c08C0568f6F2F0ee452 | 0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0 |
| cbETH | 0x2Ae3F1Ec7F1F5012CFEab0185bfc7aa3cf0DEc22 | 0xBe9895146f7AF43049ca1c1AE358B0541Ea49704 |

### 3. Code Changes

#### In `loom_base/src/main.rs`:
- Changed blockchain references from "base" to "ethereum"
- Updated comments to reflect Ethereum mainnet

#### In `loom_backrun/src/main.rs`:
- Changed default blockchain from "base" to "ethereum"
- Updated chain ID map to use Ethereum mainnet (1) instead of Base (8453)
- Updated blockchain references in get_blockchain calls

### 4. Configuration Changes

#### In `config.toml`:
- Updated section names from `[tokens.base]` to `[tokens.ethereum]`
- Updated section names from `[backrun_strategy.base_config]` to `[backrun_strategy.ethereum_config]`
- Updated actor configurations to reference "ethereum" instead of "base"

## Migration Process

The migration process is automated through the `switch_to_ethereum.sh` script, which:

1. Backs up the original Base configuration
2. Copies the Ethereum configuration to the main config file
3. Updates blockchain references in the source code
4. Updates the chain ID map

## Docker Build Process

The `Dockerfile.ethereum` provides a complete build process that:

1. Applies the general fixes from `fix_all_issues.sh`
2. Runs the Ethereum migration script
3. Builds the application with Ethereum mainnet settings
4. Configures the runtime container with Ethereum mainnet configuration

## Verification Steps

After migration, verify:

1. The bot connects to Ethereum mainnet RPC
2. The chain ID is set to 1
3. Token addresses are correct for Ethereum mainnet
4. All blockchain references use "ethereum" instead of "base"

## Performance Considerations

When running on Ethereum mainnet:

1. Gas prices are typically higher than on Base
2. Transaction confirmation times may be longer
3. MEV competition is more intense
4. Capital requirements may be higher due to higher gas costs

Adjust the bot's configuration parameters accordingly, especially:
- `max_gas_price`
- `priority_fee`
- `min_profit_wei` (should be higher to account for higher gas costs)