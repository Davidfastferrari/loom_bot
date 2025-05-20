@echo off
echo Fixing protocol files...

REM Fix aerodrome.rs
echo use alloy::primitives::{Address, B256}; > c:\loom_bot\aerodrome_fixed.txt
echo use loom_types_entities::PoolClass; >> c:\loom_bot\aerodrome_fixed.txt
echo use hex; >> c:\loom_bot\aerodrome_fixed.txt
echo. >> c:\loom_bot\aerodrome_fixed.txt
echo use crate::protocols::helper::get_uniswap2pool_address; >> c:\loom_bot\aerodrome_fixed.txt
echo use crate::protocols::protocol::Protocol; >> c:\loom_bot\aerodrome_fixed.txt
echo. >> c:\loom_bot\aerodrome_fixed.txt
echo pub struct AerodromeProtocol {} >> c:\loom_bot\aerodrome_fixed.txt
echo. >> c:\loom_bot\aerodrome_fixed.txt
echo impl AerodromeProtocol { >> c:\loom_bot\aerodrome_fixed.txt
echo     pub fn name(^&self) -^> ^&'static str { >> c:\loom_bot\aerodrome_fixed.txt
echo         "aerodrome" >> c:\loom_bot\aerodrome_fixed.txt
echo     } >> c:\loom_bot\aerodrome_fixed.txt
echo. >> c:\loom_bot\aerodrome_fixed.txt
echo     pub fn pool_class(^&self) -^> PoolClass { >> c:\loom_bot\aerodrome_fixed.txt
echo         PoolClass::UniswapV2 >> c:\loom_bot\aerodrome_fixed.txt
echo     } >> c:\loom_bot\aerodrome_fixed.txt
echo. >> c:\loom_bot\aerodrome_fixed.txt
echo     pub fn factory_address(^&self) -^> Address { >> c:\loom_bot\aerodrome_fixed.txt
echo         // Aerodrome factory address on Base >> c:\loom_bot\aerodrome_fixed.txt
echo         "0x420DD381b31aEf6683db6B902084cB0FFECe40Da".parse().unwrap() >> c:\loom_bot\aerodrome_fixed.txt
echo     } >> c:\loom_bot\aerodrome_fixed.txt
echo. >> c:\loom_bot\aerodrome_fixed.txt
echo     pub fn init_code_hash(^&self) -^> B256 { >> c:\loom_bot\aerodrome_fixed.txt
echo         // Aerodrome init code hash >> c:\loom_bot\aerodrome_fixed.txt
echo         "9b3ee7f1379a9d34cdb09c1030616cb1c9c04cf9f7a3c4af8d1e6e3c9ce108e3".parse().unwrap() >> c:\loom_bot\aerodrome_fixed.txt
echo     } >> c:\loom_bot\aerodrome_fixed.txt
echo. >> c:\loom_bot\aerodrome_fixed.txt
echo     pub fn supports_chain_id(^&self, chain_id: u64) -^> bool { >> c:\loom_bot\aerodrome_fixed.txt
echo         // Base chain ID >> c:\loom_bot\aerodrome_fixed.txt
echo         chain_id == 8453 >> c:\loom_bot\aerodrome_fixed.txt
echo     } >> c:\loom_bot\aerodrome_fixed.txt
echo     >> c:\loom_bot\aerodrome_fixed.txt
echo     pub fn get_pool_address_for_tokens(token0: Address, token1: Address) -^> Address { >> c:\loom_bot\aerodrome_fixed.txt
echo         let factory = "0x420DD381b31aEf6683db6B902084cB0FFECe40Da".parse().unwrap(); >> c:\loom_bot\aerodrome_fixed.txt
echo         let init_code_hash: B256 = "9b3ee7f1379a9d34cdb09c1030616cb1c9c04cf9f7a3c4af8d1e6e3c9ce108e3".parse().unwrap(); >> c:\loom_bot\aerodrome_fixed.txt
echo         get_uniswap2pool_address(token0, token1, factory, init_code_hash) >> c:\loom_bot\aerodrome_fixed.txt
echo     } >> c:\loom_bot\aerodrome_fixed.txt
echo } >> c:\loom_bot\aerodrome_fixed.txt
echo. >> c:\loom_bot\aerodrome_fixed.txt
echo impl Protocol for AerodromeProtocol { >> c:\loom_bot\aerodrome_fixed.txt
echo     fn get_pool_address_vec_for_tokens(token0: Address, token1: Address) -^> Vec^<Address^> { >> c:\loom_bot\aerodrome_fixed.txt
echo         vec![AerodromeProtocol::get_pool_address_for_tokens(token0, token1)] >> c:\loom_bot\aerodrome_fixed.txt
echo     } >> c:\loom_bot\aerodrome_fixed.txt
echo } >> c:\loom_bot\aerodrome_fixed.txt

REM Fix baseswap.rs
echo use alloy::primitives::{Address, B256}; > c:\loom_bot\baseswap_fixed.txt
echo use loom_types_entities::PoolClass; >> c:\loom_bot\baseswap_fixed.txt
echo use hex; >> c:\loom_bot\baseswap_fixed.txt
echo. >> c:\loom_bot\baseswap_fixed.txt
echo use crate::protocols::helper::get_uniswap2pool_address; >> c:\loom_bot\baseswap_fixed.txt
echo use crate::protocols::protocol::Protocol; >> c:\loom_bot\baseswap_fixed.txt
echo. >> c:\loom_bot\baseswap_fixed.txt
echo pub struct BaseSwapV2Protocol {} >> c:\loom_bot\baseswap_fixed.txt
echo. >> c:\loom_bot\baseswap_fixed.txt
echo impl BaseSwapV2Protocol { >> c:\loom_bot\baseswap_fixed.txt
echo     pub fn name(^&self) -^> ^&'static str { >> c:\loom_bot\baseswap_fixed.txt
echo         "baseswap_v2" >> c:\loom_bot\baseswap_fixed.txt
echo     } >> c:\loom_bot\baseswap_fixed.txt
echo. >> c:\loom_bot\baseswap_fixed.txt
echo     pub fn pool_class(^&self) -^> PoolClass { >> c:\loom_bot\baseswap_fixed.txt
echo         PoolClass::UniswapV2 >> c:\loom_bot\baseswap_fixed.txt
echo     } >> c:\loom_bot\baseswap_fixed.txt
echo. >> c:\loom_bot\baseswap_fixed.txt
echo     pub fn factory_address(^&self) -^> Address { >> c:\loom_bot\baseswap_fixed.txt
echo         // BaseSwap factory address on Base >> c:\loom_bot\baseswap_fixed.txt
echo         "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".parse().unwrap() >> c:\loom_bot\baseswap_fixed.txt
echo     } >> c:\loom_bot\baseswap_fixed.txt
echo. >> c:\loom_bot\baseswap_fixed.txt
echo     pub fn init_code_hash(^&self) -^> B256 { >> c:\loom_bot\baseswap_fixed.txt
echo         // BaseSwap init code hash >> c:\loom_bot\baseswap_fixed.txt
echo         "f4ccce374816856d11f00e4069e7cada164065686fbef53c6167a63ec2fd8c5b".parse().unwrap() >> c:\loom_bot\baseswap_fixed.txt
echo     } >> c:\loom_bot\baseswap_fixed.txt
echo. >> c:\loom_bot\baseswap_fixed.txt
echo     pub fn supports_chain_id(^&self, chain_id: u64) -^> bool { >> c:\loom_bot\baseswap_fixed.txt
echo         // Base chain ID >> c:\loom_bot\baseswap_fixed.txt
echo         chain_id == 8453 >> c:\loom_bot\baseswap_fixed.txt
echo     } >> c:\loom_bot\baseswap_fixed.txt
echo     >> c:\loom_bot\baseswap_fixed.txt
echo     pub fn get_pool_address_for_tokens(token0: Address, token1: Address) -^> Address { >> c:\loom_bot\baseswap_fixed.txt
echo         let factory = "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".parse().unwrap(); >> c:\loom_bot\baseswap_fixed.txt
echo         let init_code_hash: B256 = "f4ccce374816856d11f00e4069e7cada164065686fbef53c6167a63ec2fd8c5b".parse().unwrap(); >> c:\loom_bot\baseswap_fixed.txt
echo         get_uniswap2pool_address(token0, token1, factory, init_code_hash) >> c:\loom_bot\baseswap_fixed.txt
echo     } >> c:\loom_bot\baseswap_fixed.txt
echo } >> c:\loom_bot\baseswap_fixed.txt
echo. >> c:\loom_bot\baseswap_fixed.txt
echo impl Protocol for BaseSwapV2Protocol { >> c:\loom_bot\baseswap_fixed.txt
echo     fn get_pool_address_vec_for_tokens(token0: Address, token1: Address) -^> Vec^<Address^> { >> c:\loom_bot\baseswap_fixed.txt
echo         vec![BaseSwapV2Protocol::get_pool_address_for_tokens(token0, token1)] >> c:\loom_bot\baseswap_fixed.txt
echo     } >> c:\loom_bot\baseswap_fixed.txt
echo } >> c:\loom_bot\baseswap_fixed.txt

REM Apply the fixes
copy c:\loom_bot\aerodrome_fixed.txt c:\loom_bot\crates\defi\pools\src\protocols\aerodrome.rs
copy c:\loom_bot\baseswap_fixed.txt c:\loom_bot\crates\defi\pools\src\protocols\baseswap.rs

echo Protocol files fixed successfully!