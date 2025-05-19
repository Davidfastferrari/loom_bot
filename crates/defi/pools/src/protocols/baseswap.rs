use alloy::primitives::Address;
use loom_types_entities::PoolClass;

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

    pub fn init_code_hash(&self) -> [u8; 32] {
        // BaseSwap init code hash
        hex::decode("f4ccce374816856d11f00e4069e7cada164065686fbef53c6167a63ec2fd8c5b")
            .unwrap()
            .try_into()
            .unwrap()
    }

    pub fn supports_chain_id(&self, chain_id: u64) -> bool {
        // Base chain ID
        chain_id == 8453
    }
    
    pub fn get_pool_address_for_tokens(token0: Address, token1: Address) -> Address {
        let factory = "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".parse().unwrap();
        let init_code_hash = hex::decode("f4ccce374816856d11f00e4069e7cada164065686fbef53c6167a63ec2fd8c5b")
            .unwrap()
            .try_into()
            .unwrap();
        get_uniswap2pool_address(token0, token1, factory, init_code_hash)
    }
}

impl Protocol for BaseSwapV2Protocol {
    fn get_pool_address_vec_for_tokens(token0: Address, token1: Address) -> Vec<Address> {
        vec![BaseSwapV2Protocol::get_pool_address_for_tokens(token0, token1)]
    }
}