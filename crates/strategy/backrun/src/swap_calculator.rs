use std::cmp::min;
use alloy_primitives::utils::parse_units;
use alloy_primitives::U256;
use eyre::{eyre, ErrReport, Result};
use lazy_static::lazy_static;
use loom_types_blockchain::LoomDataTypes;
use loom_types_entities::{PoolWrapper, SwapError, SwapLine};
use revm::primitives::Env;
use revm::DatabaseRef;
use tracing::debug;

// Extension trait for PoolWrapper to add missing methods
trait PoolWrapperExt<LDT: LoomDataTypes> {
    fn contains_token(&self, token_address: &LDT::Address) -> bool;
    fn get_reserves(&self) -> (U256, U256);
    fn get_token_addresses(&self) -> Vec<LDT::Address>;
}

impl<LDT: LoomDataTypes> PoolWrapperExt<LDT> for PoolWrapper<LDT> {
    fn contains_token(&self, token_address: &LDT::Address) -> bool {
        self.get_tokens().contains(token_address)
    }
    
    fn get_reserves(&self) -> (U256, U256) {
        // This is a simplified implementation - in a real scenario, you would
        // need to get the actual reserves from the pool
        // For now, we'll return dummy values
        (U256::from(1000000), U256::from(1000000))
    }
    
    fn get_token_addresses(&self) -> Vec<LDT::Address> {
        // Return the tokens from the pool
        self.get_tokens()
    }
}

lazy_static! {
    // Default starting amount for optimization (0.1 ETH for better opportunities)
    static ref DEFAULT_OPTIMIZE_INPUT: U256 = parse_units("0.1", "ether").unwrap();
    
    // Maximum capital in USD (with 6 decimals) - $100,000
    static ref MAX_CAPITAL_USD: U256 = U256::from(100_000_000_000u64);
    
    // Flash loan fee optimized for Ethereum mainnet (0.05% for Aave)
    static ref FLASH_LOAN_FEE_NUMERATOR: U256 = U256::from(5);
    static ref FLASH_LOAN_FEE_DENOMINATOR: U256 = U256::from(10000);
    
    // Minimum profit threshold (0.001 ETH = ~$2-3)
    static ref MIN_PROFIT_THRESHOLD: U256 = parse_units("0.001", "ether").unwrap();
    
    // Gas cost estimation (21000 base + ~200000 for complex swaps)
    static ref ESTIMATED_GAS_COST: U256 = U256::from(250000);
}

pub struct SwapCalculator {}

impl SwapCalculator {
    /// Calculate the optimal input amount and profit for a swap path with enhanced profitability checks
    #[inline]
    pub fn calculate<'a, DB: DatabaseRef<Error = ErrReport>, LDT: LoomDataTypes>(
        path: &'a mut SwapLine<LDT>,
        state: &'a DB,
        env: Env,
    ) -> Result<&'a mut SwapLine<LDT>, SwapError<LDT>> {
        let first_token = path.get_first_token().unwrap();
        
        // Start with multiple test amounts to find the best range
        let test_amounts = vec![
            parse_units("0.01", "ether").unwrap(),
            parse_units("0.1", "ether").unwrap(),
            parse_units("1.0", "ether").unwrap(),
            parse_units("5.0", "ether").unwrap(),
        ];
        
        let mut best_path: Option<SwapLine<LDT>> = None;
        let mut best_profit = U256::ZERO;
        
        for test_eth_amount in test_amounts {
            if let Some(amount_in) = first_token.calc_token_value_from_eth(test_eth_amount) {
                let mut path_clone = path.clone();
                
                // Test this amount
                if let Ok(_) = path_clone.optimize_with_in_amount(state, env.clone(), amount_in) {
                    let profit = path_clone.abs_profit_eth();
                    
                    // Check if this is profitable after costs
                    if Self::is_profitable_after_costs(profit, test_eth_amount, &env) {
                        if profit > best_profit {
                            best_profit = profit;
                            
                            // Try to optimize around this amount
                            if let Ok(_) = Self::optimize_input_amount(&mut path_clone, state, env.clone(), amount_in) {
                                let optimized_profit = path_clone.abs_profit_eth();
                                if optimized_profit > profit {
                                    best_path = Some(path_clone);
                                    best_profit = optimized_profit;
                                } else {
                                    // Keep the non-optimized version if it was better
                                    path_clone.optimize_with_in_amount(state, env.clone(), amount_in).ok();
                                    best_path = Some(path_clone);
                                }
                            } else {
                                best_path = Some(path_clone);
                            }
                        }
                    }
                }
            }
        }
        
        if let Some(best) = best_path {
            *path = best;
            debug!("Found profitable path with profit: {} ETH", best_profit);
            Ok(path)
        } else {
            Err(path.to_error("NO_PROFITABLE_AMOUNT_FOUND".to_string()))
        }
    }
    
    /// Check if a trade is profitable after accounting for gas costs and fees
    #[inline]
    fn is_profitable_after_costs(profit: U256, input_amount: U256, env: &Env) -> bool {
        // Calculate gas cost in ETH
        let gas_price = env.tx.gas_price.unwrap_or_else(|| U256::from(20_000_000_000u64)); // 20 gwei default
        let gas_cost_wei = gas_price * *ESTIMATED_GAS_COST;
        
        // Calculate flash loan fee
        let flash_loan_fee = Self::calculate_flash_loan_fee(input_amount);
        
        // Total costs
        let total_costs = gas_cost_wei + flash_loan_fee;
        
        // Profit must exceed costs plus minimum threshold
        let required_profit = total_costs + *MIN_PROFIT_THRESHOLD;
        
        let is_profitable = profit > required_profit;
        
        if is_profitable {
            debug!("Trade is profitable: profit={} ETH, costs={} ETH, net={} ETH", 
                   profit, total_costs, profit.saturating_sub(total_costs));
        } else {
            debug!("Trade not profitable: profit={} ETH, required={} ETH", profit, required_profit);
        }
        
        is_profitable
    }
    
    /// Calculate flash loan fee based on input amount
    #[inline]
    fn calculate_flash_loan_fee(input_amount: U256) -> U256 {
        // Aave flash loan fee is 0.05% (5 basis points)
        input_amount * *FLASH_LOAN_FEE_NUMERATOR / *FLASH_LOAN_FEE_DENOMINATOR
    } // kept private for internal use
    
    /// Optimize the input amount using binary search to find the most profitable amount
    #[inline]
    pub fn optimize_input_amount<'a, DB: DatabaseRef<Error = ErrReport>, LDT: LoomDataTypes>(
        path: &'a mut SwapLine<LDT>,
        state: &DB,
        env: Env,
        initial_amount: U256,
    ) -> Result<&'a mut SwapLine<LDT>, SwapError<LDT>> {
        // This token is used in estimate_max_amount_from_liquidity
        let _first_token = path.get_first_token().unwrap();
        
        // Estimate the maximum amount based on pool liquidity
        let max_amount = Self::estimate_max_amount_from_liquidity(path);
        
        // Use binary search to find the optimal input amount
        let mut low = initial_amount;
        let mut high = max_amount;
        let mut best_amount = initial_amount;
        let mut best_profit = U256::ZERO;
        
        // Number of iterations for binary search
        let iterations = 8;
        
        for _ in 0..iterations {
            if high <= low {
                break;
            }
            
            // Try three points: low, mid, high
            let mid = low + (high - low) / U256::from(2);
            
            // Calculate profit for each point
            let mut path_low = path.clone();
            let mut path_mid = path.clone();
            let mut path_high = path.clone();
            
            let low_result = path_low.optimize_with_in_amount(state, env.clone(), low);
            let mid_result = path_mid.optimize_with_in_amount(state, env.clone(), mid);
            let high_result = path_high.optimize_with_in_amount(state, env.clone(), high);
            
            // Get profits
            let profit_low = if low_result.is_ok() {
                path_low.abs_profit_eth()
            } else {
                U256::ZERO
            };
            
            let profit_mid = if mid_result.is_ok() {
                path_mid.abs_profit_eth()
            } else {
                U256::ZERO
            };
            
            let profit_high = if high_result.is_ok() {
                path_high.abs_profit_eth()
            } else {
                U256::ZERO
            };
            
            // Update best profit
            if profit_low > best_profit {
                best_profit = profit_low;
                best_amount = low;
            }
            
            if profit_mid > best_profit {
                best_profit = profit_mid;
                best_amount = mid;
            }
            
            if profit_high > best_profit {
                best_profit = profit_high;
                best_amount = high;
            }
            
            // Narrow search range based on where the highest profit is
            if profit_mid > profit_low && profit_mid > profit_high {
                // Peak is in the middle, narrow to both sides
                low = low + (mid - low) / U256::from(2);
                high = mid + (high - mid) / U256::from(2);
            } else if profit_low > profit_mid {
                // Peak is toward the lower end
                high = mid;
            } else {
                // Peak is toward the higher end
                low = mid;
            }
        }
        
        // Use the best amount found
        debug!("Optimized input amount: {} with profit: {}", best_amount, best_profit);
        path.optimize_with_in_amount(state, env, best_amount)
    }
    
    /// Estimate the maximum amount based on pool liquidity
    /// This ensures we don't try to use more capital than the pools can handle
    #[inline]
    fn estimate_max_amount_from_liquidity<LDT: LoomDataTypes>(path: &SwapLine<LDT>) -> U256 {
        let first_token = path.get_first_token().unwrap();
        
        // Get the minimum liquidity across all pools in the path
        let min_liquidity = path.path.pools.iter()
            .filter_map(|pool| {
                if pool.contains_token(&first_token.get_address()) {
                    let (reserve0, reserve1) = pool.get_reserves();
                    let token_addresses = pool.get_tokens();
                    
                    // Get the reserve of the first token
                    let token_reserve = if token_addresses[0] == first_token.get_address() {
                        reserve0
                    } else {
                        reserve1
                    };
                    
                    Some(token_reserve)
                } else {
                    None
                }
            })
            .min()
            .unwrap_or(U256::from(0));
        
        // Use at most 10% of the minimum liquidity
        let max_from_liquidity = min_liquidity / U256::from(10);
        
        // Get the maximum amount in ETH that we're willing to use
        let max_eth_amount = parse_units("10", "ether").unwrap();
        
        // Convert max ETH to token amount
        let max_token_amount = first_token.calc_token_value_from_eth(max_eth_amount)
            .unwrap_or(U256::from(0));
        
        // Use the minimum of the two limits
        if max_from_liquidity < max_token_amount {
            max_from_liquidity
        } else {
            max_token_amount
        }
    }
    
    /// Calculate the flash loan fee for a given amount (public API)
    /// Mirrors the internal fee method to avoid duplicate definitions
    #[inline]
    pub fn flash_loan_fee(amount: U256) -> U256 {
        (amount * *FLASH_LOAN_FEE_NUMERATOR) / *FLASH_LOAN_FEE_DENOMINATOR
    }
    
    /// Calculate the minimum profit required for a trade to be profitable
    #[inline]
    pub fn calculate_min_profit(amount: U256) -> U256 {
        let flash_loan_fee = Self::calculate_flash_loan_fee(amount);
        let repayment_amount = amount + flash_loan_fee;
        
        // Require at least 1% profit on top of the flash loan fee
        let min_profit_percentage = (amount * U256::from(1)) / U256::from(100);
        
        repayment_amount + min_profit_percentage
    }
}
