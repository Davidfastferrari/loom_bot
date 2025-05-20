use alloy::primitives::Address;
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

    pub fn init_code_hash(&self) -> [u8; 32] {
        // Aerodrome init code hash
        let bytes = hex::decode("9b3ee7f1379a9d34cdb09c1030616cb1c9c04cf9f7a3c4af8d1e6e3c9ce108e3").unwrap();
        let mut result = [0u8; 32];
        result.copy_from_slice(&bytes);
        result
    }

    pub fn supports_chain_id(&self, chain_id: u64) -> bool {
        // Base chain ID
        chain_id == 8453
    }
    
    pub fn get_pool_address_for_tokens(token0: Address, token1: Address) -> Address {
        let factory = "0x420DD381b31aEf6683db6B902084cB0FFECe40Da".parse().unwrap();
        let bytes = hex::decode("9b3ee7f1379a9d34cdb09c1030616cb1c9c04cf9f7a3c4af8d1e6e3c9ce108e3").unwrap();
        let mut init_code_hash = [0u8; 32];
        init_code_hash.copy_from_slice(&bytes);
        get_uniswap2pool_address(token0, token1, factory, init_code_hash)
    }
}

impl Protocol for AerodromeProtocol {
    fn get_pool_address_vec_for_tokens(token0: Address, token1: Address) -> Vec<Address> {
        vec![AerodromeProtocol::get_pool_address_for_tokens(token0, token1)]
    }
}