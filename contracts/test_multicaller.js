// Test script for LoomMulticaller contract
const { ethers } = require('ethers');
const fs = require('fs');

// Load contract ABI
const contractABI = JSON.parse(fs.readFileSync('./LoomMulticaller.abi.json', 'utf8'));

// Configuration
const config = {
  rpcUrl: 'https://mainnet.base.org',
  privateKey: process.env.PRIVATE_KEY, // Set your private key as an environment variable
  contractAddress: '0x3dd35b4da6534230ff53048f7477f17f7f4e7a70', // Update with your deployed contract address
  tokens: {
    WETH: '0x4200000000000000000000000000000000000006',
    USDC: '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',
    USDT: '0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA',
    DAI: '0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb'
  },
  dexRouters: {
    uniswapV3: '0x2626664c2603336E57B271c5C0b26F421741e481', // Base Uniswap V3 Router
    baseswap: '0x327Df1E6de05895d2ab08513aaDD9313Fe505d86', // BaseSwap Router
    aerodrome: '0xcF77a3Ba9A5CA399B7c97c74d54e5b1Beb874E43'  // Aerodrome Router
  }
};

// Connect to provider and wallet
const provider = new ethers.providers.JsonRpcProvider(config.rpcUrl);
const wallet = new ethers.Wallet(config.privateKey, provider);
const contract = new ethers.Contract(config.contractAddress, contractABI, wallet);

// Test functions
async function testContractSetup() {
  console.log('Testing contract setup...');
  
  // Set approvals for DEX routers
  const tokens = Object.values(config.tokens);
  const routers = Object.values(config.dexRouters);
  
  const tx = await contract.setApprovals(tokens, routers);
  await tx.wait();
  
  console.log('Approvals set successfully');
}

async function testSimpleSwap() {
  console.log('Testing simple swap...');
  
  // Create a simple swap that swaps WETH for USDC on Uniswap V3
  // This is a simplified example - in practice, the Loom bot would generate this data
  
  // Function selector for Uniswap V3 exactInputSingle
  const uniswapV3ExactInputSingleSelector = '0x414bf389';
  
  // Encode parameters for exactInputSingle
  const params = ethers.utils.defaultAbiCoder.encode(
    ['address', 'address', 'uint24', 'address', 'uint256', 'uint256', 'uint256', 'uint160'],
    [
      config.tokens.WETH,                // tokenIn
      config.tokens.USDC,                // tokenOut
      500,                               // fee (0.05%)
      config.contractAddress,            // recipient
      ethers.utils.parseEther('0.01'),   // amountIn
      0,                                 // amountOutMinimum
      0,                                 // sqrtPriceLimitX96
      0                                  // deadline
    ]
  );
  
  // Combine selector and parameters
  const uniswapCalldata = uniswapV3ExactInputSingleSelector + params.slice(2);
  
  // Create opcode for the call
  // This is a simplified version - the actual encoding would be done by the Loom bot
  const opcode = encodeZeroValueCall(
    config.dexRouters.uniswapV3,
    uniswapCalldata,
    0xFFFFFF,  // No call stack
    0xFFFFFF   // No return stack
  );
  
  // Wrap the opcode in doCalls
  const doCallsData = ethers.utils.defaultAbiCoder.encode(['bytes'], [opcode]);
  const doCallsSelector = '0x2636f943'; // doCalls selector
  const finalCalldata = doCallsSelector + doCallsData.slice(2);
  
  // Send the transaction
  const tx = await wallet.sendTransaction({
    to: config.contractAddress,
    data: finalCalldata,
    value: ethers.utils.parseEther('0.01')
  });
  
  const receipt = await tx.wait();
  console.log('Transaction successful:', receipt.transactionHash);
}

// Helper function to encode a zero value call opcode
function encodeZeroValueCall(target, callData, callStackInfo, returnStackInfo) {
  // Convert callData to bytes
  const callDataBytes = ethers.utils.arrayify(callData);
  
  // Create header
  const selector = 0x7FFC; // ZERO_VALUE_CALL_SELECTOR
  const callDataLength = callDataBytes.length;
  
  // Encode header (12 bytes)
  const header = ethers.utils.hexZeroPad(
    ethers.BigNumber.from(selector).shl(80)
      .or(ethers.BigNumber.from(callStackInfo).shl(16))
      .or(ethers.BigNumber.from(returnStackInfo).shl(40))
      .or(ethers.BigNumber.from(callDataLength))
      .toHexString(),
    12
  );
  
  // Encode target address (20 bytes)
  const targetBytes = ethers.utils.hexZeroPad(target, 20);
  
  // Combine everything
  return header + targetBytes.slice(2) + callData.slice(2);
}

// Main function
async function main() {
  try {
    await testContractSetup();
    await testSimpleSwap();
    console.log('All tests completed successfully');
  } catch (error) {
    console.error('Error during testing:', error);
  }
}

// Run the tests
main();// Test script for LoomMulticaller contract
const { ethers } = require('ethers');
const fs = require('fs');

// Load contract ABI
const contractABI = JSON.parse(fs.readFileSync('./LoomMulticaller.abi.json', 'utf8'));

// Configuration
const config = {
  rpcUrl: 'https://mainnet.base.org',
  privateKey: process.env.PRIVATE_KEY, // Set your private key as an environment variable
  contractAddress: '0x3dd35b4da6534230ff53048f7477f17f7f4e7a70', // Update with your deployed contract address
  tokens: {
    WETH: '0x4200000000000000000000000000000000000006',
    USDC: '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',
    USDT: '0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA',
    DAI: '0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb'
  },
  dexRouters: {
    uniswapV3: '0x2626664c2603336E57B271c5C0b26F421741e481', // Base Uniswap V3 Router
    baseswap: '0x327Df1E6de05895d2ab08513aaDD9313Fe505d86', // BaseSwap Router
    aerodrome: '0xcF77a3Ba9A5CA399B7c97c74d54e5b1Beb874E43'  // Aerodrome Router
  }
};

// Connect to provider and wallet
const provider = new ethers.providers.JsonRpcProvider(config.rpcUrl);
const wallet = new ethers.Wallet(config.privateKey, provider);
const contract = new ethers.Contract(config.contractAddress, contractABI, wallet);

// Test functions
async function testContractSetup() {
  console.log('Testing contract setup...');
  
  // Set approvals for DEX routers
  const tokens = Object.values(config.tokens);
  const routers = Object.values(config.dexRouters);
  
  const tx = await contract.setApprovals(tokens, routers);
  await tx.wait();
  
  console.log('Approvals set successfully');
}

async function testSimpleSwap() {
  console.log('Testing simple swap...');
  
  // Create a simple swap that swaps WETH for USDC on Uniswap V3
  // This is a simplified example - in practice, the Loom bot would generate this data
  
  // Function selector for Uniswap V3 exactInputSingle
  const uniswapV3ExactInputSingleSelector = '0x414bf389';
  
  // Encode parameters for exactInputSingle
  const params = ethers.utils.defaultAbiCoder.encode(
    ['address', 'address', 'uint24', 'address', 'uint256', 'uint256', 'uint256', 'uint160'],
    [
      config.tokens.WETH,                // tokenIn
      config.tokens.USDC,                // tokenOut
      500,                               // fee (0.05%)
      config.contractAddress,            // recipient
      ethers.utils.parseEther('0.01'),   // amountIn
      0,                                 // amountOutMinimum
      0,                                 // sqrtPriceLimitX96
      0                                  // deadline
    ]
  );
  
  // Combine selector and parameters
  const uniswapCalldata = uniswapV3ExactInputSingleSelector + params.slice(2);
  
  // Create opcode for the call
  // This is a simplified version - the actual encoding would be done by the Loom bot
  const opcode = encodeZeroValueCall(
    config.dexRouters.uniswapV3,
    uniswapCalldata,
    0xFFFFFF,  // No call stack
    0xFFFFFF   // No return stack
  );
  
  // Wrap the opcode in doCalls
  const doCallsData = ethers.utils.defaultAbiCoder.encode(['bytes'], [opcode]);
  const doCallsSelector = '0x2636f943'; // doCalls selector
  const finalCalldata = doCallsSelector + doCallsData.slice(2);
  
  // Send the transaction
  const tx = await wallet.sendTransaction({
    to: config.contractAddress,
    data: finalCalldata,
    value: ethers.utils.parseEther('0.01')
  });
  
  const receipt = await tx.wait();
  console.log('Transaction successful:', receipt.transactionHash);
}

// Helper function to encode a zero value call opcode
function encodeZeroValueCall(target, callData, callStackInfo, returnStackInfo) {
  // Convert callData to bytes
  const callDataBytes = ethers.utils.arrayify(callData);
  
  // Create header
  const selector = 0x7FFC; // ZERO_VALUE_CALL_SELECTOR
  const callDataLength = callDataBytes.length;
  
  // Encode header (12 bytes)
  const header = ethers.utils.hexZeroPad(
    ethers.BigNumber.from(selector).shl(80)
      .or(ethers.BigNumber.from(callStackInfo).shl(16))
      .or(ethers.BigNumber.from(returnStackInfo).shl(40))
      .or(ethers.BigNumber.from(callDataLength))
      .toHexString(),
    12
  );
  
  // Encode target address (20 bytes)
  const targetBytes = ethers.utils.hexZeroPad(target, 20);
  
  // Combine everything
  return header + targetBytes.slice(2) + callData.slice(2);
}

// Main function
async function main() {
  try {
    await testContractSetup();
    await testSimpleSwap();
    console.log('All tests completed successfully');
  } catch (error) {
    console.error('Error during testing:', error);
  }
}

// Run the tests
main();// Test script for LoomMulticaller contract
const { ethers } = require('ethers');
const fs = require('fs');

// Load contract ABI
const contractABI = JSON.parse(fs.readFileSync('./LoomMulticaller.abi.json', 'utf8'));

// Configuration
const config = {
  rpcUrl: 'https://mainnet.base.org',
  privateKey: process.env.PRIVATE_KEY, // Set your private key as an environment variable
  contractAddress: '0x3dd35b4da6534230ff53048f7477f17f7f4e7a70', // Update with your deployed contract address
  tokens: {
    WETH: '0x4200000000000000000000000000000000000006',
    USDC: '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',
    USDT: '0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA',
    DAI: '0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb'
  },
  dexRouters: {
    uniswapV3: '0x2626664c2603336E57B271c5C0b26F421741e481', // Base Uniswap V3 Router
    baseswap: '0x327Df1E6de05895d2ab08513aaDD9313Fe505d86', // BaseSwap Router
    aerodrome: '0xcF77a3Ba9A5CA399B7c97c74d54e5b1Beb874E43'  // Aerodrome Router
  }
};

// Connect to provider and wallet
const provider = new ethers.providers.JsonRpcProvider(config.rpcUrl);
const wallet = new ethers.Wallet(config.privateKey, provider);
const contract = new ethers.Contract(config.contractAddress, contractABI, wallet);

// Test functions
async function testContractSetup() {
  console.log('Testing contract setup...');
  
  // Set approvals for DEX routers
  const tokens = Object.values(config.tokens);
  const routers = Object.values(config.dexRouters);
  
  const tx = await contract.setApprovals(tokens, routers);
  await tx.wait();
  
  console.log('Approvals set successfully');
}

async function testSimpleSwap() {
  console.log('Testing simple swap...');
  
  // Create a simple swap that swaps WETH for USDC on Uniswap V3
  // This is a simplified example - in practice, the Loom bot would generate this data
  
  // Function selector for Uniswap V3 exactInputSingle
  const uniswapV3ExactInputSingleSelector = '0x414bf389';
  
  // Encode parameters for exactInputSingle
  const params = ethers.utils.defaultAbiCoder.encode(
    ['address', 'address', 'uint24', 'address', 'uint256', 'uint256', 'uint256', 'uint160'],
    [
      config.tokens.WETH,                // tokenIn
      config.tokens.USDC,                // tokenOut
      500,                               // fee (0.05%)
      config.contractAddress,            // recipient
      ethers.utils.parseEther('0.01'),   // amountIn
      0,                                 // amountOutMinimum
      0,                                 // sqrtPriceLimitX96
      0                                  // deadline
    ]
  );
  
  // Combine selector and parameters
  const uniswapCalldata = uniswapV3ExactInputSingleSelector + params.slice(2);
  
  // Create opcode for the call
  // This is a simplified version - the actual encoding would be done by the Loom bot
  const opcode = encodeZeroValueCall(
    config.dexRouters.uniswapV3,
    uniswapCalldata,
    0xFFFFFF,  // No call stack
    0xFFFFFF   // No return stack
  );
  
  // Wrap the opcode in doCalls
  const doCallsData = ethers.utils.defaultAbiCoder.encode(['bytes'], [opcode]);
  const doCallsSelector = '0x2636f943'; // doCalls selector
  const finalCalldata = doCallsSelector + doCallsData.slice(2);
  
  // Send the transaction
  const tx = await wallet.sendTransaction({
    to: config.contractAddress,
    data: finalCalldata,
    value: ethers.utils.parseEther('0.01')
  });
  
  const receipt = await tx.wait();
  console.log('Transaction successful:', receipt.transactionHash);
}

// Helper function to encode a zero value call opcode
function encodeZeroValueCall(target, callData, callStackInfo, returnStackInfo) {
  // Convert callData to bytes
  const callDataBytes = ethers.utils.arrayify(callData);
  
  // Create header
  const selector = 0x7FFC; // ZERO_VALUE_CALL_SELECTOR
  const callDataLength = callDataBytes.length;
  
  // Encode header (12 bytes)
  const header = ethers.utils.hexZeroPad(
    ethers.BigNumber.from(selector).shl(80)
      .or(ethers.BigNumber.from(callStackInfo).shl(16))
      .or(ethers.BigNumber.from(returnStackInfo).shl(40))
      .or(ethers.BigNumber.from(callDataLength))
      .toHexString(),
    12
  );
  
  // Encode target address (20 bytes)
  const targetBytes = ethers.utils.hexZeroPad(target, 20);
  
  // Combine everything
  return header + targetBytes.slice(2) + callData.slice(2);
}

// Main function
async function main() {
  try {
    await testContractSetup();
    await testSimpleSwap();
    console.log('All tests completed successfully');
  } catch (error) {
    console.error('Error during testing:', error);
  }
}

// Run the tests
main();// Test script for LoomMulticaller contract
const { ethers } = require('ethers');
const fs = require('fs');

// Load contract ABI
const contractABI = JSON.parse(fs.readFileSync('./LoomMulticaller.abi.json', 'utf8'));

// Configuration
const config = {
  rpcUrl: 'https://mainnet.base.org',
  privateKey: process.env.PRIVATE_KEY, // Set your private key as an environment variable
  contractAddress: '0x3dd35b4da6534230ff53048f7477f17f7f4e7a70', // Update with your deployed contract address
  tokens: {
    WETH: '0x4200000000000000000000000000000000000006',
    USDC: '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',
    USDT: '0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA',
    DAI: '0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb'
  },
  dexRouters: {
    uniswapV3: '0x2626664c2603336E57B271c5C0b26F421741e481', // Base Uniswap V3 Router
    baseswap: '0x327Df1E6de05895d2ab08513aaDD9313Fe505d86', // BaseSwap Router
    aerodrome: '0xcF77a3Ba9A5CA399B7c97c74d54e5b1Beb874E43'  // Aerodrome Router
  }
};

// Connect to provider and wallet
const provider = new ethers.providers.JsonRpcProvider(config.rpcUrl);
const wallet = new ethers.Wallet(config.privateKey, provider);
const contract = new ethers.Contract(config.contractAddress, contractABI, wallet);

// Test functions
async function testContractSetup() {
  console.log('Testing contract setup...');
  
  // Set approvals for DEX routers
  const tokens = Object.values(config.tokens);
  const routers = Object.values(config.dexRouters);
  
  const tx = await contract.setApprovals(tokens, routers);
  await tx.wait();
  
  console.log('Approvals set successfully');
}

async function testSimpleSwap() {
  console.log('Testing simple swap...');
  
  // Create a simple swap that swaps WETH for USDC on Uniswap V3
  // This is a simplified example - in practice, the Loom bot would generate this data
  
  // Function selector for Uniswap V3 exactInputSingle
  const uniswapV3ExactInputSingleSelector = '0x414bf389';
  
  // Encode parameters for exactInputSingle
  const params = ethers.utils.defaultAbiCoder.encode(
    ['address', 'address', 'uint24', 'address', 'uint256', 'uint256', 'uint256', 'uint160'],
    [
      config.tokens.WETH,                // tokenIn
      config.tokens.USDC,                // tokenOut
      500,                               // fee (0.05%)
      config.contractAddress,            // recipient
      ethers.utils.parseEther('0.01'),   // amountIn
      0,                                 // amountOutMinimum
      0,                                 // sqrtPriceLimitX96
      0                                  // deadline
    ]
  );
  
  // Combine selector and parameters
  const uniswapCalldata = uniswapV3ExactInputSingleSelector + params.slice(2);
  
  // Create opcode for the call
  // This is a simplified version - the actual encoding would be done by the Loom bot
  const opcode = encodeZeroValueCall(
    config.dexRouters.uniswapV3,
    uniswapCalldata,
    0xFFFFFF,  // No call stack
    0xFFFFFF   // No return stack
  );
  
  // Wrap the opcode in doCalls
  const doCallsData = ethers.utils.defaultAbiCoder.encode(['bytes'], [opcode]);
  const doCallsSelector = '0x2636f943'; // doCalls selector
  const finalCalldata = doCallsSelector + doCallsData.slice(2);
  
  // Send the transaction
  const tx = await wallet.sendTransaction({
    to: config.contractAddress,
    data: finalCalldata,
    value: ethers.utils.parseEther('0.01')
  });
  
  const receipt = await tx.wait();
  console.log('Transaction successful:', receipt.transactionHash);
}

// Helper function to encode a zero value call opcode
function encodeZeroValueCall(target, callData, callStackInfo, returnStackInfo) {
  // Convert callData to bytes
  const callDataBytes = ethers.utils.arrayify(callData);
  
  // Create header
  const selector = 0x7FFC; // ZERO_VALUE_CALL_SELECTOR
  const callDataLength = callDataBytes.length;
  
  // Encode header (12 bytes)
  const header = ethers.utils.hexZeroPad(
    ethers.BigNumber.from(selector).shl(80)
      .or(ethers.BigNumber.from(callStackInfo).shl(16))
      .or(ethers.BigNumber.from(returnStackInfo).shl(40))
      .or(ethers.BigNumber.from(callDataLength))
      .toHexString(),
    12
  );
  
  // Encode target address (20 bytes)
  const targetBytes = ethers.utils.hexZeroPad(target, 20);
  
  // Combine everything
  return header + targetBytes.slice(2) + callData.slice(2);
}

// Main function
async function main() {
  try {
    await testContractSetup();
    await testSimpleSwap();
    console.log('All tests completed successfully');
  } catch (error) {
    console.error('Error during testing:', error);
  }
}

// Run the tests
main();