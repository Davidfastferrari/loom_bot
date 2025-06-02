use alloy_primitives::{Address, U256};
use eyre::{Result, eyre};
use revm::DatabaseRef;
use tracing::{info, debug};
use std::collections::HashMap;

// Token addresses for different networks
// Base Network token addresses
pub const BASE_USDC_ADDRESS: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
pub const BASE_USDT_ADDRESS: &str = "0x4A3A6Dd60A34bB2Aba60D73B4C88315E9CeB6A3D";
pub const BASE_WBTC_ADDRESS: &str = "0x77852193BD608A4523325bAB2e3Cfdb183424F34";
pub const BASE_WETH_ADDRESS: &str = "0x4200000000000000000000000000000000000006";
pub const BASE_DAI_ADDRESS: &str = "0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb";

// Ethereum Mainnet token addresses
pub const ETH_USDC_ADDRESS: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
pub const ETH_USDT_ADDRESS: &str = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
pub const ETH_WBTC_ADDRESS: &str = "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599";
pub const ETH_WETH_ADDRESS: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
pub const ETH_DAI_ADDRESS: &str = "0x6B175474E89094C44Da98b954EedeAC495271d0F";

// Structure to hold profit in multiple currencies
#[derive(Debug, Clone)]
pub struct MultiCurrencyProfit {
    pub eth: U256,
    pub usdc: Option<U256>,
    pub usdt: Option<U256>,
    pub wbtc: Option<U256>,
    pub weth: Option<U256>,
    pub dai: Option<U256>,
}

impl MultiCurrencyProfit {
    pub fn new(eth_profit: U256) -> Self {
        Self {
            eth: eth_profit,
            usdc: None,
            usdt: None,
            wbtc: None,
            weth: None,
            dai: None,
        }
    }

    pub fn log_profits(&self) {
        info!("Profit in ETH: {} wei", self.eth);
        if let Some(usdc) = self.usdc {
            info!("Profit in USDC: {} (6 decimals)", usdc);
        }
        if let Some(usdt) = self.usdt {
            info!("Profit in USDT: {} (6 decimals)", usdt);
        }
        if let Some(wbtc) = self.wbtc {
            info!("Profit in WBTC: {} (8 decimals)", wbtc);
        }
        if let Some(weth) = self.weth {
            info!("Profit in WETH: {} (18 decimals)", weth);
        }
        if let Some(dai) = self.dai {
            info!("Profit in DAI: {} (18 decimals)", dai);
        }
    }
}

pub struct ProfitCalculator {}

impl ProfitCalculator {
    // Calculate profit in multiple currencies
    pub async fn calculate_multi_currency_profit<DB: DatabaseRef>(
        eth_profit: U256,
        _market_state: &DB,
        chain_id: Option<u64>,
    ) -> Result<MultiCurrencyProfit> {
        let mut profit = MultiCurrencyProfit::new(eth_profit);
        
        // Determine which network we're on based on chain ID
        let is_base_network = chain_id.unwrap_or(1) == 8453;
        
        // Calculate profits based on network
        if is_base_network {
            Self::calculate_base_network_profits(&mut profit, eth_profit).await?;
        } else {
            Self::calculate_ethereum_profits(&mut profit, eth_profit).await?;
        }
        
        // Calculate USD value
        let eth_price_usd = 2000.0; // $2000 per ETH (placeholder)
        let eth_amount = eth_profit.to_string().parse::<f64>().unwrap_or(0.0) / 1e18;
        let usd_value = eth_amount * eth_price_usd;
        
        info!("Total profit value: ${} USD", usd_value.round());
        
        Ok(profit)
    }
    
    // Calculate profits for Base Network
    async fn calculate_base_network_profits(
        profit: &mut MultiCurrencyProfit,
        eth_profit: U256,
    ) -> Result<()> {
        debug!("Calculating profits for Base Network");
        
        // Base Network conversion rates (these would ideally come from an oracle)
        // 1 ETH = 2000 USDC (6 decimals)
        profit.usdc = Some(eth_profit.saturating_mul(U256::from(2000)) / U256::from(10).pow(U256::from(12)));
        
        // 1 ETH = 2000 USDT (6 decimals)
        profit.usdt = Some(eth_profit.saturating_mul(U256::from(2000)) / U256::from(10).pow(U256::from(12)));
        
        // 1 ETH = 0.06 WBTC (8 decimals)
        profit.wbtc = Some(eth_profit.saturating_mul(U256::from(6)) / U256::from(10).pow(U256::from(11)));
        
        // 1 ETH = 1 WETH (18 decimals)
        profit.weth = Some(eth_profit);
        
        // 1 ETH = 2000 DAI (18 decimals)
        profit.dai = Some(eth_profit.saturating_mul(U256::from(2000)) / U256::from(10).pow(U256::from(18)));
        
        Ok(())
    }
    
    // Calculate profits for Ethereum Mainnet
    async fn calculate_ethereum_profits(
        profit: &mut MultiCurrencyProfit,
        eth_profit: U256,
    ) -> Result<()> {
        debug!("Calculating profits for Ethereum Mainnet");
        
        // Ethereum Mainnet conversion rates (these would ideally come from an oracle)
        // 1 ETH = 2000 USDC (6 decimals)
        profit.usdc = Some(eth_profit.saturating_mul(U256::from(2000)) / U256::from(10).pow(U256::from(12)));
        
        // 1 ETH = 2000 USDT (6 decimals)
        profit.usdt = Some(eth_profit.saturating_mul(U256::from(2000)) / U256::from(10).pow(U256::from(12)));
        
        // 1 ETH = 0.06 WBTC (8 decimals)
        profit.wbtc = Some(eth_profit.saturating_mul(U256::from(6)) / U256::from(10).pow(U256::from(11)));
        
        // 1 ETH = 1 WETH (18 decimals)
        profit.weth = Some(eth_profit);
        
        // 1 ETH = 2000 DAI (18 decimals)
        profit.dai = Some(eth_profit.saturating_mul(U256::from(2000)) / U256::from(10).pow(U256::from(18)));
        
        Ok(())
    }
}