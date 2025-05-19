use alloy_primitives::Address;
use loom_types_entities::PoolClass;

use crate::protocols::protocol::Protocol;

pub struct AerodromeProtocol {}

impl Protocol for AerodromeProtocol {
    fn name(&self) -> &'static str {
        "aerodrome"
    }

    fn pool_class(&self) -> PoolClass {
        PoolClass::UniswapV2
    }

    fn factory_address(&self) -> Address {
        // Aerodrome factory address on Base
        "0x420DD381b31aEf6683db6B902084cB0FFECe40Da".parse().unwrap()
    }

    fn init_code_hash(&self) -> [u8; 32] {
        // Aerodrome init code hash
        hex::decode("9b3ee7f1379a9d34cdb09c1030616cb1c9c04cf9f7a3c4af8d1e6e3c9ce108e3")
            .unwrap()
            .try_into()
            .unwrap()
    }

    fn supports_chain_id(&self, chain_id: u64) -> bool {
        // Base chain ID
        chain_id == 8453
    }
}