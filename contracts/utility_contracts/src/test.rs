#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

#[test]
fn test_utility_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let oracle = Address::generate(&env);
    
    // Setup Oracle
    client.set_oracle(&oracle);

    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin.clone());
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Initial funding
    token_admin_client.mint(&user, &1000);

    // 1. Register Meter
    let rate = 10; // 10 tokens per unit (kWh)
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    assert_eq!(meter_id, 1);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.rate_per_unit, 10);
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.is_active, false);
    assert_eq!(meter.max_flow_rate_per_hour, 36000); // 10 * 3600

    // 2. Top up
    client.top_up(&meter_id, &500);
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 500);
    assert_eq!(meter.is_active, true);
    assert_eq!(token.balance(&user), 500);
    assert_eq!(token.balance(&contract_id), 500);

    // 3. Report usage (billing by units)
    let units_consumed = 15; // 15 kWh
    client.deduct_units(&meter_id, &units_consumed);
    
    let meter = client.get_meter(&meter_id).unwrap();
    // 15 units * 10 tokens/unit = 150 tokens claimed
    assert_eq!(meter.balance, 350);
    assert_eq!(token.balance(&provider), 150);
    assert_eq!(token.balance(&contract_id), 350);

    // 4. Report usage that exceeds balance
    let more_units = 50; // 50 units * 10 = 500 cost, but only 350 left
    client.deduct_units(&meter_id, &more_units);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.is_active, false);
    assert_eq!(token.balance(&provider), 500);
    assert_eq!(token.balance(&contract_id), 0);
}

#[test]
fn test_max_flow_rate_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    
    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin.clone());
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Initial funding
    token_admin_client.mint(&user, &10000);

    // Register Meter with high rate
    let rate = 100; // 100 tokens per second
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    
    // Set a low max flow rate cap
    client.set_max_flow_rate(&meter_id, &5000); // 5000 tokens per hour max
    
    // Top up with large balance
    client.top_up(&meter_id, &10000);
    
    // Try to claim more than the hourly cap
    env.ledger().set_timestamp(env.ledger().timestamp() + 120); // 2 minutes pass
    client.claim(&meter_id);
    
    let meter = client.get_meter(&meter_id).unwrap();
    // Should be capped at 5000 tokens per hour
    assert_eq!(meter.claimed_this_hour, 5000); // 120 seconds * 100 = 12000, but capped at 5000
    assert_eq!(meter.balance, 5000); // Should have exactly 5000 remaining
}

#[test]
fn test_calculate_expected_depletion() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    
    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin.clone());
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    // Register Meter
    let rate = 10; // 10 tokens per second
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    client.top_up(&meter_id, &500);
    
    // Calculate depletion time
    let depletion_time = client.calculate_expected_depletion(&meter_id).unwrap();
    let current_time = env.ledger().timestamp();
    let expected_depletion = current_time + 50; // 500 tokens / 10 tokens per second = 50 seconds
    
    assert_eq!(depletion_time, expected_depletion);
}

#[test]
fn test_emergency_shutdown() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    
    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin.clone());
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    // Register and top up meter
    let rate = 10;
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    client.top_up(&meter_id, &500);
    
    // Verify meter is active
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.is_active, true);
    
    // Emergency shutdown
    client.emergency_shutdown(&meter_id);
    
    // Verify meter is inactive
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.is_active, false);
}

#[test]
fn test_heartbeat_functionality() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    
    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin.clone());
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    // Register meter
    let rate = 10;
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    
    // Initially should not be offline
    assert_eq!(client.is_meter_offline(&meter_id), false);
    
    // Simulate time passing more than 1 hour
    env.ledger().set_timestamp(env.ledger().timestamp() + 3700); // > 1 hour
    
    // Should now be offline
    assert_eq!(client.is_meter_offline(&meter_id), true);
    
    // Update heartbeat
    client.update_heartbeat(&meter_id);
    
    // Should no longer be offline
    assert_eq!(client.is_meter_offline(&meter_id), false);
}
