in // Deploy script for LoomMulticaller contract
const { ethers } = require('ethers');
const fs = require('fs');
const path = require('path');

// Configuration
const config = {
  rpcUrl: 'https://mainnet.base.org',
  privateKey: process.env.PRIVATE_KEY, // Set your private key as an environment variable
  gasPrice: 1000000000, // 1 gwei
  gasLimit: 5000000,
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

// Load contract bytecode and ABI
const contractBytecode = fs.readFileSync('./LoomMulticaller.bin', 'utf8');
const contractABI = JSON.parse(fs.readFileSync('./LoomMulticaller.abi.json', 'utf8'));

// Connect to provider and wallet
const provider = new ethers.providers.JsonRpcProvider(config.rpcUrl);
const wallet = new ethers.Wallet(config.privateKey, provider);

async function deployContract() {
  console.log('Deploying LoomMulticaller contract...');
  
  // Create contract factory
  const factory = new ethers.ContractFactory(contractABI, contractBytecode, wallet);
  
  // Deploy contract
  const contract = await factory.deploy({
    gasPrice: config.gasPrice,
    gasLimit: config.gasLimit
  });
  
  // Wait for deployment to complete
  await contract.deployed();
  
  console.log('Contract deployed at:', contract.address);
  
  // Update config file with new contract address
  updateConfig(contract.address);
  
  // Set approvals for DEX routers
  await setApprovals(contract);
  
  return contract.address;
}

async function setApprovals(contract) {
  console.log('Setting token approvals for DEX routers...');
  
  const tokens = Object.values(config.tokens);
  const routers = Object.values(config.dexRouters);
  
  const tx = await contract.setApprovals(tokens, routers, {
    gasPrice: config.gasPrice,
    gasLimit: config.gasLimit
  });
  
  await tx.wait();
  
  console.log('Approvals set successfully');
}

function updateConfig(contractAddress) {
  console.log('Updating config file with new contract address...');
  
  // Read the current config file
  const configPath = path.resolve(__dirname, '../config_base.toml');
  let configContent = fs.readFileSync(configPath, 'utf8');
  
  // Update the multicaller address
  configContent = configContent.replace(
    /address = "0x[a-fA-F0-9]{40}"/,
    `address = "${contractAddress}"`
  );
  
  // Write the updated config back to the file
  fs.writeFileSync(configPath, configContent);
  
  console.log('Config file updated successfully');
}

// Main function
async function main() {
  try {
    const contractAddress = await deployContract();
    console.log('Deployment completed successfully');
    console.log('Contract address:', contractAddress);
    
    // Instructions for next steps
    console.log('\nNext steps:');
    console.log('1. Update your config_base.toml file with the new contract address');
    console.log('2. Fund the contract with ETH for gas and initial capital');
    console.log('3. Run the test script to verify everything works correctly');
    console.log('4. Start the Loom bot with the updated configuration');
  } catch (error) {
    console.error('Error during deployment:', error);
  }
}

// Run the deployment
main();// Deploy script for LoomMulticaller contract
const { ethers } = require('ethers');
const fs = require('fs');
const path = require('path');

// Configuration
const config = {
  rpcUrl: 'https://mainnet.base.org',
  privateKey: process.env.PRIVATE_KEY, // Set your private key as an environment variable
  gasPrice: 1000000000, // 1 gwei
  gasLimit: 5000000,
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

// Load contract bytecode and ABI
const contractBytecode = fs.readFileSync('./LoomMulticaller.bin', 'utf8');
const contractABI = JSON.parse(fs.readFileSync('./LoomMulticaller.abi.json', 'utf8'));

// Connect to provider and wallet
const provider = new ethers.providers.JsonRpcProvider(config.rpcUrl);
const wallet = new ethers.Wallet(config.privateKey, provider);

async function deployContract() {
  console.log('Deploying LoomMulticaller contract...');
  
  // Create contract factory
  const factory = new ethers.ContractFactory(contractABI, contractBytecode, wallet);
  
  // Deploy contract
  const contract = await factory.deploy({
    gasPrice: config.gasPrice,
    gasLimit: config.gasLimit
  });
  
  // Wait for deployment to complete
  await contract.deployed();
  
  console.log('Contract deployed at:', contract.address);
  
  // Update config file with new contract address
  updateConfig(contract.address);
  
  // Set approvals for DEX routers
  await setApprovals(contract);
  
  return contract.address;
}

async function setApprovals(contract) {
  console.log('Setting token approvals for DEX routers...');
  
  const tokens = Object.values(config.tokens);
  const routers = Object.values(config.dexRouters);
  
  const tx = await contract.setApprovals(tokens, routers, {
    gasPrice: config.gasPrice,
    gasLimit: config.gasLimit
  });
  
  await tx.wait();
  
  console.log('Approvals set successfully');
}

function updateConfig(contractAddress) {
  console.log('Updating config file with new contract address...');
  
  // Read the current config file
  const configPath = path.resolve(__dirname, '../config_base.toml');
  let configContent = fs.readFileSync(configPath, 'utf8');
  
  // Update the multicaller address
  configContent = configContent.replace(
    /address = "0x[a-fA-F0-9]{40}"/,
    `address = "${contractAddress}"`
  );
  
  // Write the updated config back to the file
  fs.writeFileSync(configPath, configContent);
  
  console.log('Config file updated successfully');
}

// Main function
async function main() {
  try {
    const contractAddress = await deployContract();
    console.log('Deployment completed successfully');
    console.log('Contract address:', contractAddress);
    
    // Instructions for next steps
    console.log('\nNext steps:');
    console.log('1. Update your config_base.toml file with the new contract address');
    console.log('2. Fund the contract with ETH for gas and initial capital');
    console.log('3. Run the test script to verify everything works correctly');
    console.log('4. Start the Loom bot with the updated configuration');
  } catch (error) {
    console.error('Error during deployment:', error);
  }
}

// Run the deployment
main();