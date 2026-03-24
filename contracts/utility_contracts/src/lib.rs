#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    Address, Env,
};

// Oracle client interface
use soroban_sdk::contractclient;

#[contractclient(name = "PriceOracleClient")]
pub trait PriceOracle {
    fn xlm_to_usd_cents(env: Env, xlm_amount: i128) -> i128;
    fn usd_cents_to_xlm(env: Env, usd_cents: i128) -> i128;
    fn get_price(env: Env) -> PriceData;
}

#[contracttype]
#[derive(Clone)]
pub struct PriceData {
    pub price: i128,
    pub decimals: u32,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BillingType {
    PrePaid,
    PostPaid,
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
    pub rate_per_second: i128,
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

#[contracterror]
#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    MeterNotFound = 1,
    OracleNotSet = 2,
    WithdrawalLimitExceeded = 3,
    PriceConversionFailed = 4,
    InvalidTokenAmount = 5,
}

#[contract]
pub struct UtilityContract;

const HOUR_IN_SECONDS: u64 = 60 * 60;
const DAY_IN_SECONDS: u64 = 24 * HOUR_IN_SECONDS;
const DAILY_WITHDRAWAL_PERCENT: i128 = 10;

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

    let client = token::Client::new(env, &meter.token);
    client.transfer(&env.current_contract_address(), &meter.provider, &amount);

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
    pub fn set_oracle(env: Env, oracle: Address) {
        env.storage().instance().set(&DataKey::Oracle, &oracle);
    }

    pub fn register_meter(
        env: Env,
        user: Address,
        provider: Address,
        rate: i128,
        token: Address,
    ) -> u64 {
        Self::register_meter_with_mode(env, user, provider, rate, token, BillingType::PrePaid)
    }

    pub fn register_meter_with_mode(
        env: Env,
        user: Address,
        provider: Address,
        rate: i128,
        token: Address,
        billing_type: BillingType,
    ) -> u64 {
        user.require_auth();

        let mut count = env
            .storage()
            .instance()
            .get::<DataKey, u64>(&DataKey::Count)
            .unwrap_or(0);
        count += 1;

        let now = env.ledger().timestamp();
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
            rate_per_second: rate,
            balance: 0,
            debt: 0,
            collateral_limit: 0,
            last_update: now,
            is_active: false,
            token,
            usage_data,
            max_flow_rate_per_hour: rate.saturating_mul(HOUR_IN_SECONDS as i128),
            last_claim_time: now,
            claimed_this_hour: 0,
            heartbeat: now,
        };

        env.storage().instance().set(&DataKey::Meter(count), &meter);
        env.storage().instance().set(&DataKey::Count, &count);
        count
    }

    pub fn top_up(env: Env, meter_id: u64, amount: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        let was_active = meter.is_active;
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

        match meter.billing_type {
            BillingType::PrePaid => {
                meter.balance = meter.balance.saturating_add(converted_amount);
            }
            BillingType::PostPaid => {
                let settlement = converted_amount.min(meter.debt.max(0));
                meter.debt = meter.debt.saturating_sub(settlement);
                meter.collateral_limit = meter
                    .collateral_limit
                    .saturating_add(converted_amount.saturating_sub(settlement));
            }
        }

        let now = env.ledger().timestamp();
        refresh_activity(&mut meter);
        if !was_active && meter.is_active {
            meter.last_update = now;
        }

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        if !was_active && meter.is_active {
            publish_active_event(&env, meter_id, now);
        }
        
        // Emit conversion event if XLM was used
        if !meter.token.to_string().starts_with("CA") {
            env.events().publish(
                (symbol_short!("XLMtoUSD"), meter_id), 
                (amount, converted_amount)
            );
        }
    }

    pub fn claim(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();

        let now = env.ledger().timestamp();
        if !meter.is_active {
            meter.last_update = now;
            env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
            return;
        }

        reset_claim_window_if_needed(&mut meter, now);

        let elapsed = now.saturating_sub(meter.last_update);
        let requested = (elapsed as i128).saturating_mul(meter.rate_per_second);
        let claimable = requested
            .min(remaining_claim_capacity(&meter))
            .min(provider_meter_value(&meter));

        if claimable > 0 {
            let provider_window =
                apply_provider_withdrawal_limit(&env, &meter.provider, claimable);
            apply_provider_claim(&env, &mut meter, claimable);
            env.storage().instance().set(
                &DataKey::ProviderWindow(meter.provider.clone()),
                &provider_window,
            );
        }

        let was_active = meter.is_active;
        meter.last_update = now;
        refresh_activity(&mut meter);
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        if was_active && !meter.is_active {
            publish_inactive_event(&env, meter_id, now);
        }
    }

    pub fn deduct_units(env: Env, meter_id: u64, units_consumed: i128) {
        let oracle = get_oracle_or_panic(&env);
        oracle.require_auth();

        let mut meter = get_meter_or_panic(&env, meter_id);
        let now = env.ledger().timestamp();
        reset_claim_window_if_needed(&mut meter, now);

        let requested = units_consumed.saturating_mul(meter.rate_per_second);
        let claimable = requested
            .min(remaining_claim_capacity(&meter))
            .min(provider_meter_value(&meter));

        let was_active = meter.is_active;
        apply_provider_claim(&env, &mut meter, claimable);
        meter.last_update = now;
        refresh_activity(&mut meter);

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        if was_active && !meter.is_active {
            publish_inactive_event(&env, meter_id, now);
        }

        env.events()
            .publish((symbol_short!("Usage"), meter_id), (units_consumed, claimable));
    }

    pub fn set_max_flow_rate(env: Env, meter_id: u64, max_flow_rate_per_hour: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        meter.max_flow_rate_per_hour = max_flow_rate_per_hour.max(0);
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn update_usage(env: Env, meter_id: u64, watt_hours_consumed: i128) {
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
        precise_watt_hours / precision_factor
    }

    pub fn calculate_expected_depletion(env: Env, meter_id: u64) -> Option<u64> {
        env.storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
            .map(|meter| {
                if meter.rate_per_second <= 0 {
                    return 0;
                }

                let available = provider_meter_value(&meter);
                if available <= 0 {
                    return 0;
                }

                env.ledger().timestamp() + (available / meter.rate_per_second) as u64
            })
    }

    pub fn emergency_shutdown(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
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
}

mod test;
