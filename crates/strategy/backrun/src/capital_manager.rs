use alloy_primitives::{Address, U256};
use eyre::{eyre, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use loom_types_entities::{Market, PoolWrapper, Token};
use std::collections::HashSet;

/// CapitalManager handles dynamic capital allocation for arbitrage trades
pub struct CapitalManager {
    /// Maximum capital in USD (with 6 decimals)
    max_capital_usd: U256,
    /// Token prices in USD (with 6 decimals)
    prices: RwLock<HashMap<Address, U256>>,
    /// Pool liquidity estimates
    pool_liquidity: RwLock<HashMap<String, U256>>,
    /// ETH price in USD (with 6 decimals)
    eth_usd_price: RwLock<U256>,
}

impl CapitalManager {
    /// Create a new capital manager
    pub fn new(max_capital_usd: u64) -> Self {
        Self {
            max_capital_usd: U256::from(max_capital_usd * 1_000_000), // Convert to 6 decimals
            prices: RwLock::new(HashMap::new()),
            pool_liquidity: RwLock::new(HashMap::new()),
            eth_usd_price: RwLock::new(U256::from(2000 * 1_000_000)), // Default ETH price: $2000 with 6 decimals
        }
    }
    
    /// Update the ETH price in USD
    pub async fn update_eth_price(&self, price_usd: u64) {
        let price = U256::from(price_usd * 1_000_000); // Convert to 6 decimals
        *self.eth_usd_price.write().await = price;
        info!("Updated ETH price to ${} USD", price_usd);
    }
    
    /// Set the maximum capital in USD
    pub fn set_max_capital_usd(&mut self, max_capital_usd: u64) {
        self.max_capital_usd = U256::from(max_capital_usd * 1_000_000); // Convert to 6 decimals
    }
    
    /// Update the price of a token
    pub async fn update_price(&self, token_address: Address, price: U256) {
        self.prices.write().await.insert(token_address, price);
    }
    
    /// Update the liquidity of a pool
    pub async fn update_pool_liquidity(&self, pool_id: String, liquidity: U256) {
        self.pool_liquidity.write().await.insert(pool_id, liquidity);
    }
    
    /// Calculate the optimal capital allocation for a trade
    pub async fn calculate_optimal_capital(
        &self,
        token: &Token,
        pools: &[Arc<PoolWrapper>],
        market: &Market,
    ) -> Result<U256> {
        // Get the token price
        let token_price = match self.get_token_price(token, market).await {
            Ok(price) => price,
            Err(e) => {
                // If we can't get the price, use a fallback price
                // This is a conservative approach to avoid blocking trades
                debug!("Could not get token price: {}, using fallback price", e);
                U256::from(1_000_000) // Assume $1 with 6 decimals
            }
        };
        
        if token_price.is_zero() {
            return Err(eyre!("Token price is zero"));
        }
        
        // Calculate the maximum amount based on USD limit
        let max_from_usd = self.max_capital_usd
            .checked_mul(U256::from(10).pow(U256::from(token.get_decimals())))
            .ok_or_else(|| eyre!("Overflow in max_from_usd calculation"))?
            .checked_div(token_price)
            .ok_or_else(|| eyre!("Division by zero in max_from_usd calculation"))?;
        
        // Calculate the maximum amount based on pool liquidity
        let max_from_liquidity = match self.calculate_max_from_liquidity(token, pools).await {
            Ok(liquidity) => liquidity,
            Err(e) => {
                // If we can't calculate liquidity, use a conservative default
                debug!("Could not calculate liquidity: {}, using default", e);
                U256::from(100) * U256::from(10).pow(U256::from(token.get_decimals()))
            }
        };
        
        // Use the smaller of the two limits
        let optimal_amount = if max_from_liquidity < max_from_usd {
            max_from_liquidity
        } else {
            max_from_usd
        };
        
        // Ensure the amount is not zero
        if optimal_amount.is_zero() {
            return Err(eyre!("Calculated optimal amount is zero"));
        }
        
        let usd_value = token.to_float(optimal_amount) * token.to_float(token_price);
        debug!(
            "Optimal capital allocation: {} {} (${} USD)",
            token.to_float(optimal_amount),
            token.get_symbol(),
            usd_value
        );
        
        Ok(optimal_amount)
    }
    
    /// Get the price of a token in USD (with 6 decimals)
    async fn get_token_price(&self, token: &Token, market: &Market) -> Result<U256> {
        // Check if we have the price in cache
        if let Some(price) = self.prices.read().await.get(&token.get_address()) {
            return Ok(*price);
        }
        
        // Try to get the price from the token's eth_price and convert to USD
        // using the current ETH/USD price
        if let Some(eth_price) = token.get_eth_price() {
            // Get the current ETH price in USD
            let eth_usd_price = *self.eth_usd_price.read().await;
            
            // Convert ETH price to USD price (with 6 decimals)
            let price_u256 = eth_price.checked_mul(eth_usd_price)
                .ok_or_else(|| eyre!("Overflow in price calculation"))?
                .checked_div(U256::from(10).pow(U256::from(18))) // Adjust for ETH's 18 decimals
                .ok_or_else(|| eyre!("Division by zero in price calculation"))?;
            
            // Cache the price
            self.prices.write().await.insert(token.get_address(), price_u256);
            
            return Ok(price_u256);
        }
        
        // If the token doesn't have a price, try to calculate it from pools
        let pools = if let Some(token_pools) = market.get_token_pools(&token.get_address()) {
            token_pools.iter()
                .filter_map(|pool_id| market.get_pool(pool_id))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        
        for pool in pools {
            // Find a pool with a token that has a price
            let token_addresses = pool.get_tokens();
            if token_addresses.len() < 2 {
                continue;
            }
            
            let other_token_address = if token_addresses[0] == token.get_address() {
                token_addresses[1]
            } else {
                token_addresses[0]
            };
            
            let other_token = match market.get_token(&other_token_address) {
                Some(t) => t,
                None => continue,
            };
            
            if let Some(other_eth_price) = other_token.get_eth_price() {
                // Get the current ETH price in USD
                let eth_usd_price = *self.eth_usd_price.read().await;
                
                // Convert ETH price to USD price (with 6 decimals)
                let other_price = other_eth_price.checked_mul(eth_usd_price)
                    .ok_or_else(|| eyre!("Overflow in price calculation"))?
                    .checked_div(U256::from(10).pow(U256::from(18))) // Adjust for ETH's 18 decimals
                    .ok_or_else(|| eyre!("Division by zero in price calculation"))?;
                
                // Get the exchange rate from the pool
                // Since we don't have direct access to reserves, we'll need to estimate
                // This is a simplified approach - in a real implementation, you'd use the pool's
                // actual reserves or a price oracle
                
                // For now, we'll assume a 1:1 ratio adjusted for decimals
                let token_decimals = token.get_decimals();
                let other_decimals = other_token.get_decimals();
                
                // Adjust for decimal differences
                let decimal_adjustment = if token_decimals > other_decimals {
                    10u64.pow((token_decimals - other_decimals) as u32)
                } else if token_decimals < other_decimals {
                    10u64.pow((other_decimals - token_decimals) as u32)
                } else {
                    1
                };
                
                let price_u256 = if token_decimals > other_decimals {
                    // If token has more decimals, we need to multiply the price
                    // because each unit of the token represents a smaller value
                    other_price.checked_mul(U256::from(decimal_adjustment))
                        .ok_or_else(|| eyre!("Overflow in price calculation"))?
                } else if token_decimals < other_decimals {
                    // If token has fewer decimals, we need to divide the price
                    // because each unit of the token represents a larger value
                    other_price.checked_div(U256::from(decimal_adjustment))
                        .ok_or_else(|| eyre!("Division by zero in price calculation"))?
                } else {
                    other_price
                };
                
                // Cache the price
                self.prices.write().await.insert(token.get_address(), price_u256);
                
                return Ok(price_u256);
            }
        }
        
        // If we couldn't calculate the price, return an error
        Err(eyre!("Could not calculate token price"))
    }
    
    /// Calculate the maximum amount based on pool liquidity
    async fn calculate_max_from_liquidity(
        &self,
        token: &Token,
        pools: &[Arc<PoolWrapper>],
    ) -> Result<U256> {
        let mut min_liquidity = U256::MAX;
        let mut found_liquidity = false;
        
        // Check if we have any pool liquidity estimates in our cache
        let pool_liquidity = self.pool_liquidity.read().await;
        
        for pool in pools {
            // Check if we have a liquidity estimate for this pool
            let pool_id = pool.get_pool_id().to_string();
            
            if let Some(liquidity) = pool_liquidity.get(&pool_id) {
                // Use at most 10% of the pool's liquidity
                let max_amount = *liquidity / U256::from(10);
                
                if max_amount < min_liquidity {
                    min_liquidity = max_amount;
                    found_liquidity = true;
                }
                continue;
            }
            
            // If we don't have a cached liquidity estimate, use a default value
            // In a real implementation, you would calculate this based on the pool's reserves
            // For now, we'll use a conservative default value
            let default_liquidity = U256::from(1000) * U256::from(10).pow(U256::from(token.get_decimals()));
            let max_amount = default_liquidity / U256::from(10);
            
            if max_amount < min_liquidity {
                min_liquidity = max_amount;
                found_liquidity = true;
            }
        }
        
        if !found_liquidity {
            return Err(eyre!("No liquidity found for token"));
        }
        
        Ok(min_liquidity)
    }
}