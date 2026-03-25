#![no_std]
use soroban_sdk::{contract, contracttype, contractimpl, Address, Env, token, Symbol};

// Minimum balance required to keep the IoT relay open (500 tokens for testing)
const MINIMUM_BALANCE_TO_FLOW: i128 = 500; // 500 tokens minimum for testing

#[contracttype]
#[derive(Clone)]
pub struct SignedUsageData {
    pub meter_id: u64,
    pub timestamp: u64,
    pub watt_hours_consumed: i128,
    pub units_consumed: i128,
    pub signature: BytesN<64>,
    pub public_key: BytesN<32>,
}

#[contracttype]
#[derive(Clone)]
pub struct UsageData {
    pub total_watt_hours: i128,
    pub current_cycle_watt_hours: i128,
    pub peak_usage_watt_hours: i128,
    pub last_reading_timestamp: u64,
    pub precision_factor: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct Meter {
    pub user: Address,
    pub provider: Address,
    pub billing_type: BillingType,
    pub off_peak_rate: i128,      // rate per second during off-peak hours
    pub peak_rate: i128,          // rate per second during peak hours (1.5x off-peak)
    pub rate_per_second: i128,
    pub rate_per_unit: i128,
    pub balance: i128,
    pub debt: i128,
    pub collateral_limit: i128,
    pub last_update: u64,
    pub is_active: bool,
    pub token: Address,
    pub usage_data: UsageData,
    pub max_flow_rate_per_hour: i128,
    pub last_claim_time: u64,
    pub claimed_this_hour: i128,
    pub heartbeat: u64,
    pub device_public_key: BytesN<32>,
}

#[contracttype]
#[derive(Clone)]
pub struct ProviderWithdrawalWindow {
    pub daily_withdrawn: i128,
    pub last_reset: u64,
}

#[contracttype]
pub enum DataKey {
    Meter(u64),
    ProviderWindow(Address),
    Count,
    Oracle,
}

#[contract]
pub struct UtilityContract;

const HOUR_IN_SECONDS: u64 = 60 * 60;
const DAY_IN_SECONDS: u64 = 24 * HOUR_IN_SECONDS;
const DAILY_WITHDRAWAL_PERCENT: i128 = 10;
const MAX_USAGE_PER_UPDATE: i128 = 1_000_000_000_000i128; // 1 billion kWh max per update
const MIN_PRECISION_FACTOR: i128 = 1;
const MAX_TIMESTAMP_DELAY: u64 = 300; // 5 minutes

fn verify_usage_signature(env: &Env, signed_data: &SignedUsageData, meter: &Meter) -> Result<(), ContractError> {
    // Check if the provided public key matches the registered meter's public key
    if signed_data.public_key != meter.device_public_key {
        return Err(ContractError::PublicKeyMismatch);
    }

    // Check timestamp is not too old (prevent replay attacks)
    let current_time = env.ledger().timestamp();
    if current_time.saturating_sub(signed_data.timestamp) > MAX_TIMESTAMP_DELAY {
        return Err(ContractError::TimestampTooOld);
    }

    // Create the message that was signed: meter_id || timestamp || watt_hours_consumed || units_consumed
    let mut message = Vec::new(env);
    message.push_back(&signed_data.meter_id);
    message.push_back(&signed_data.timestamp);
    message.push_back(&signed_data.watt_hours_consumed);
    message.push_back(&signed_data.units_consumed);

    // Verify the signature using Soroban's built-in signature verification
    if env.crypto().ed25519_verify(
        &signed_data.public_key,
        &message.to_bytes(),
        &signed_data.signature,
    ) {
        Ok(())
    } else {
        Err(ContractError::InvalidSignature)
    }
}

// Peak hours: 18:00 - 21:00 UTC
const PEAK_HOUR_START: u64 = 18 * HOUR_IN_SECONDS; // 64800 seconds
const PEAK_HOUR_END: u64 = 21 * HOUR_IN_SECONDS;   // 75600 seconds
const PEAK_RATE_MULTIPLIER: i128 = 3; // 1.5x => stored as 3 (divide by 2)
const RATE_PRECISION: i128 = 2; // Precision for rate calculations

/// Checks if an address represents the native Stellar asset (XLM)
fn is_native_token(token_address: &Address) -> bool {
    // In Soroban, the native asset address can be identified by specific patterns
    // For testing purposes, we'll use a special address that represents native XLM
    // In production, this would be the actual native token address
    let addr_str = token_address.to_string();
    // Common patterns for native XLM in Soroban test environments
    addr_str.starts_with("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABG5ydGg") ||
    addr_str.starts_with("CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2Y2W3U2XPIVVU4XZQ4") ||
    addr_str.contains("NATIVE")
}

/// Transfer tokens, handling both native XLM and SAC tokens
fn transfer_tokens(env: &Env, token_address: &Address, from: &Address, to: &Address, amount: &i128) {
    if is_native_token(token_address) {
        // For native XLM, use the built-in transfer function
        env.token().transfer(from, to, amount);
    } else {
        // For SAC tokens, use the token contract
        let client = token::Client::new(env, token_address);
        client.transfer(from, to, amount);
    }
}

/// Get token balance, handling both native XLM and SAC tokens
fn get_token_balance(env: &Env, token_address: &Address, account: &Address) -> i128 {
    if is_native_token(token_address) {
        // For native XLM, use the built-in balance function
        env.token().balance(account)
    } else {
        // For SAC tokens, use the token contract
        let client = token::Client::new(env, token_address);
        client.balance(account)
    }
}

/// Get the native token address for testing purposes
#[cfg(test)]
fn get_native_token_address(env: &Env) -> Address {
    // For testing, we create a special address that will be identified as native
    // In production, this would return the actual native token address
    Address::from_string(&soroban_sdk::symbol_short!("NATIVE_TOKEN"))
}

fn get_meter_or_panic(env: &Env, meter_id: u64) -> Meter {
    match env
        .storage()
        .instance()
        .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
    {
        Some(meter) => meter,
        None => panic_with_error!(env, ContractError::MeterNotFound),
    }
}

fn get_oracle_or_panic(env: &Env) -> Address {
    match env
        .storage()
        .instance()
        .get::<DataKey, Address>(&DataKey::Oracle)
    {
        Some(oracle) => oracle,
        None => panic_with_error!(env, ContractError::OracleNotSet),
    }
}

fn convert_xlm_to_usd_if_needed(env: &Env, amount: i128, token: &Address) -> Result<i128, ContractError> {
    // Check if the token is XLM (native token)
    // In Stellar, the native token has address = 0
    if token.to_string().starts_with("CA") {
        // This is a custom token, no conversion needed
        return Ok(amount);
    }
    
    // Assume native token is XLM, convert to USD cents
    let oracle_address = get_oracle_or_panic(env);
    let oracle_client = PriceOracleClient::new(env, &oracle_address);
    
    match oracle_client.xlm_to_usd_cents(&amount) {
        result => Ok(result),
        _ => Err(ContractError::PriceConversionFailed),
    }
}

fn convert_usd_to_xlm_if_needed(env: &Env, usd_cents: i128, token: &Address) -> Result<i128, ContractError> {
    // Check if the token is XLM (native token)
    if token.to_string().starts_with("CA") {
        // This is a custom token, no conversion needed
        return Ok(usd_cents);
    }
    
    // Assume native token is XLM, convert from USD cents to XLM
    let oracle_address = get_oracle_or_panic(env);
    let oracle_client = PriceOracleClient::new(env, &oracle_address);
    
    match oracle_client.usd_cents_to_xlm(&usd_cents) {
        result => Ok(result),
        _ => Err(ContractError::PriceConversionFailed),
    }
}

fn remaining_postpaid_collateral(meter: &Meter) -> i128 {
    meter.collateral_limit.saturating_sub(meter.debt).max(0)
}

fn is_peak_hour(timestamp: u64) -> bool {
    let seconds_in_day = timestamp % DAY_IN_SECONDS;
    seconds_in_day >= PEAK_HOUR_START && seconds_in_day < PEAK_HOUR_END
}

fn get_effective_rate(meter: &Meter, timestamp: u64) -> i128 {
    if is_peak_hour(timestamp) {
        meter.peak_rate
    } else {
        meter.off_peak_rate
    }
}

fn provider_meter_value(meter: &Meter) -> i128 {
    match meter.billing_type {
        BillingType::PrePaid => meter.balance.max(0),
        BillingType::PostPaid => remaining_postpaid_collateral(meter),
    }
}

fn refresh_activity(meter: &mut Meter) {
    meter.is_active = provider_meter_value(meter) > 0;
}

fn reset_claim_window_if_needed(meter: &mut Meter, now: u64) {
    if now.saturating_sub(meter.last_claim_time) >= HOUR_IN_SECONDS {
        meter.claimed_this_hour = 0;
        meter.last_claim_time = now;
    }
}

fn remaining_claim_capacity(meter: &Meter) -> i128 {
    meter
        .max_flow_rate_per_hour
        .saturating_sub(meter.claimed_this_hour)
        .max(0)
}

fn get_provider_window_or_default(
    env: &Env,
    provider: &Address,
    now: u64,
) -> ProviderWithdrawalWindow {
    env.storage()
        .instance()
        .get(&DataKey::ProviderWindow(provider.clone()))
        .unwrap_or(ProviderWithdrawalWindow {
            daily_withdrawn: 0,
            last_reset: now,
        })
}

fn reset_provider_window_if_needed(window: &mut ProviderWithdrawalWindow, now: u64) {
    if now.saturating_sub(window.last_reset) >= DAY_IN_SECONDS {
        window.daily_withdrawn = 0;
        window.last_reset = now;
    }
}

fn get_provider_total_pool(env: &Env, provider: &Address) -> i128 {
    let count = env
        .storage()
        .instance()
        .get::<DataKey, u64>(&DataKey::Count)
        .unwrap_or(0);
    let mut total_pool: i128 = 0;
    let mut meter_id = 1;

    while meter_id <= count {
        if let Some(meter) = env
            .storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
        {
            if meter.provider == *provider {
                total_pool = total_pool.saturating_add(provider_meter_value(&meter));
            }
        }

        meter_id += 1;
    }

    total_pool
}

fn apply_provider_withdrawal_limit(
    env: &Env,
    provider: &Address,
    amount: i128,
) -> ProviderWithdrawalWindow {
    let now = env.ledger().timestamp();
    let mut window = get_provider_window_or_default(env, provider, now);
    reset_provider_window_if_needed(&mut window, now);

    if amount <= 0 {
        return window;
    }

    let total_pool_before_claim =
        get_provider_total_pool(env, provider).saturating_add(window.daily_withdrawn);
    let daily_limit = total_pool_before_claim / DAILY_WITHDRAWAL_PERCENT;

    if window.daily_withdrawn.saturating_add(amount) > daily_limit {
        panic_with_error!(env, ContractError::WithdrawalLimitExceeded);
    }

    window.daily_withdrawn = window.daily_withdrawn.saturating_add(amount);
    window
}

fn apply_provider_claim(env: &Env, meter: &mut Meter, amount: i128) {
    if amount <= 0 {
        return;
    }

    transfer_tokens(env, &meter.token, &env.current_contract_address(), &meter.provider, &amount);

    match meter.billing_type {
        BillingType::PrePaid => {
            meter.balance = meter.balance.saturating_sub(amount);
        }
        BillingType::PostPaid => {
            meter.debt = meter.debt.saturating_add(amount);
        }
    }

    meter.claimed_this_hour = meter.claimed_this_hour.saturating_add(amount);
}

fn publish_active_event(env: &Env, meter_id: u64, now: u64) {
    env.events()
        .publish((symbol_short!("Active"), meter_id), now);
}

fn publish_inactive_event(env: &Env, meter_id: u64, now: u64) {
    env.events()
        .publish((symbol_short!("Inactive"), meter_id), now);
}

#[contractimpl]
impl UtilityContract {
    pub fn get_minimum_balance_to_flow() -> i128 {
        MINIMUM_BALANCE_TO_FLOW
    }

    pub fn set_oracle(env: Env, oracle_address: Address) {
        // This should be called by admin to set the oracle address
        // For now, we'll just store it in instance storage
        env.storage().instance().set(&DataKey::Oracle, &oracle_address);
    }

    pub fn set_maintenance_config(env: Env, wallet: Address, fee_bps: i128) {
        env.storage().instance().set(&DataKey::MaintenanceWallet, &wallet);
        env.storage().instance().set(&DataKey::ProtocolFeeBps, &fee_bps);
    }

    pub fn add_supported_token(env: Env, token: Address) {
        // Ideally requires admin auth, but for simplicity:
        env.storage().instance().set(&DataKey::SupportedToken(token), &true);
    }

    pub fn remove_supported_token(env: Env, token: Address) {
        env.storage().instance().set(&DataKey::SupportedToken(token), &false);
    }

    pub fn register_meter(
        env: Env,
        user: Address,
        provider: Address,
        off_peak_rate: i128,
        token: Address,
        device_public_key: BytesN<32>,
    ) -> u64 {
        Self::register_meter_with_mode(env, user, provider, rate, token, BillingType::PrePaid, device_public_key)
        Self::register_meter_with_mode(env, user, provider, off_peak_rate, token, BillingType::PrePaid)
    }

    pub fn register_meter_with_mode(
        env: Env,
        user: Address,
        provider: Address,
        off_peak_rate: i128,
        token: Address,
        billing_type: BillingType,
        device_public_key: BytesN<32>,
    ) -> u64 {
        user.require_auth();

        let mut count = env
            .storage()
            .instance()
            .get::<DataKey, u64>(&DataKey::Count)
            .unwrap_or(0);
        count += 1;

        let now = env.ledger().timestamp();
        let peak_rate = off_peak_rate.saturating_mul(PEAK_RATE_MULTIPLIER) / RATE_PRECISION;
        
        let usage_data = UsageData {
            total_watt_hours: 0,
            current_cycle_watt_hours: 0,
            peak_usage_watt_hours: 0,
            last_reading_timestamp: now,
            precision_factor: 1000,
        };

        let meter = Meter {
            user,
            provider,
            billing_type,
            off_peak_rate,
            peak_rate,
            rate_per_second: rate,
            rate_per_unit: rate,
            balance: 0,
            debt: 0,
            collateral_limit: 0,
            last_update: now,
            is_active: false,
            token,
            usage_data,
            max_flow_rate_per_hour: off_peak_rate.saturating_mul(HOUR_IN_SECONDS as i128),
            last_claim_time: now,
            claimed_this_hour: 0,
            heartbeat: now,
            device_public_key,
        };

        env.storage().instance().set(&DataKey::Meter(count), &meter);
        env.storage().instance().set(&DataKey::Count, &count);
        count
    }

    pub fn top_up(env: Env, meter_id: u64, amount: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        let was_active = meter.is_active;
        transfer_tokens(&env, &meter.token, &meter.user, &env.current_contract_address(), &amount);
        let client = token::Client::new(&env, &meter.token);
        
        // Convert XLM to USD cents if needed
        let converted_amount = match convert_xlm_to_usd_if_needed(&env, amount, &meter.token) {
            Ok(amount) => amount,
            Err(_) => panic_with_error!(&env, ContractError::PriceConversionFailed),
        };
        
        if converted_amount <= 0 {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }
        
        client.transfer(&meter.user, &env.current_contract_address(), &amount);

        meter.balance += amount;
        
        // Only activate if balance meets minimum requirement
        meter.is_active = meter.balance >= MINIMUM_BALANCE_TO_FLOW;
        meter.last_update = env.ledger().timestamp();
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn top_up_with_token(env: Env, meter_id: u64, amount: i128, payment_token: Address) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.user.require_auth();

        let is_supported: bool = env.storage().instance().get(&DataKey::SupportedToken(payment_token.clone())).unwrap_or(false);
        if !is_supported {
            panic!("Token not supported for payment");
        }

        let client = token::Client::new(&env, &payment_token);
        
        // Burn the alternative token (Carbon Credit) to offset energy footprint
        client.burn(&meter.user, &amount);
        meter.is_active = true;
        meter.last_update = env.ledger().timestamp();
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

        // Credit the meter balance 1:1 for the burned tokens
        meter.balance += amount;
        
        meter.is_active = meter.balance >= MINIMUM_BALANCE_TO_FLOW;
        meter.last_update = env.ledger().timestamp();
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    #[allow(deprecated)]
    pub fn deduct_units(env: Env, meter_id: u64, units_consumed: i128) {
        let oracle = get_oracle_or_panic(&env);
        oracle.require_auth();

        let mut meter = get_meter_or_panic(&env, signed_data.meter_id);
        
        // Verify the signature matches the registered device public key
        verify_usage_signature(&env, &signed_data, &meter)
            .unwrap_or_else(|e| panic_with_error!(&env, e));

        let now = env.ledger().timestamp();
        reset_claim_window_if_needed(&mut meter, now);

        let requested = signed_data.units_consumed.saturating_mul(meter.rate_per_second);
        let effective_rate = get_effective_rate(&meter, now);
        let requested = units_consumed.saturating_mul(effective_rate);
        let claimable = requested
        // Peak hour tariff logic from Issue #13
        let current_hour = (now % 86400) / 3600;
        let is_peak = current_hour >= 18 && current_hour < 22; // 6 PM to 10 PM UTC
        let base_cost = units_consumed.saturating_mul(meter.rate_per_unit);
        let cost = if is_peak {
            base_cost.saturating_mul(15) / 10
        } else {
            base_cost
        };

        // Enforce max flow rate hourly cap and available funds
        let claimable = cost
            .min(remaining_claim_capacity(&meter))
            .min(provider_meter_value(&meter));

        let was_active = meter.is_active;
        apply_provider_claim(&env, &mut meter, claimable);
        meter.last_update = now;
        refresh_activity(&mut meter);

        env.storage().instance().set(&DataKey::Meter(signed_data.meter_id), &meter);

            if actual_claim > 0 {
                let client = token::Client::new(&env, &meter.token);
                let mut payout = actual_claim;
                
                if let Some(wallet) = env.storage().instance().get::<_, Address>(&DataKey::MaintenanceWallet) {
                    let fee_bps: i128 = env.storage().instance().get(&DataKey::ProtocolFeeBps).unwrap_or(0);
                    let fee = (actual_claim * fee_bps) / 10000;
                    payout -= fee;
                    if fee > 0 {
                        client.transfer(&env.current_contract_address(), &wallet, &fee);
                    }
                }
                if payout > 0 {
                    client.transfer(&env.current_contract_address(), &meter.provider, &payout);
                }
                meter.balance -= actual_claim;
            }
                client.transfer(&env.current_contract_address(), &meter.provider, &actual_claim);
                meter.balance -= actual_claim;
            }
        }
        
        // Check minimum balance after deduction
        if meter.balance < MINIMUM_BALANCE_TO_FLOW {
            meter.is_active = false;
        }
        
        // Check minimum balance after deduction
        if meter.balance < MINIMUM_BALANCE_TO_FLOW {
            meter.is_active = false;
        }

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        // Emit UsageReported event
        env.events().publish(
            (Symbol::new(&env, "UsageReported"), meter_id),
            (units_consumed, cost)
        );
    }

    pub fn claim(env: Env, meter_id: u64) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.provider.require_auth();

        let now = env.ledger().timestamp();
        let elapsed = now.checked_sub(meter.last_update).unwrap_or(0);
        let amount = (elapsed as i128) * meter.rate_per_unit;
        
        // Check if we're in the same hour as last claim
        let current_hour = now / 3600;
        let last_claim_hour = meter.last_claim_time / 3600;
        
        if current_hour == last_claim_hour {
            // Same hour, check if we exceed max flow rate
            let max_allowed = meter.max_flow_rate_per_hour - meter.claimed_this_hour;
            let actual_amount = if amount > max_allowed {
                max_allowed
            } else {
                amount
            };
            
            // Ensure we don't overdraw the balance
            let claimable = if actual_amount > meter.balance {
                meter.balance
            } else {
                actual_amount
            };

            if claimable > 0 {
                let client = token::Client::new(&env, &meter.token);
                let mut payout = claimable;
                
                if let Some(wallet) = env.storage().instance().get::<_, Address>(&DataKey::MaintenanceWallet) {
                    let fee_bps: i128 = env.storage().instance().get(&DataKey::ProtocolFeeBps).unwrap_or(0);
                    let fee = (claimable * fee_bps) / 10000;
                    payout -= fee;
                    if fee > 0 {
                        client.transfer(&env.current_contract_address(), &wallet, &fee);
                    }
                }
                if payout > 0 {
                    client.transfer(&env.current_contract_address(), &meter.provider, &payout);
                }
                client.transfer(&env.current_contract_address(), &meter.provider, &claimable);
                meter.balance -= claimable;
                meter.claimed_this_hour += claimable;
            }
        } else {
            // New hour, reset claimed_this_hour
            meter.claimed_this_hour = 0;
            
            // Ensure we don't overdraw the balance
            let claimable = if amount > meter.balance {
                meter.balance
            } else {
                amount
            };

            if claimable > 0 {
                let client = token::Client::new(&env, &meter.token);
                let mut payout = claimable;
                
                if let Some(wallet) = env.storage().instance().get::<_, Address>(&DataKey::MaintenanceWallet) {
                    let fee_bps: i128 = env.storage().instance().get(&DataKey::ProtocolFeeBps).unwrap_or(0);
                    let fee = (claimable * fee_bps) / 10000;
                    payout -= fee;
                    if fee > 0 {
                        client.transfer(&env.current_contract_address(), &wallet, &fee);
                    }
                }
                if payout > 0 {
                    client.transfer(&env.current_contract_address(), &meter.provider, &payout);
                }
                client.transfer(&env.current_contract_address(), &meter.provider, &claimable);
                meter.balance -= claimable;
                meter.claimed_this_hour = claimable;
            }
        }

        meter.last_update = now;
        meter.last_claim_time = now;
        
        // Deactivate if balance falls below minimum requirement
        if meter.balance < MINIMUM_BALANCE_TO_FLOW {
            meter.is_active = false;
        }

        
        // Deactivate if balance falls below minimum requirement
        if meter.balance < MINIMUM_BALANCE_TO_FLOW {
            meter.is_active = false;
        }

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        // Emit UsageReported event
        env.events().publish(
            (Symbol::new(&env, "UsageReported"), meter_id),
            (units_consumed, cost)
        );
    }

    pub fn update_usage(env: Env, meter_id: u64, watt_hours_consumed: i128) {
        // Input validation for security
        if watt_hours_consumed < 0 {
            panic_with_error!(env, ContractError::InvalidUsageValue);
        }
        
        if watt_hours_consumed > MAX_USAGE_PER_UPDATE {
            panic_with_error!(env, ContractError::UsageExceedsLimit);
        }
        
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        let precise_consumption =
            watt_hours_consumed.saturating_mul(meter.usage_data.precision_factor);
        meter.usage_data.total_watt_hours = meter
            .usage_data
            .total_watt_hours
            .saturating_add(precise_consumption);
        meter.usage_data.current_cycle_watt_hours = meter
            .usage_data
            .current_cycle_watt_hours
            .saturating_add(precise_consumption);

        if meter.usage_data.current_cycle_watt_hours > meter.usage_data.peak_usage_watt_hours {
            meter.usage_data.peak_usage_watt_hours = meter.usage_data.current_cycle_watt_hours;
        }

        meter.usage_data.last_reading_timestamp = env.ledger().timestamp();
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn reset_cycle_usage(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        meter.usage_data.current_cycle_watt_hours = 0;
        meter.usage_data.last_reading_timestamp = env.ledger().timestamp();
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn get_usage_data(env: Env, meter_id: u64) -> Option<UsageData> {
        env.storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
            .map(|meter| meter.usage_data)
    }

    pub fn get_meter(env: Env, meter_id: u64) -> Option<Meter> {
        env.storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
    }

    pub fn get_provider_window(env: Env, provider: Address) -> Option<ProviderWithdrawalWindow> {
        env.storage()
            .instance()
            .get(&DataKey::ProviderWindow(provider))
    }

    pub fn get_watt_hours_display(precise_watt_hours: i128, precision_factor: i128) -> i128 {
        if precision_factor <= 0 {
            return precise_watt_hours; // Fallback to avoid division by zero
        }
        precise_watt_hours / precision_factor
    }

    }

    pub fn calculate_expected_depletion(env: Env, meter_id: u64) -> Option<u64> {
        if let Some(meter) = env.storage().instance().get::<_, Meter>(&DataKey::Meter(meter_id)) {
            if meter.balance <= 0 || meter.rate_per_unit <= 0 {
                return Some(0); // Already depleted or no consumption
            }
            
            let seconds_until_depletion = meter.balance / meter.rate_per_unit;
            let current_time = env.ledger().timestamp();
            Some(current_time + seconds_until_depletion as u64)
        } else {
            None
        }
    }

    pub fn emergency_shutdown(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        
        // Emergency shutdown always disables the meter regardless of balance
        meter.is_active = false;
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn set_max_flow_rate(env: Env, meter_id: u64, max_rate_per_hour: i128) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.provider.require_auth();
        
        meter.max_flow_rate_per_hour = max_rate_per_hour;
        
        meter.is_active = false;
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn set_max_flow_rate(env: Env, meter_id: u64, max_rate_per_hour: i128) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.provider.require_auth();
        
        meter.max_flow_rate_per_hour = max_rate_per_hour;
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn set_max_flow_rate(env: Env, meter_id: u64, max_rate_per_hour: i128) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.provider.require_auth();
        
        meter.max_flow_rate_per_hour = max_rate_per_hour;
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn set_max_flow_rate(env: Env, meter_id: u64, max_rate_per_hour: i128) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.provider.require_auth();
        
        // Emergency shutdown always disables the meter regardless of balance
        meter.is_active = false;
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn update_heartbeat(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();
        meter.heartbeat = env.ledger().timestamp();
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn withdraw_earnings(env: Env, meter_id: u64, amount_usd_cents: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        
        if amount_usd_cents <= 0 {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }
        
        let available_earnings = match meter.billing_type {
            BillingType::PrePaid => meter.balance,
            BillingType::PostPaid => meter.debt,
        };
        
        if amount_usd_cents > available_earnings {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }
        
        // Convert USD cents to XLM if needed
        let withdrawal_amount = match convert_usd_to_xlm_if_needed(&env, amount_usd_cents, &meter.token) {
            Ok(amount) => amount,
            Err(_) => panic_with_error!(&env, ContractError::PriceConversionFailed),
        };
        
        let client = token::Client::new(&env, &meter.token);
        client.transfer(&env.current_contract_address(), &meter.provider, &withdrawal_amount);
        
        // Update meter balance/debt
        match meter.billing_type {
            BillingType::PrePaid => {
                meter.balance = meter.balance.saturating_sub(amount_usd_cents);
            }
            BillingType::PostPaid => {
                meter.debt = meter.debt.saturating_sub(amount_usd_cents);
            }
        }
        
        let now = env.ledger().timestamp();
        refresh_activity(&mut meter);
        meter.last_update = now;
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
        
        // Emit conversion event if XLM was used
        if !meter.token.to_string().starts_with("CA") {
            env.events().publish(
                (symbol_short!("USDtoXLM"), meter_id), 
                (amount_usd_cents, withdrawal_amount)
            );
        }
    }

    pub fn get_current_rate(env: Env) -> Option<PriceData> {
        match env.storage().instance().get::<DataKey, Address>(&DataKey::Oracle) {
            Some(oracle_address) => {
                let oracle_client = PriceOracleClient::new(&env, &oracle_address);
                Some(oracle_client.get_price())
            }
            None => None,
        }
    }

    pub fn is_meter_offline(env: Env, meter_id: u64) -> bool {
        match env
            .storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
        {
            Some(meter) => {
                env.ledger().timestamp().saturating_sub(meter.heartbeat) > HOUR_IN_SECONDS
            }
            None => true,
        }
    }

    pub fn get_watt_hours_display(watt_hours: i128, precision_factor: i128) -> i128 {
        watt_hours / precision_factor
    }

    /// Unlink a meter from its current tenant and link it to a new tenant.
    /// All historical usage data is preserved. Requires auth from the current
    /// user, the new user, and the provider.
    pub fn transfer_meter_ownership(env: Env, meter_id: u64, new_user: Address) {
        let mut meter = get_meter_or_panic(&env, meter_id);

        meter.user.require_auth();
        meter.provider.require_auth();
        new_user.require_auth();

        let old_user = meter.user.clone();
        meter.user = new_user.clone();

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        env.events().publish(
            (symbol_short!("Transfer"), meter_id),
            (old_user, new_user),
        );
    }
}

mod test;
