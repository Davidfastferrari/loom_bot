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
    pub gas_boost_percent: Option<u64>,  // Percentage to boost gas price by
    pub private_tx_enabled: Option<bool>, // Whether to use private transactions
    pub mev_blocker_enabled: Option<bool>, // Whether to use MEV blocker
}

impl Default for BaseNetworkConfig {
    fn default() -> Self {
        Self {
            min_profit_wei: Some(U256::from(1_000_000_000_000_000u64)), // 0.001 ETH
            priority_fee: Some(U256::from(100_000_000u64)), // 0.1 Gwei
            max_gas_price: Some(U256::from(30_000_000_000u64)), // 30 Gwei
            flash_loan_fee_bps: Some(30), // 0.3% flash loan fee
            max_capital_usd: Some(100_000), // $100,000 USD
            gas_boost_percent: Some(10), // 10% gas boost
            private_tx_enabled: Some(false), // Private transactions disabled by default
            mev_blocker_enabled: Some(false), // MEV blocker disabled by default
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
    private_tx_url: Option<String>, // URL for private transaction service
    pub rate_limit_rps: Option<u32>,
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
            chain_id: Some(8453), // Default to Base Network
            base_config: None,
            dynamic_capital: Some(true),
            max_path_length: Some(4),
            private_tx_url: None,
        }
    }
    
    pub fn is_base_network(&self) -> bool {
        self.chain_id.unwrap_or(1) == 8453
    }
    
    pub fn chain_id(&self) -> u64 {
        self.chain_id.unwrap_or(1) // Default to Ethereum mainnet
    }
    
    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = Some(chain_id);
        self
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
    
    // Gas optimization methods
    pub fn gas_boost_percent(&self) -> u64 {
        self.base_config().gas_boost_percent.unwrap_or(10) // Default 10%
    }
    
    pub fn calculate_gas_price(&self, base_gas_price: U256) -> U256 {
        let boost_percent = self.gas_boost_percent();
        let boost_multiplier = 100 + boost_percent;
        
        // Apply boost: base_gas_price * (100 + boost_percent) / 100
        let boosted_gas_price = base_gas_price
            .saturating_mul(U256::from(boost_multiplier))
            / U256::from(100);
        
        // Cap at max gas price if configured
        if let Some(max_gas_price) = self.base_config().max_gas_price {
            if boosted_gas_price > max_gas_price {
                return max_gas_price;
            }
        }
        
        boosted_gas_price
    }
    
    // MEV protection methods
    pub fn private_tx_enabled(&self) -> bool {
        self.base_config().private_tx_enabled.unwrap_or(false)
    }
    
    pub fn private_tx_url(&self) -> Option<String> {
        self.private_tx_url.clone()
    }
    
    pub fn mev_blocker_enabled(&self) -> bool {
        self.base_config().mev_blocker_enabled.unwrap_or(false)
    }
}

impl Default for BackrunConfig {
    fn default() -> Self {
        Self { 
            eoa: None, 
            smart: true,
            chain_id: Some(8453), // Default to Base Network
            base_config: None,
            dynamic_capital: Some(true),
            max_path_length: Some(4),
            private_tx_url: None,
        }
    }
}