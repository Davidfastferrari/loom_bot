use std::collections::HashMap;
use std::sync::Arc;
use alloy_primitives::{Address, U256};
use eyre::{eyre, Result};
use tokio::sync::RwLock;

use loom_types_entities::Market;

/// PriceFeed provides token prices in USD
pub struct PriceFeed {
    /// Token prices in USD (with 6 decimals)
    prices: RwLock<HashMap<Address, U256>>,
    /// Reference to the market for getting token information
    market: Arc<RwLock<Market>>,
}

impl PriceFeed {
    /// Create a new price feed
    pub fn new(market: Arc<RwLock<Market>>) -> Self {
        Self {
            prices: RwLock::new(HashMap::new()),
            market,
        }
    }

    /// Get the price of a token in USD (with 6 decimals)
    pub async fn get_price(&self, token_address: &Address) -> Result<U256> {
        // Check if we have the price in cache
        if let Some(price) = self.prices.read().await.get(token_address) {
            return Ok(*price);
        }

        // If not, try to calculate it from the market
        let market_guard = self.market.read().await;
        
        // Get the token
        let token = market_guard.get_token(token_address)
            .ok_or_else(|| eyre!("Token not found"))?;
        
        // If the token has a price, use it
        // Convert ETH price to USD price (assuming 1 ETH = $2000 for simplicity)
        if let Some(eth_price) = token.get_eth_price() {
            // ETH price is in wei, convert to USD with 6 decimals
            // This is a simplified conversion - in a real system you'd use an oracle
            let eth_usd_price = U256::from(2000_000_000); // $2000 with 6 decimals
            let price_u256 = eth_price.checked_mul(eth_usd_price)
                .ok_or_else(|| eyre!("Price calculation overflow"))?
                .checked_div(U256::from(10).pow(U256::from(18)))
                .ok_or_else(|| eyre!("Price calculation division by zero"))?;
            
            // Cache the price
            self.prices.write().await.insert(*token_address, price_u256);
            
            return Ok(price_u256);
        }
        
        // If the token doesn't have a price, try to calculate it from pools
        // Use get_token_pools instead of get_pools_by_token
        let pool_ids = market_guard.get_token_pools(token_address)
            .ok_or_else(|| eyre!("No pools found for token"))?;
        
        // Convert pool IDs to pool wrappers
        let pools: Vec<_> = pool_ids.iter()
            .filter_map(|pool_id| market_guard.get_pool(pool_id))
            .collect();
        
        for pool in pools {
            // Find a pool with a token that has a price
            let token_addresses = pool.get_tokens();
            let other_token_address = if token_addresses[0] == *token_address {
                token_addresses[1]
            } else {
                token_addresses[0]
            };
            
            let other_token = market_guard.get_token(&other_token_address)
                .ok_or_else(|| eyre!("Other token not found"))?;
            
            if let Some(other_eth_price) = other_token.get_eth_price() {
                // Convert ETH price to USD price
                let eth_usd_price = U256::from(2000_000_000); // $2000 with 6 decimals
                let other_price = other_eth_price.checked_mul(eth_usd_price)
                    .ok_or_else(|| eyre!("Price calculation overflow"))?
                    .checked_div(U256::from(10).pow(U256::from(18)))
                    .ok_or_else(|| eyre!("Price calculation division by zero"))?;
                
                // Get the exchange rate from the pool
                // Since we don't have direct access to reserves, we'll estimate based on token prices
                // In a real implementation, you would call the pool contract to get reserves
                let reserve0 = U256::from(1000000); // Placeholder value
                let reserve1 = U256::from(1000000); // Placeholder value
                
                let (token_reserve, other_reserve) = if token_addresses[0] == *token_address {
                    (reserve0, reserve1)
                } else {
                    (reserve1, reserve0)
                };
                
                if other_reserve.is_zero() {
                    continue;
                }
                
                // Calculate the price
                let token_decimals = token.get_decimals();
                let other_decimals = other_token.get_decimals();
                
                // Adjust for decimal differences
                let decimal_adjustment = if token_decimals > other_decimals {
                    10u64.pow((token_decimals - other_decimals) as u32)
                } else {
                    1
                };
                
                // Calculate price using integer arithmetic
                // We need to scale up for precision since we're working with integers
                let _scale_factor = U256::from(1_000_000); // 6 decimal places for precision
                
                // Calculate token_price = other_price * other_reserve * decimal_adjustment / token_reserve
                let scaled_other_reserve = other_reserve.checked_mul(U256::from(decimal_adjustment))
                    .ok_or_else(|| eyre!("Overflow in reserve calculation"))?;
                
                let numerator = other_price.checked_mul(scaled_other_reserve)
                    .ok_or_else(|| eyre!("Overflow in price calculation"))?;
                
                let price_u256 = numerator.checked_div(token_reserve)
                    .ok_or_else(|| eyre!("Division by zero in price calculation"))?;
                
                // Cache the price
                self.prices.write().await.insert(*token_address, price_u256);
                
                return Ok(price_u256);
            }
        }
        
        // If we couldn't calculate the price, return an error
        Err(eyre!("Could not calculate token price"))
    }
    
    /// Update the price of a token
    pub async fn update_price(&self, token_address: Address, price: U256) {
        self.prices.write().await.insert(token_address, price);
    }
    
    /// Update prices from external source
    pub async fn update_prices_from_external(&self) -> Result<()> {
        // This would typically call an external API to get prices
        // For now, we'll just use a placeholder implementation
        
        // Get the market
        let market_guard = self.market.read().await;
        
        // Get all tokens from the tokens HashMap
        let tokens: Vec<_> = market_guard.tokens().values().cloned().collect();
        
        // Update prices for basic tokens
        for token in tokens {
            if token.is_basic() {
                if let Some(eth_price) = token.get_eth_price() {
                    // Convert ETH price to USD price (assuming 1 ETH = $2000)
                    let eth_usd_price = U256::from(2000_000_000); // $2000 with 6 decimals
                    let price_u256 = eth_price.checked_mul(eth_usd_price)
                        .ok_or_else(|| eyre!("Price calculation overflow"))?
                        .checked_div(U256::from(10).pow(U256::from(18)))
                        .ok_or_else(|| eyre!("Price calculation division by zero"))?;
                    
                    self.prices.write().await.insert(token.get_address(), price_u256);
                }
            }
        }
        
        Ok(())
    }
    
    /// Estimate the maximum capital that can be used for a token
    pub async fn estimate_max_capital(&self, token_address: &Address, max_usd: U256) -> Result<U256> {
        // Get the token price
        let token_price = self.get_price(token_address).await?;
        
        if token_price.is_zero() {
            return Err(eyre!("Token price is zero"));
        }
        
        // Calculate the maximum amount of tokens
        let max_tokens = max_usd.checked_mul(U256::from(10).pow(U256::from(6)))
            .ok_or_else(|| eyre!("Overflow in max_tokens calculation"))?
            .checked_div(token_price)
            .ok_or_else(|| eyre!("Division by zero in max_tokens calculation"))?;
        
        // Get the token
        let market_guard = self.market.read().await;
        let token = market_guard.get_token(token_address)
            .ok_or_else(|| eyre!("Token not found"))?;
        
        // Adjust for token decimals
        let decimals = token.get_decimals();
        let max_amount = max_tokens.checked_mul(U256::from(10).pow(U256::from(decimals)))
            .ok_or_else(|| eyre!("Overflow in max_amount calculation"))?
            .checked_div(U256::from(10).pow(U256::from(6)))
            .ok_or_else(|| eyre!("Division by zero in max_amount calculation"))?;
        
        Ok(max_amount)
    }
}