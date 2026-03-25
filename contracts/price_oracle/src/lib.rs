#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PriceData {
    pub price: i128,        // Price in smallest units (e.g., cents for USD)
    pub decimals: u32,      // Number of decimal places
    pub last_updated: u64,  // Timestamp of last update
}

#[contracttype]
pub enum DataKey {
    Price,
    Admin,
    Updater,
}

#[contracterror]
#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotAuthorized = 1,
    InvalidPrice = 2,
    StalePrice = 3,
    NotInitialized = 4,
}

const MAX_PRICE_AGE_SECONDS: u64 = 300; // 5 minutes

#[contract]
pub struct PriceOracle;

#[contractimpl]
impl PriceOracle {
    /// Initialize the oracle with admin and updater addresses
    pub fn initialize(env: Env, admin: Address, updater: Address, initial_price: i128, decimals: u32) {
        if env.storage().instance().get::<DataKey, Address>(&DataKey::Admin).is_some() {
            panic!("already initialized");
        }

        if initial_price <= 0 {
            panic_with_error!(env, ContractError::InvalidPrice);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Updater, &updater);
        
        let price_data = PriceData {
            price: initial_price,
            decimals,
            last_updated: env.ledger().timestamp(),
        };
        env.storage().instance().set(&DataKey::Price, &price_data);
    }

    /// Update the price (only callable by updater)
    pub fn update_price(env: Env, new_price: i128) {
        let updater = env.storage().instance()
            .get::<DataKey, Address>(&DataKey::Updater)
            .unwrap_or_else(|| panic_with_error!(env, ContractError::NotInitialized));
        
        updater.require_auth();

        if new_price <= 0 {
            panic_with_error!(env, ContractError::InvalidPrice);
        }

        let price_data = PriceData {
            price: new_price,
            decimals: Self::get_decimals(env.clone()),
            last_updated: env.ledger().timestamp(),
        };
        env.storage().instance().set(&DataKey::Price, &price_data);
    }

    /// Get current price data
    pub fn get_price(env: Env) -> PriceData {
        env.storage().instance()
            .get::<DataKey, PriceData>(&DataKey::Price)
            .unwrap_or_else(|| panic_with_error!(env, ContractError::NotInitialized))
    }

    /// Get price with staleness check
    pub fn get_fresh_price(env: Env) -> PriceData {
        let price_data = Self::get_price(env.clone());
        let now = env.ledger().timestamp();
        
        if now.saturating_sub(price_data.last_updated) > MAX_PRICE_AGE_SECONDS {
            panic_with_error!(env, ContractError::StalePrice);
        }
        
        price_data
    }

    /// Get just the price value
    pub fn get_price_value(env: Env) -> i128 {
        Self::get_price(env).price
    }

    /// Get number of decimals
    pub fn get_decimals(env: Env) -> u32 {
        Self::get_price(env).decimals
    }

    /// Convert XLM amount to USD cents
    pub fn xlm_to_usd_cents(env: Env, xlm_amount: i128) -> i128 {
        let price_data = Self::get_fresh_price(env);
        
        // price is in cents per XLM, so multiply
        xlm_amount.saturating_mul(price_data.price) / (10_i128.pow(price_data.decimals))
    }

    /// Convert USD cents to XLM amount
    pub fn usd_cents_to_xlm(env: Env, usd_cents: i128) -> i128 {
        let price_data = Self::get_fresh_price(env);
        
        // price is in cents per XLM, so divide
        usd_cents.saturating_mul(10_i128.pow(price_data.decimals)) / price_data.price
    }

    /// Check if price is fresh
    pub fn is_price_fresh(env: Env) -> bool {
        let price_data = Self::get_price(env.clone());
        let now = env.ledger().timestamp();
        now.saturating_sub(price_data.last_updated) <= MAX_PRICE_AGE_SECONDS
    }

    /// Admin functions
    pub fn set_admin(env: Env, new_admin: Address) {
        let admin = env.storage().instance()
            .get::<DataKey, Address>(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, ContractError::NotInitialized));
        
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
    }

    pub fn set_updater(env: Env, new_updater: Address) {
        let admin = env.storage().instance()
            .get::<DataKey, Address>(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, ContractError::NotInitialized));
        
        admin.require_auth();
        env.storage().instance().set(&DataKey::Updater, &new_updater);
    }

    /// Get admin address
    pub fn get_admin(env: Env) -> Address {
        env.storage().instance()
            .get::<DataKey, Address>(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, ContractError::NotInitialized))
    }

    /// Get updater address  
    pub fn get_updater(env: Env) -> Address {
        env.storage().instance()
            .get::<DataKey, Address>(&DataKey::Updater)
            .unwrap_or_else(|| panic_with_error!(env, ContractError::NotInitialized))
    }
}
