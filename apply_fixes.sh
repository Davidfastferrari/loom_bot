#!/bin/bash

# Fix aerodrome.rs
cp -f crates/defi/pools/src/protocols/aerodrome.rs crates/defi/pools/src/protocols/aerodrome.rs.bak
cat > crates/defi/pools/src/protocols/aerodrome.rs << 'EOF'
use alloy::primitives::{Address, B256};
use loom_types_entities::PoolClass;
use hex;

use crate::protocols::helper::get_uniswap2pool_address;
use crate::protocols::protocol::Protocol;

pub struct AerodromeProtocol {}

impl AerodromeProtocol {
    pub fn name(&self) -> &'static str {
        "aerodrome"
    }

    pub fn pool_class(&self) -> PoolClass {
        PoolClass::UniswapV2
    }

    pub fn factory_address(&self) -> Address {
        // Aerodrome factory address on Base
        "0x420DD381b31aEf6683db6B902084cB0FFECe40Da".parse().unwrap()
    }

    pub fn init_code_hash(&self) -> B256 {
        // Aerodrome init code hash
        "9b3ee7f1379a9d34cdb09c1030616cb1c9c04cf9f7a3c4af8d1e6e3c9ce108e3".parse().unwrap()
    }

    pub fn supports_chain_id(&self, chain_id: u64) -> bool {
        // Base chain ID
        chain_id == 8453
    }
    
    pub fn get_pool_address_for_tokens(token0: Address, token1: Address) -> Address {
        let factory = "0x420DD381b31aEf6683db6B902084cB0FFECe40Da".parse().unwrap();
        let init_code_hash: B256 = "9b3ee7f1379a9d34cdb09c1030616cb1c9c04cf9f7a3c4af8d1e6e3c9ce108e3".parse().unwrap();
        get_uniswap2pool_address(token0, token1, factory, init_code_hash)
    }
}

impl Protocol for AerodromeProtocol {
    fn get_pool_address_vec_for_tokens(token0: Address, token1: Address) -> Vec<Address> {
        vec![AerodromeProtocol::get_pool_address_for_tokens(token0, token1)]
    }
}
EOF

# Fix baseswap.rs
cp -f crates/defi/pools/src/protocols/baseswap.rs crates/defi/pools/src/protocols/baseswap.rs.bak
cat > crates/defi/pools/src/protocols/baseswap.rs << 'EOF'
use alloy::primitives::{Address, B256};
use loom_types_entities::PoolClass;
use hex;

use crate::protocols::helper::get_uniswap2pool_address;
use crate::protocols::protocol::Protocol;

pub struct BaseSwapV2Protocol {}

impl BaseSwapV2Protocol {
    pub fn name(&self) -> &'static str {
        "baseswap_v2"
    }

    pub fn pool_class(&self) -> PoolClass {
        PoolClass::UniswapV2
    }

    pub fn factory_address(&self) -> Address {
        // BaseSwap factory address on Base
        "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".parse().unwrap()
    }

    pub fn init_code_hash(&self) -> B256 {
        // BaseSwap init code hash
        "f4ccce374816856d11f00e4069e7cada164065686fbef53c6167a63ec2fd8c5b".parse().unwrap()
    }

    pub fn supports_chain_id(&self, chain_id: u64) -> bool {
        // Base chain ID
        chain_id == 8453
    }
    
    pub fn get_pool_address_for_tokens(token0: Address, token1: Address) -> Address {
        let factory = "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".parse().unwrap();
        let init_code_hash: B256 = "f4ccce374816856d11f00e4069e7cada164065686fbef53c6167a63ec2fd8c5b".parse().unwrap();
        get_uniswap2pool_address(token0, token1, factory, init_code_hash)
    }
}

impl Protocol for BaseSwapV2Protocol {
    fn get_pool_address_vec_for_tokens(token0: Address, token1: Address) -> Vec<Address> {
        vec![BaseSwapV2Protocol::get_pool_address_for_tokens(token0, token1)]
    }
}
EOF

echo "Fixes applied successfully!"#!/bin/bash

# Fix aerodrome.rs
cp -f crates/defi/pools/src/protocols/aerodrome.rs crates/defi/pools/src/protocols/aerodrome.rs.bak
cat > crates/defi/pools/src/protocols/aerodrome.rs << 'EOF'
use alloy::primitives::{Address, B256};
use loom_types_entities::PoolClass;
use hex;

use crate::protocols::helper::get_uniswap2pool_address;
use crate::protocols::protocol::Protocol;

pub struct AerodromeProtocol {}

impl AerodromeProtocol {
    pub fn name(&self) -> &'static str {
        "aerodrome"
    }

    pub fn pool_class(&self) -> PoolClass {
        PoolClass::UniswapV2
    }

    pub fn factory_address(&self) -> Address {
        // Aerodrome factory address on Base
        "0x420DD381b31aEf6683db6B902084cB0FFECe40Da".parse().unwrap()
    }

    pub fn init_code_hash(&self) -> B256 {
        // Aerodrome init code hash
        "9b3ee7f1379a9d34cdb09c1030616cb1c9c04cf9f7a3c4af8d1e6e3c9ce108e3".parse().unwrap()
    }

    pub fn supports_chain_id(&self, chain_id: u64) -> bool {
        // Base chain ID
        chain_id == 8453
    }
    
    pub fn get_pool_address_for_tokens(token0: Address, token1: Address) -> Address {
        let factory = "0x420DD381b31aEf6683db6B902084cB0FFECe40Da".parse().unwrap();
        let init_code_hash: B256 = "9b3ee7f1379a9d34cdb09c1030616cb1c9c04cf9f7a3c4af8d1e6e3c9ce108e3".parse().unwrap();
        get_uniswap2pool_address(token0, token1, factory, init_code_hash)
    }
}

impl Protocol for AerodromeProtocol {
    fn get_pool_address_vec_for_tokens(token0: Address, token1: Address) -> Vec<Address> {
        vec![AerodromeProtocol::get_pool_address_for_tokens(token0, token1)]
    }
}
EOF

# Fix baseswap.rs
cp -f crates/defi/pools/src/protocols/baseswap.rs crates/defi/pools/src/protocols/baseswap.rs.bak
cat > crates/defi/pools/src/protocols/baseswap.rs << 'EOF'
use alloy::primitives::{Address, B256};
use loom_types_entities::PoolClass;
use hex;

use crate::protocols::helper::get_uniswap2pool_address;
use crate::protocols::protocol::Protocol;

pub struct BaseSwapV2Protocol {}

impl BaseSwapV2Protocol {
    pub fn name(&self) -> &'static str {
        "baseswap_v2"
    }

    pub fn pool_class(&self) -> PoolClass {
        PoolClass::UniswapV2
    }

    pub fn factory_address(&self) -> Address {
        // BaseSwap factory address on Base
        "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".parse().unwrap()
    }

    pub fn init_code_hash(&self) -> B256 {
        // BaseSwap init code hash
        "f4ccce374816856d11f00e4069e7cada164065686fbef53c6167a63ec2fd8c5b".parse().unwrap()
    }

    pub fn supports_chain_id(&self, chain_id: u64) -> bool {
        // Base chain ID
        chain_id == 8453
    }
    
    pub fn get_pool_address_for_tokens(token0: Address, token1: Address) -> Address {
        let factory = "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".parse().unwrap();
        let init_code_hash: B256 = "f4ccce374816856d11f00e4069e7cada164065686fbef53c6167a63ec2fd8c5b".parse().unwrap();
        get_uniswap2pool_address(token0, token1, factory, init_code_hash)
    }
}

impl Protocol for BaseSwapV2Protocol {
    fn get_pool_address_vec_for_tokens(token0: Address, token1: Address) -> Vec<Address> {
        vec![BaseSwapV2Protocol::get_pool_address_for_tokens(token0, token1)]
    }
}
EOF

echo "Fixes applied successfully!"