# Ethereum Mainnet Configuration for Loom Bot

This document provides instructions for configuring the Loom Bot to run on Ethereum mainnet instead of Base network.

## Token Address Updates

The token addresses have been updated to use Ethereum mainnet addresses:

| Token | Ethereum Mainnet Address |
|-------|--------------------------|
| WETH  | 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2 |
| USDT  | 0xdAC17F958D2ee523a2206206994597C13D831ec7 |
| USDC  | 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48 |
| DAI   | 0x6B175474E89094C44Da98b954EedeAC495271d0F |
| WBTC  | 0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599 |
| rETH  | 0xae78736Cd615f374D3085123A210448E74Fc6393 |
| stETH | 0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84 |
| wstETH| 0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0 |
| cbETH | 0xBe9895146f7AF43049ca1c1AE358B0541Ea49704 |

## Configuration Changes

The following changes have been made to switch from Base to Ethereum mainnet:

1. Updated RPC endpoint to Ethereum mainnet
2. Changed chain ID from 8453 (Base) to 1 (Ethereum)
3. Updated all blockchain references from "base" to "ethereum"
4. Updated token addresses to Ethereum mainnet addresses
5. Updated configuration section names to reflect Ethereum mainnet

## How to Apply the Changes

### Option 1: Use the provided script

1. Run the switch script:
   ```bash
   chmod +x switch_to_ethereum.sh
   ./switch_to_ethereum.sh
   ```

2. Rebuild the application:
   ```bash
   cargo build --release
   ```

### Option 2: Use the Ethereum Dockerfile

1. Build the Docker image using the Ethereum Dockerfile:
   ```bash
   docker build -t loom-bot-ethereum -f Dockerfile.ethereum .
   ```

2. Run the container:
   ```bash
   docker run -d --name loom-bot-ethereum loom-bot-ethereum
   ```

## Verification

After applying these changes, the bot should connect to Ethereum mainnet and use the correct token addresses. You can verify this by checking the logs:

```bash
docker logs loom-bot-ethereum
```

Look for messages indicating:
- Connection to Ethereum mainnet
- Chain ID 1
- References to Ethereum mainnet tokens

## Important Notes

1. Make sure your private key has sufficient ETH on Ethereum mainnet
2. Gas prices on Ethereum mainnet are typically higher than on Base
3. Adjust the `max_gas_price` and `priority_fee` settings as needed for Ethereum mainnet
4. The multicaller contract address may need to be deployed on Ethereum mainnet

## Troubleshooting

If you encounter issues:

1. Check the logs for any connection errors
2. Verify that the RPC endpoint is accessible
3. Ensure your private key has sufficient ETH for gas
4. Check that the token addresses are correct for the pools you're targeting