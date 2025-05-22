use alloy_primitives::{Address, U256};
use eyre::{Result, eyre};
use revm::DatabaseRef;
use tracing::{info, debug};
use std::collections::HashMap;

// Major token addresses on Ethereum mainnet
pub const USDC_ADDRESS: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
pub const USDT_ADDRESS: &str = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
pub const WBTC_ADDRESS: &str = "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599";
pub const WETH_ADDRESS: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
pub const DAI_ADDRESS: &str = "0x6B175474E89094C44Da98b954EedeAC495271d0F";

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
        market_state: &DB,
        token_prices: Option<HashMap<Address, U256>>,
    ) -> Result<MultiCurrencyProfit> {
        let mut profit = MultiCurrencyProfit::new(eth_profit);
        
        // If token prices are provided, use them directly
        if let Some(prices) = token_prices {
            Self::calculate_from_prices(&mut profit, eth_profit, prices)?;
            return Ok(profit);
        }
        
        // Otherwise, try to fetch prices from the market state
        // This would require implementing price fetching logic specific to your system
        Self::fetch_and_calculate_prices(&mut profit, eth_profit, market_state).await?;
        
        Ok(profit)
    }
    
    // Calculate profits using provided token prices
    fn calculate_from_prices(
        profit: &mut MultiCurrencyProfit,
        eth_profit: U256,
        prices: HashMap<Address, U256>,
    ) -> Result<()> {
        // Parse token addresses
        let usdc_addr = Address::parse_checksummed(USDC_ADDRESS, None)?;
        let usdt_addr = Address::parse_checksummed(USDT_ADDRESS, None)?;
        let wbtc_addr = Address::parse_checksummed(WBTC_ADDRESS, None)?;
        let weth_addr = Address::parse_checksummed(WETH_ADDRESS, None)?;
        let dai_addr = Address::parse_checksummed(DAI_ADDRESS, None)?;
        
        // Calculate profits in each currency
        if let Some(usdc_price) = prices.get(&usdc_addr) {
            // USDC has 6 decimals, ETH has 18, so adjust the calculation
            // Formula: eth_profit * usdc_price / 10^12
            profit.usdc = Some(eth_profit.saturating_mul(*usdc_price) / U256::from(10).pow(U256::from(12)));
        }
        
        if let Some(usdt_price) = prices.get(&usdt_addr) {
            // USDT has 6 decimals
            profit.usdt = Some(eth_profit.saturating_mul(*usdt_price) / U256::from(10).pow(U256::from(12)));
        }
        
        if let Some(wbtc_price) = prices.get(&wbtc_addr) {
            // WBTC has 8 decimals
            profit.wbtc = Some(eth_profit.saturating_mul(*wbtc_price) / U256::from(10).pow(U256::from(10)));
        }
        
        if let Some(weth_price) = prices.get(&weth_addr) {
            // WETH has 18 decimals, same as ETH
            profit.weth = Some(eth_profit.saturating_mul(*weth_price) / U256::from(10).pow(U256::from(18)));
        }
        
        if let Some(dai_price) = prices.get(&dai_addr) {
            // DAI has 18 decimals, same as ETH
            profit.dai = Some(eth_profit.saturating_mul(*dai_price) / U256::from(10).pow(U256::from(18)));
        }
        
        Ok(())
    }
    
    // Fetch prices from market state and calculate profits
    async fn fetch_and_calculate_prices<DB: DatabaseRef>(
        profit: &mut MultiCurrencyProfit,
        eth_profit: U256,
        _market_state: &DB,
    ) -> Result<()> {
        // This would need to be implemented based on your specific market state structure
        // For now, we'll use placeholder logic
        
        debug!("Fetching token prices from market state");
        
        // Placeholder implementation - in a real system, you would fetch actual prices
        // from your market state or from an oracle
        
        // Example conversion rates (these should be replaced with actual fetched values):
        // 1 ETH = 2000 USDC
        profit.usdc = Some(eth_profit.saturating_mul(U256::from(2000)) / U256::from(10).pow(U256::from(12)));
        
        // 1 ETH = 2000 USDT
        profit.usdt = Some(eth_profit.saturating_mul(U256::from(2000)) / U256::from(10).pow(U256::from(12)));
        
        // 1 ETH = 0.06 WBTC (approximate)
        profit.wbtc = Some(eth_profit.saturating_mul(U256::from(6)) / U256::from(10).pow(U256::from(11)));
        
        // 1 ETH = 1 WETH
        profit.weth = Some(eth_profit);
        
        // 1 ETH = 2000 DAI
        profit.dai = Some(eth_profit.saturating_mul(U256::from(2000)) / U256::from(10).pow(U256::from(18)));
        
        Ok(())
    }
}