#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

#[test]
fn test_prepaid_meter_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);

    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Initial funding
    token_admin_client.mint(&user, &1000);

    // 1. Register Meter
    let rate = 10; // 10 tokens per second
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    assert_eq!(meter_id, 1);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.billing_type, BillingType::PrePaid);
    assert_eq!(meter.rate_per_second, 10);
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.debt, 0);
    assert_eq!(meter.collateral_limit, 0);
    assert_eq!(meter.is_active, false);

    // 2. Top up
    client.top_up(&meter_id, &500);
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 500);
    assert_eq!(meter.debt, 0);
    assert_eq!(meter.collateral_limit, 0);
    assert_eq!(meter.is_active, true);
    assert_eq!(token.balance(&user), 500);
    assert_eq!(token.balance(&contract_id), 500);

    // 3. Claim balance (simulate time passing)
    env.ledger().set_timestamp(env.ledger().timestamp() + 10); // 10 seconds pass
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    // 10 seconds * 10 tokens/sec = 100 tokens claimed
    assert_eq!(meter.balance, 400);
    assert_eq!(meter.debt, 0);
    assert_eq!(token.balance(&provider), 100);
    assert_eq!(token.balance(&contract_id), 400);

    // 4. Claim more than balance
    env.ledger().set_timestamp(env.ledger().timestamp() + 50); // 50 seconds pass
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.is_active, false);
    assert_eq!(token.balance(&provider), 500);
    assert_eq!(token.balance(&contract_id), 0);
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
