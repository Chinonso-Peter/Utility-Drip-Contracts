#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{token, Address, Env};

#[test]
fn test_prepaid_meter_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let oracle = Address::generate(&env);
    client.set_oracle(&oracle);

    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let rate = 10;
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    assert_eq!(meter_id, 1);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.billing_type, BillingType::PrePaid);
    assert_eq!(meter.rate_per_second, 10);
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.debt, 0);
    assert_eq!(meter.collateral_limit, 0);
    assert_eq!(meter.is_active, false);
    assert_eq!(meter.usage_data.total_watt_hours, 0);
    assert_eq!(meter.usage_data.current_cycle_watt_hours, 0);
    assert_eq!(meter.usage_data.peak_usage_watt_hours, 0);
    assert_eq!(meter.usage_data.precision_factor, 1000);
    assert_eq!(meter.max_flow_rate_per_hour, 36000);

    client.top_up(&meter_id, &500);
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 500);
    assert_eq!(meter.debt, 0);
    assert_eq!(meter.collateral_limit, 0);
    assert_eq!(meter.is_active, true);
    assert_eq!(token.balance(&user), 500);
    assert_eq!(token.balance(&contract_id), 500);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 400);
    assert_eq!(meter.debt, 0);
    assert_eq!(token.balance(&provider), 100);
    assert_eq!(token.balance(&contract_id), 400);

    let units_consumed = 15;
    client.deduct_units(&meter_id, &units_consumed);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 250);
    assert_eq!(token.balance(&provider), 250);
    assert_eq!(token.balance(&contract_id), 250);

    let more_units = 50;
    client.deduct_units(&meter_id, &more_units);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.is_active, false);
    assert_eq!(token.balance(&provider), 500);
    assert_eq!(token.balance(&contract_id), 0);

    client.update_usage(&meter_id, &1500);
    let usage_data = client.get_usage_data(&meter_id).unwrap();
    assert_eq!(usage_data.total_watt_hours, 1500000);
    assert_eq!(usage_data.current_cycle_watt_hours, 1500000);
    assert_eq!(usage_data.peak_usage_watt_hours, 1500000);

    client.reset_cycle_usage(&meter_id);
    let usage_data = client.get_usage_data(&meter_id).unwrap();
    assert_eq!(usage_data.total_watt_hours, 1500000);
    assert_eq!(usage_data.current_cycle_watt_hours, 0);
    assert_eq!(usage_data.peak_usage_watt_hours, 1500000);

    client.update_usage(&meter_id, &2000);
    let usage_data = client.get_usage_data(&meter_id).unwrap();
    assert_eq!(usage_data.total_watt_hours, 3500000);
    assert_eq!(usage_data.current_cycle_watt_hours, 2000000);
    assert_eq!(usage_data.peak_usage_watt_hours, 2000000);

    let display_total =
        UtilityContract::get_watt_hours_display(usage_data.total_watt_hours, usage_data.precision_factor);
    assert_eq!(display_total, 3500);
}

#[test]
fn test_max_flow_rate_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);

    let oracle = Address::generate(&env);

    client.set_oracle(&oracle);
    
    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &10000);

    let rate = 100;
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    client.set_max_flow_rate(&meter_id, &5000);
    client.top_up(&meter_id, &10000);

    env.ledger().set_timestamp(env.ledger().timestamp() + 120);
    client.claim(&meter_id);

    
    // Try to deduct more than the hourly cap (simulated via Oracle)
    // 120 units at rate 100 should cost 12000, which exceeds 5000 cap
    client.deduct_units(&meter_id, &120);
    
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.claimed_this_hour, 5000);
    assert_eq!(meter.balance, 5000);
}

#[test]
fn test_calculate_expected_depletion() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let rate = 10;
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    client.top_up(&meter_id, &500);

    let depletion_time = client.calculate_expected_depletion(&meter_id).unwrap();
    let current_time = env.ledger().timestamp();
    let expected_depletion = current_time + 50;

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

    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let rate = 10;
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    client.top_up(&meter_id, &500);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.is_active, true);

    client.emergency_shutdown(&meter_id);

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

    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let rate = 10;
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);

    assert_eq!(client.is_meter_offline(&meter_id), false);

    env.ledger().set_timestamp(env.ledger().timestamp() + 3700);
    assert_eq!(client.is_meter_offline(&meter_id), true);

    client.update_heartbeat(&meter_id);
    assert_eq!(client.is_meter_offline(&meter_id), false);
}

#[test]
fn test_postpaid_claims_against_collateral_limit() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &500);

    let meter_id = client.register_meter_with_mode(
        &user,
        &provider,
        &10,
        &token_address,
        &BillingType::PostPaid,
    );

    client.top_up(&meter_id, &300);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.billing_type, BillingType::PostPaid);
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.debt, 0);
    assert_eq!(meter.collateral_limit, 300);
    assert_eq!(meter.is_active, true);
    assert_eq!(token.balance(&contract_id), 300);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 100);
    assert_eq!(meter.collateral_limit, 300);
    assert_eq!(meter.is_active, true);
    assert_eq!(token.balance(&provider), 100);
    assert_eq!(token.balance(&contract_id), 200);

    env.ledger().set_timestamp(env.ledger().timestamp() + 25);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 300);
    assert_eq!(meter.collateral_limit, 300);
    assert_eq!(meter.is_active, false);
    assert_eq!(token.balance(&provider), 300);
    assert_eq!(token.balance(&contract_id), 0);
}

#[test]
fn test_postpaid_top_up_settles_debt_and_resets_when_reactivated() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &500);

    let meter_id = client.register_meter_with_mode(
        &user,
        &provider,
        &10,
        &token_address,
        &BillingType::PostPaid,
    );

    client.top_up(&meter_id, &100);
    env.ledger().set_timestamp(env.ledger().timestamp() + 20);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 100);
    assert_eq!(meter.is_active, false);
    assert_eq!(token.balance(&provider), 100);

    env.ledger().set_timestamp(env.ledger().timestamp() + 80);
    client.top_up(&meter_id, &150);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 0);
    assert_eq!(meter.collateral_limit, 150);
    assert_eq!(meter.is_active, true);
    assert_eq!(token.balance(&contract_id), 150);

    env.ledger().set_timestamp(env.ledger().timestamp() + 5);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 50);
    assert_eq!(meter.collateral_limit, 150);
    assert_eq!(meter.is_active, true);
    assert_eq!(token.balance(&provider), 150);
    assert_eq!(token.balance(&contract_id), 100);
}
