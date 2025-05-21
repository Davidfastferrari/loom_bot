use std::cmp::min;
use alloy_primitives::utils::parse_units;
use alloy_primitives::U256;
use eyre::{eyre, ErrReport, Result};
use lazy_static::lazy_static;
use loom_types_blockchain::LoomDataTypes;
use loom_types_entities::{PoolWrapper, SwapError, SwapLine};
use revm::primitives::Env;
use revm::DatabaseRef;
use tracing::debugg;

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
    // Default starting amount for optimization (0.01 ETH)
    static ref DEFAULT_OPTIMIZE_INPUT: U256 = parse_units("0.01", "ether").unwrap().get_absolute();
    
    // Maximum capital in USD (with 6 decimals) - $100,000
    static ref MAX_CAPITAL_USD: U256 = U256::from(100_000_000_000u64);
    
    // Flash loan fee reduced from 0.9% to 0.3%
    static ref FLASH_LOAN_FEE_NUMERATOR: U256 = U256::from(3);
    static ref FLASH_LOAN_FEE_DENOMINATOR: U256 = U256::from(1000);
}

pub struct SwapCalculator {}

impl SwapCalculator {
    /// Calculate the optimal input amount and profit for a swap path
    #[inline]
    pub fn calculate<DB: DatabaseRef<Error = ErrReport>, LDT: LoomDataTypes>(
        path: &mut SwapLine<LDT>,
        state: &DB,
        env: Env,
    ) -> Result<&mut SwapLine<LDT>, SwapError<LDT>> {
        let first_token = path.get_first_token().unwrap();
        
        // Start with the default amount
        if let Some(amount_in) = first_token.calc_token_value_from_eth(*DEFAULT_OPTIMIZE_INPUT) {
            // First create a clone to work with
            let mut path_clone = path.clone();
            
            // First try with the default amount to see if the path is profitable
            let result = path_clone.optimize_with_in_amount(state, env.clone(), amount_in);
            
            if result.is_ok() {
                // If profitable, try to optimize the input amount for maximum profit
                let optimized_result = Self::optimize_input_amount(&mut path_clone, state, env, amount_in);
                
                if optimized_result.is_ok() {
                    // Copy the optimized values back to the original path
                    *path = path_clone;
                    return Ok(path);
                }
                
                // Even if optimization failed, use the initial result
                *path = path_clone;
                return Ok(path);
            } else {
                // Return the error from the initial attempt
                return Err(path_clone.to_error("OPTIMIZATION_FAILED".to_string()));
            }
        } else {
            return Err(path.to_error("PRICE_NOT_SET".to_string()));
        }
    }
    
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
        let max_eth_amount = parse_units("10", "ether").unwrap().get_absolute();
        
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
    
    /// Calculate the flash loan fee for a given amount
    /// Fee reduced from 0.9% to 0.3%
    #[inline]
    pub fn calculate_flash_loan_fee(amount: U256) -> U256 {
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
