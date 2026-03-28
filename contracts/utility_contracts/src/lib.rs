#![no_std]
use soroban_sdk::{contract, contracttype, contractimpl, Address, Env, token, Symbol};

mod gas_estimator;
use gas_estimator::{GasCostEstimator, LargeScaleCostEstimate};

#[contracttype]
#[derive(Clone)]
pub struct Meter {
    pub user: Address,
    pub provider: Address,
    pub rate_per_unit: i128,
    pub balance: i128,
    pub last_update: u64,
    pub is_active: bool,
    pub token: Address,
    pub max_flow_rate_per_hour: i128,
    pub last_claim_time: u64,
    pub claimed_this_hour: i128,
    pub heartbeat: u64,
    pub parent_account: Option<Address>,
}

#[contracttype]
#[derive(Clone)]
pub struct BillingGroup {
    pub parent_account: Address,
    pub child_meters: Vec<u64>,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct WebhookConfig {
    pub url: String,
    pub user: Address,
    pub is_active: bool,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct LowBalanceAlert {
    pub meter_id: u64,
    pub user: Address,
    pub remaining_balance: i128,
    pub hours_remaining: f32,
    pub timestamp: u64,
}

#[contracttype]
pub enum DataKey {
    Meter(u64),
    Count,
    Oracle,
}

#[contract]
pub struct UtilityContract;

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
        parent_account: Option<Address>,
    ) -> u64 {
        user.require_auth();
        let mut count: u64 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);
        count += 1;

        let meter = Meter {
            user,
            provider,
            rate_per_unit: rate,
            balance: 0,
            last_update: env.ledger().timestamp(),
            is_active: false,
            token,
            max_flow_rate_per_hour: rate * 3600, // Default to 1 hour of normal flow
            last_claim_time: env.ledger().timestamp(),
            claimed_this_hour: 0,
            heartbeat: env.ledger().timestamp(),
            parent_account,
        };

        env.storage().instance().set(&DataKey::Meter(count), &meter);
        env.storage().instance().set(&DataKey::Count, &count);
        
        // If this meter has a parent account, add it to the billing group
        if let Some(parent) = parent_account {
            Self::add_meter_to_billing_group(env, parent, count);
        }
        
        count
    }

    pub fn top_up(env: Env, meter_id: u64, amount: i128) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.user.require_auth();

        let client = token::Client::new(&env, &meter.token);
        client.transfer(&meter.user, &env.current_contract_address(), &amount);

        meter.balance += amount;
        meter.is_active = true;
        meter.last_update = env.ledger().timestamp();
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn deduct_units(env: Env, meter_id: u64, units_consumed: i128) {
        let oracle: Address = env.storage().instance().get(&DataKey::Oracle).expect("Oracle address not set");
        oracle.require_auth();

        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        
        let cost = units_consumed * meter.rate_per_unit;
        
        if cost > 0 {
            let actual_claim = if cost > meter.balance {
                meter.balance
            } else {
                cost
            };

            if actual_claim > 0 {
                let client = token::Client::new(&env, &meter.token);
                client.transfer(&env.current_contract_address(), &meter.provider, &actual_claim);
                meter.balance -= actual_claim;
            }
        }

        meter.last_update = env.ledger().timestamp();
        if meter.balance <= 0 {
            meter.is_active = false;
        }

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        // Emit UsageReported event
        env.events().publish(
            (Symbol::new(&env, "UsageReported"), meter_id),
            (units_consumed, cost)
        );
    }

    pub fn get_meter(env: Env, meter_id: u64) -> Option<Meter> {
        env.storage().instance().get(&DataKey::Meter(meter_id))
    }

    pub fn calculate_expected_depletion(env: Env, meter_id: u64) -> Option<u64> {
        if let Some(meter) = env.storage().instance().get::<_, Meter>(&DataKey::Meter(meter_id)) {
            if meter.balance <= 0 || meter.rate_per_second <= 0 {
                return Some(0); // Already depleted or no consumption
            }
            
            let seconds_until_depletion = meter.balance / meter.rate_per_second;
            let current_time = env.ledger().timestamp();
            Some(current_time + seconds_until_depletion as u64)
        } else {
            None
        }
    }

    pub fn emergency_shutdown(env: Env, meter_id: u64) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.provider.require_auth();
        
        // Immediately disable the meter
        meter.is_active = false;
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn update_heartbeat(env: Env, meter_id: u64) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.user.require_auth();
        
        meter.heartbeat = env.ledger().timestamp();
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn is_meter_offline(env: Env, meter_id: u64) -> bool {
        if let Some(meter) = env.storage().instance().get::<_, Meter>(&DataKey::Meter(meter_id)) {
            let current_time = env.ledger().timestamp();
            let time_since_heartbeat = current_time.checked_sub(meter.heartbeat).unwrap_or(0);
            // Consider offline if heartbeat is > 1 hour old (3600 seconds)
            time_since_heartbeat > 3600
        } else {
            true // Meter not found, consider offline
        }
    }

    // Group Billing Functions
    pub fn create_billing_group(env: Env, parent_account: Address) {
        parent_account.require_auth();
        
        let billing_group = BillingGroup {
            parent_account: parent_account.clone(),
            child_meters: Vec::new(),
            created_at: env.ledger().timestamp(),
        };
        
        env.storage().instance().set(&DataKey::BillingGroup(parent_account), &billing_group);
    }

    fn add_meter_to_billing_group(env: Env, parent_account: Address, meter_id: u64) {
        let mut billing_group: BillingGroup = env.storage().instance()
            .get(&DataKey::BillingGroup(parent_account.clone()))
            .unwrap_or_else(|| BillingGroup {
                parent_account: parent_account.clone(),
                child_meters: Vec::new(),
                created_at: env.ledger().timestamp(),
            });
        
        // Add meter to the group if not already present
        if !billing_group.child_meters.contains(&meter_id) {
            billing_group.child_meters.push(meter_id);
            env.storage().instance().set(&DataKey::BillingGroup(parent_account), &billing_group);
        }
    }

    pub fn group_top_up(env: Env, parent_account: Address, amount_per_meter: i128) {
        parent_account.require_auth();
        
        let billing_group: BillingGroup = env.storage().instance()
            .get(&DataKey::BillingGroup(parent_account.clone()))
            .ok_or("Billing group not found").unwrap();
        
        if billing_group.child_meters.is_empty() {
            return;
        }
        
        let total_amount = amount_per_meter * billing_group.child_meters.len() as i128;
        
        // Transfer total amount from parent to contract
        if let Some(first_meter_id) = billing_group.child_meters.first() {
            if let Some(first_meter) = env.storage().instance().get::<_, Meter>(&DataKey::Meter(*first_meter_id)) {
                let client = token::Client::new(&env, &first_meter.token);
                client.transfer(&parent_account, &env.current_contract_address(), &total_amount);
            }
        }
        
        // Distribute funds to all child meters
        for &meter_id in &billing_group.child_meters {
            if let Some(mut meter) = env.storage().instance().get::<_, Meter>(&DataKey::Meter(meter_id)) {
                meter.balance += amount_per_meter;
                meter.is_active = true;
                meter.last_update = env.ledger().timestamp();
                env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
            }
        }
    }

    pub fn get_billing_group(env: Env, parent_account: Address) -> Option<BillingGroup> {
        env.storage().instance().get(&DataKey::BillingGroup(parent_account))
    }

    pub fn remove_meter_from_billing_group(env: Env, parent_account: Address, meter_id: u64) {
        parent_account.require_auth();
        
        let mut billing_group: BillingGroup = env.storage().instance()
            .get(&DataKey::BillingGroup(parent_account.clone()))
            .ok_or("Billing group not found").unwrap();
        
        billing_group.child_meters.retain(|&id| id != meter_id);
        env.storage().instance().set(&DataKey::BillingGroup(parent_account), &billing_group);
        
        // Update the meter to remove parent reference
        if let Some(mut meter) = env.storage().instance().get::<_, Meter>(&DataKey::Meter(meter_id)) {
            meter.parent_account = None;
            env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
        }
    }

    // Gas Cost Estimator Functions
    pub fn estimate_meter_monthly_cost(env: Env, is_group_meter: bool, meters_in_group: u32) -> i128 {
        GasCostEstimator::estimate_meter_monthly_cost(&env, is_group_meter, meters_in_group)
    }

    pub fn estimate_provider_monthly_cost(env: Env, number_of_meters: u32, percentage_group_meters: f32) -> i128 {
        GasCostEstimator::estimate_provider_monthly_cost(&env, number_of_meters, percentage_group_meters)
    }

    pub fn estimate_large_scale_cost(env: Env, number_of_meters: u32, group_billing_enabled: bool) -> LargeScaleCostEstimate {
        GasCostEstimator::estimate_large_scale_cost(&env, number_of_meters, group_billing_enabled)
    }

    pub fn get_operation_cost(_env: Env, operation: String) -> i128 {
        GasCostEstimator::get_operation_cost(&operation)
    }

    // Webhook and Alert Functions
    pub fn configure_webhook(env: Env, user: Address, webhook_url: String) {
        user.require_auth();
        
        let webhook_config = WebhookConfig {
            url: webhook_url.clone(),
            user: user.clone(),
            is_active: true,
            created_at: env.ledger().timestamp(),
        };
        
        env.storage().instance().set(&DataKey::WebhookConfig(user), &webhook_config);
    }

    pub fn deactivate_webhook(env: Env, user: Address) {
        user.require_auth();
        
        if let Some(mut config) = env.storage().instance().get::<_, WebhookConfig>(&DataKey::WebhookConfig(user.clone())) {
            config.is_active = false;
            env.storage().instance().set(&DataKey::WebhookConfig(user), &config);
        }
    }

    pub fn get_webhook_config(env: Env, user: Address) -> Option<WebhookConfig> {
        env.storage().instance().get(&DataKey::WebhookConfig(user))
    }

    fn check_and_send_low_balance_alert(env: &Env, meter: &Meter, meter_id: u64) {
        // Only check if webhook is configured for this user
        let webhook_config = match env.storage().instance().get::<_, WebhookConfig>(&DataKey::WebhookConfig(meter.user.clone())) {
            Some(config) if config.is_active => config,
            _ => return, // No active webhook configured
        };

        // Calculate hours remaining
        let hours_remaining = if meter.rate_per_second > 0 {
            meter.balance as f32 / meter.rate_per_second as f32 / 3600.0
        } else {
            f32::INFINITY
        };

        // Check if balance is low (< 24 hours)
        if hours_remaining < 24.0 {
            // Check if we've sent an alert recently (within last 12 hours)
            let current_time = env.ledger().timestamp();
            let last_alert_time: Option<u64> = env.storage().instance().get(&DataKey::LastAlert(meter_id));
            
            if let Some(last_time) = last_alert_time {
                if current_time.checked_sub(last_time).unwrap_or(0) < 43200 { // 12 hours in seconds
                    return; // Already sent alert recently
                }
            }

            // Create and send alert
            let alert = LowBalanceAlert {
                meter_id,
                user: meter.user.clone(),
                remaining_balance: meter.balance,
                hours_remaining,
                timestamp: current_time,
            };

            // Store the alert timestamp
            env.storage().instance().set(&DataKey::LastAlert(meter_id), &current_time);

            // In a real implementation, this would make an HTTP call to the webhook
            // For now, we'll store the alert in contract storage for demonstration
            let alert_key = format!("alert:{}:{}", meter_id, current_time);
            env.storage().instance().set(&alert_key, &alert);
        }
    }

    pub fn get_pending_alerts(env: Env, user: Address) -> Vec<LowBalanceAlert> {
        let mut alerts = Vec::new();
        
        // This is a simplified implementation
        // In practice, you'd want to iterate through storage more efficiently
        let count: u64 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);
        
        for meter_id in 1..=count {
            if let Some(meter) = env.storage().instance().get::<_, Meter>(&DataKey::Meter(meter_id)) {
                if meter.user == user {
                    // Check for recent alerts
                    let current_time = env.ledger().timestamp();
                    let alert_key = format!("alert:{}:{}", meter_id, current_time);
                    if let Some(alert) = env.storage().instance().get::<_, LowBalanceAlert>(&alert_key) {
                        alerts.push(alert);
                    }
                }
            }
        }
        
        alerts
    }

    // Enhanced claim function with webhook integration
    pub fn claim_with_alerts(env: Env, meter_id: u64) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.provider.require_auth();

        let now = env.ledger().timestamp();
        let elapsed = now.checked_sub(meter.last_update).unwrap_or(0);
        let amount = (elapsed as i128) * meter.rate_per_second;
        
        // Check if we need to reset the hourly counter
        let hours_passed = now.checked_sub(meter.last_claim_time).unwrap_or(0) / 3600;
        if hours_passed >= 1 {
            meter.claimed_this_hour = 0;
            meter.last_claim_time = now;
        }
        
        // Ensure we don't overdraw the balance
        let claimable = if amount > meter.balance {
            meter.balance
        } else {
            amount
        };
        
        // Apply max flow rate cap
        let final_claimable = if claimable > 0 {
            let remaining_hourly_capacity = meter.max_flow_rate_per_hour - meter.claimed_this_hour;
            if claimable > remaining_hourly_capacity {
                remaining_hourly_capacity
            } else {
                claimable
            }
        } else {
            0
        };

        if final_claimable > 0 {
            let client = token::Client::new(&env, &meter.token);
            client.transfer(&env.current_contract_address(), &meter.provider, &final_claimable);
            meter.balance -= final_claimable;
            meter.claimed_this_hour += final_claimable;
        }

        meter.last_update = now;
        if meter.balance <= 0 {
            meter.is_active = false;
        }

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        // Check for low balance and send alert if needed
        Self::check_and_send_low_balance_alert(&env, &meter, meter_id);
    }
}

mod test;
