use alloy_primitives::{Address, U256};
use eyre::{eyre, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use loom_types_entities::{Market, PoolWrapper, Token};

/// CapitalManager handles dynamic capital allocation for arbitrage trades
pub struct CapitalManager {
    /// Maximum capital in USD (with 6 decimals)
    max_capital_usd: U256,
    /// Token prices in USD (with 6 decimals)
    prices: RwLock<HashMap<Address, U256>>,
    /// Pool liquidity estimates
    pool_liquidity: RwLock<HashMap<String, U256>>,
}

impl CapitalManager {
    /// Create a new capital manager
    pub fn new(max_capital_usd: u64) -> Self {
        Self {
            max_capital_usd: U256::from(max_capital_usd * 1_000_000), // Convert to 6 decimals
            prices: RwLock::new(HashMap::new()),
            pool_liquidity: RwLock::new(HashMap::new()),
        }
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
        let token_price = self.get_token_price(token, market).await?;
        
        if token_price.is_zero() {
            return Err(eyre!("Token price is zero"));
        }
        
        // Calculate the maximum amount based on USD limit
        let max_from_usd = self.max_capital_usd
            .checked_mul(U256::from(10).pow(U256::from(token.decimals)))
            .ok_or_else(|| eyre!("Overflow in max_from_usd calculation"))?
            .checked_div(token_price)
            .ok_or_else(|| eyre!("Division by zero in max_from_usd calculation"))?;
        
        // Calculate the maximum amount based on pool liquidity
        let max_from_liquidity = self.calculate_max_from_liquidity(token, pools).await?;
        
        // Use the smaller of the two limits
        let optimal_amount = if max_from_liquidity < max_from_usd {
            max_from_liquidity
        } else {
            max_from_usd
        };
        
        debug!(
            "Optimal capital allocation: {} {} (${} USD)",
            token.to_float(optimal_amount),
            token.get_symbol(),
            (token.to_float(optimal_amount) * token.to_float(token_price))
        );
        
        Ok(optimal_amount)
    }
    
    /// Get the price of a token in USD (with 6 decimals)
    async fn get_token_price(&self, token: &Token, market: &Market) -> Result<U256> {
        // Check if we have the price in cache
        if let Some(price) = self.prices.read().await.get(&token.address()) {
            return Ok(*price);
        }
        
        // If the token has a price, use it
        if let Some(price) = token.usd_price {
            // Convert to 6 decimals
            let price_u256 = U256::from((price * 1_000_000.0) as u64);
            
            // Cache the price
            self.prices.write().await.insert(token.address(), price_u256);
            
            return Ok(price_u256);
        }
        
        // If the token doesn't have a price, try to calculate it from pools
        let pools = market.get_pools_by_token(&token.address());
        
        for pool in pools {
            // Find a pool with a token that has a price
            let token_addresses = pool.get_token_addresses();
            let other_token_address = if token_addresses[0] == token.address() {
                token_addresses[1]
            } else {
                token_addresses[0]
            };
            
            let other_token = match market.get_token(&other_token_address) {
                Some(t) => t,
                None => continue,
            };
            
            if let Some(other_price) = other_token.usd_price {
                // Get the exchange rate from the pool
                let (reserve0, reserve1) = pool.get_reserves();
                
                let (token_reserve, other_reserve) = if token_addresses[0] == token.address() {
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
                
                let price_ratio = token_reserve.as_u128() as f64 / (other_reserve.as_u128() as f64 * decimal_adjustment as f64);
                let token_price = other_price / price_ratio;
                
                // Convert to 6 decimals
                let price_u256 = U256::from((token_price * 1_000_000.0) as u64);
                
                // Cache the price
                self.prices.write().await.insert(token.address(), price_u256);
                
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
        
        for pool in pools {
            if pool.contains_token(&token.address()) {
                // Get the reserves
                let (reserve0, reserve1) = pool.get_reserves();
                
                // Get the token reserve
                let token_addresses = pool.get_token_addresses();
                let token_reserve = if token_addresses[0] == token.address() {
                    reserve0
                } else {
                    reserve1
                };
                
                // Use at most 10% of the pool's liquidity
                let max_amount = token_reserve / U256::from(10);
                
                if max_amount < min_liquidity {
                    min_liquidity = max_amount;
                }
            }
        }
        
        if min_liquidity == U256::MAX {
            return Err(eyre!("No liquidity found for token"));
        }
        
        Ok(min_liquidity)
    }
}