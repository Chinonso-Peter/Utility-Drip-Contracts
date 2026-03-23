#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BillingType {
    PrePaid,
    PostPaid,
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
}

#[contracttype]
pub enum DataKey {
    Meter(u64),
    Count,
}

#[contract]
pub struct UtilityContract;

fn get_meter(env: &Env, meter_id: u64) -> Meter {
    env.storage()
        .instance()
        .get(&DataKey::Meter(meter_id))
        .ok_or("Meter not found")
        .unwrap()
}

fn remaining_postpaid_collateral(meter: &Meter) -> i128 {
    meter.collateral_limit.saturating_sub(meter.debt)
}

fn refresh_activity(meter: &mut Meter) {
    meter.is_active = match meter.billing_type {
        BillingType::PrePaid => meter.balance > 0,
        BillingType::PostPaid => remaining_postpaid_collateral(meter) > 0,
    };
}

#[contractimpl]
impl UtilityContract {
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
        let mut count: u64 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);
        count += 1;

        let meter = Meter {
            user,
            provider,
            billing_type,
            rate_per_second: rate,
            balance: 0,
            debt: 0,
            collateral_limit: 0,
            last_update: env.ledger().timestamp(),
            is_active: false,
            token,
        };

        env.storage().instance().set(&DataKey::Meter(count), &meter);
        env.storage().instance().set(&DataKey::Count, &count);
        count
    }

    pub fn top_up(env: Env, meter_id: u64, amount: i128) {
        let mut meter = get_meter(&env, meter_id);
        meter.user.require_auth();
        let was_active = meter.is_active;

        let client = token::Client::new(&env, &meter.token);
        client.transfer(&meter.user, &env.current_contract_address(), &amount);

        match meter.billing_type {
            BillingType::PrePaid => {
                meter.balance += amount;
            }
            BillingType::PostPaid => {
                let settlement = amount.min(meter.debt);
                meter.debt -= settlement;
                meter.collateral_limit += amount.saturating_sub(settlement);
            }
        }

        refresh_activity(&mut meter);
        if !was_active && meter.is_active {
            meter.last_update = env.ledger().timestamp();
        }

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn claim(env: Env, meter_id: u64) {
        let mut meter = get_meter(&env, meter_id);
        meter.provider.require_auth();

        let now = env.ledger().timestamp();
        if !meter.is_active {
            meter.last_update = now;
            env.storage()
                .instance()
                .set(&DataKey::Meter(meter_id), &meter);
            return;
        }

        let elapsed = now.checked_sub(meter.last_update).unwrap_or(0);
        let amount = (elapsed as i128).saturating_mul(meter.rate_per_second);

        let claimable = match meter.billing_type {
            BillingType::PrePaid => amount.min(meter.balance),
            BillingType::PostPaid => amount.min(remaining_postpaid_collateral(&meter)),
        };

        if claimable > 0 {
            let client = token::Client::new(&env, &meter.token);
            client.transfer(&env.current_contract_address(), &meter.provider, &claimable);
            match meter.billing_type {
                BillingType::PrePaid => {
                    meter.balance -= claimable;
                }
                BillingType::PostPaid => {
                    meter.debt += claimable;
                }
            }
        }

        meter.last_update = now;
        refresh_activity(&mut meter);

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn get_meter(env: Env, meter_id: u64) -> Option<Meter> {
        env.storage().instance().get(&DataKey::Meter(meter_id))
    }
}

mod test;
