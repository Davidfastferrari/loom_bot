use alloy_primitives::{Address, U256};
use loom_types_entities::strategy_config::StrategyConfig;
use serde::Deserialize;

#[derive(Clone, Deserialize, Debug)]
pub struct BackrunConfigSection {
    pub backrun_strategy: BackrunConfig,
}

#[derive(Clone, Deserialize, Debug)]
pub struct BaseNetworkConfig {
    pub min_profit_wei: Option<U256>,
    pub priority_fee: Option<U256>,
    pub max_gas_price: Option<U256>,
    pub flash_loan_fee_bps: Option<u64>, // Basis points (e.g., 30 = 0.3%)
    pub max_capital_usd: Option<u64>,    // Maximum capital in USD
}

impl Default for BaseNetworkConfig {
    fn default() -> Self {
        Self {
            min_profit_wei: Some(U256::from(1_000_000_000_000_000u64)), // 0.001 ETH
            priority_fee: Some(U256::from(100_000_000u64)), // 0.1 Gwei
            max_gas_price: Some(U256::from(30_000_000_000u64)), // 30 Gwei
            flash_loan_fee_bps: Some(30), // 0.3% flash loan fee
            max_capital_usd: Some(100_000), // $100,000 USD
        }
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct BackrunConfig {
    eoa: Option<Address>,
    smart: bool,
    chain_id: Option<u64>,
    base_config: Option<BaseNetworkConfig>,
    dynamic_capital: Option<bool>,
    max_path_length: Option<usize>,
}

impl StrategyConfig for BackrunConfig {
    fn eoa(&self) -> Option<Address> {
        self.eoa
    }
}

impl BackrunConfig {
    pub fn smart(&self) -> bool {
        self.smart
    }

    pub fn new_dumb() -> Self {
        Self { 
            eoa: None, 
            smart: false,
            chain_id: None,
            base_config: None,
            dynamic_capital: Some(true),
            max_path_length: Some(4),
        }
    }
    
    pub fn is_base_network(&self) -> bool {
        self.chain_id.unwrap_or(1) == 8453
    }
    
    pub fn base_config(&self) -> BaseNetworkConfig {
        self.base_config.clone().unwrap_or_default()
    }
    
    pub fn min_profit_wei(&self) -> U256 {
        if self.is_base_network() {
            self.base_config().min_profit_wei.unwrap_or(U256::from(1_000_000_000_000_000u64))
        } else {
            U256::from(1_000_000_000_000_000u64) // Default 0.001 ETH
        }
    }
    
    pub fn flash_loan_fee_bps(&self) -> u64 {
        self.base_config().flash_loan_fee_bps.unwrap_or(30) // Default 0.3%
    }
    
    pub fn max_capital_usd(&self) -> u64 {
        self.base_config().max_capital_usd.unwrap_or(100_000) // Default $100,000 USD
    }
    
    pub fn dynamic_capital(&self) -> bool {
        self.dynamic_capital.unwrap_or(true) // Default to true
    }
    
    pub fn max_path_length(&self) -> usize {
        self.max_path_length.unwrap_or(4) // Default to 4 hops
    }
}

impl Default for BackrunConfig {
    fn default() -> Self {
        Self { 
            eoa: None, 
            smart: true,
            chain_id: None,
            base_config: None,
            dynamic_capital: Some(true),
            max_path_length: Some(4),
        }
    }
}