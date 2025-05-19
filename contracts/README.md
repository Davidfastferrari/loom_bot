# LoomMulticaller Contract

This contract is designed to work with the Loom bot for executing arbitrage and backrunning trades on the Base network. It's optimized for fast execution and supports complex trade paths.

## Features

- Executes complex trade paths with multiple hops
- Supports flash loans from Balancer
- Handles callbacks from various DEX protocols (Uniswap V2/V3, etc.)
- Optimized for gas efficiency and fast execution
- Supports stack manipulation for complex operations
- Extracts profits automatically

## Prerequisites

- Node.js and npm installed
- Solidity compiler (solc) version 0.8.19 or higher
- Ethereum development environment (Hardhat, Truffle, or similar)
- Private key with ETH on Base network for deployment

## Deployment

1. Compile the contract:

```bash
solc --optimize --optimize-runs=200 --bin --abi LoomMulticaller.sol -o ./
```

2. Set your private key as an environment variable:

```bash
export PRIVATE_KEY=your_private_key_here
```

3. Run the deployment script:

```bash
node deploy_multicaller.js
```

4. Update the `config_base.toml` file with the new contract address (the script should do this automatically).

5. Fund the contract with ETH for gas and initial capital.

## Testing

Run the test script to verify the contract works correctly:

```bash
node test_multicaller.js
```

## Integration with Loom Bot

The contract is designed to work seamlessly with the Loom bot. The bot will:

1. Identify profitable trade opportunities
2. Encode the trade paths into opcodes
3. Send the encoded data to the contract's `doCalls` function
4. The contract will execute the trades and extract profits

## Contract Interface

The main function used by the Loom bot is:

```solidity
function doCalls(bytes calldata data) external payable returns (uint256);
```

This function takes encoded opcodes as input and returns the profit amount.

## Opcode Format

The opcodes are encoded in the following format:

1. **Header (12 bytes)**:
   - Selector (2 bytes): Indicates the call type
   - Call stack info (3 bytes): For stack manipulation
   - Return stack info (3 bytes): For stack manipulation
   - Call data length (2 bytes): Length of the call data

2. **Target Address (20 bytes)**: The address to call (except for calculation and internal calls)

3. **Call Data (variable length)**: The actual data to send in the call

## Capital Management

The contract is configured to use up to $100,000 USD worth of capital for trades. This is controlled by the Loom bot configuration.

## Security Considerations

- The contract owner can withdraw funds and set approvals
- Only the Balancer Vault can call the flash loan callback
- The contract uses ReentrancyGuard to prevent reentrancy attacks
- The contract uses SafeERC20 for token transfers

## Maintenance

- Regularly check token approvals and update if needed
- Monitor gas usage and optimize if necessary
- Keep the contract funded with sufficient ETH for gas

## Troubleshooting

If you encounter issues:

1. Check the contract has sufficient ETH for gas
2. Verify token approvals are set correctly
3. Ensure the Loom bot configuration is pointing to the correct contract address
4. Check the logs for any errors during execution# LoomMulticaller Contract

This contract is designed to work with the Loom bot for executing arbitrage and backrunning trades on the Base network. It's optimized for fast execution and supports complex trade paths.

## Features

- Executes complex trade paths with multiple hops
- Supports flash loans from Balancer
- Handles callbacks from various DEX protocols (Uniswap V2/V3, etc.)
- Optimized for gas efficiency and fast execution
- Supports stack manipulation for complex operations
- Extracts profits automatically

## Prerequisites

- Node.js and npm installed
- Solidity compiler (solc) version 0.8.19 or higher
- Ethereum development environment (Hardhat, Truffle, or similar)
- Private key with ETH on Base network for deployment

## Deployment

1. Compile the contract:

```bash
solc --optimize --optimize-runs=200 --bin --abi LoomMulticaller.sol -o ./
```

2. Set your private key as an environment variable:

```bash
export PRIVATE_KEY=your_private_key_here
```

3. Run the deployment script:

```bash
node deploy_multicaller.js
```

4. Update the `config_base.toml` file with the new contract address (the script should do this automatically).

5. Fund the contract with ETH for gas and initial capital.

## Testing

Run the test script to verify the contract works correctly:

```bash
node test_multicaller.js
```

## Integration with Loom Bot

The contract is designed to work seamlessly with the Loom bot. The bot will:

1. Identify profitable trade opportunities
2. Encode the trade paths into opcodes
3. Send the encoded data to the contract's `doCalls` function
4. The contract will execute the trades and extract profits

## Contract Interface

The main function used by the Loom bot is:

```solidity
function doCalls(bytes calldata data) external payable returns (uint256);
```

This function takes encoded opcodes as input and returns the profit amount.

## Opcode Format

The opcodes are encoded in the following format:

1. **Header (12 bytes)**:
   - Selector (2 bytes): Indicates the call type
   - Call stack info (3 bytes): For stack manipulation
   - Return stack info (3 bytes): For stack manipulation
   - Call data length (2 bytes): Length of the call data

2. **Target Address (20 bytes)**: The address to call (except for calculation and internal calls)

3. **Call Data (variable length)**: The actual data to send in the call

## Capital Management

The contract is configured to use up to $100,000 USD worth of capital for trades. This is controlled by the Loom bot configuration.

## Security Considerations

- The contract owner can withdraw funds and set approvals
- Only the Balancer Vault can call the flash loan callback
- The contract uses ReentrancyGuard to prevent reentrancy attacks
- The contract uses SafeERC20 for token transfers

## Maintenance

- Regularly check token approvals and update if needed
- Monitor gas usage and optimize if necessary
- Keep the contract funded with sufficient ETH for gas

## Troubleshooting

If you encounter issues:

1. Check the contract has sufficient ETH for gas
2. Verify token approvals are set correctly
3. Ensure the Loom bot configuration is pointing to the correct contract address
4. Check the logs for any errors during execution