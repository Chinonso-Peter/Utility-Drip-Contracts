use super::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

#[test]
fn test_extreme_usage_values() {
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
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);
    token_admin_client.mint(&user, &1_000_000_000_000i128);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id =
        client.register_meter(&user, &provider, &100, &token_address, &device_public_key);

    client.top_up(&meter_id, &1_000_000_000_000i128);

    // Test large (but valid) usage updates
    let extreme_values: [i128; 3] = [1_000_000_000i128, 10_000_000_000i128, 100_000_000_000i128];

    for &usage in extreme_values.iter() {
        client.update_usage(&meter_id, &usage);
        let usage_data = client.get_usage_data(&meter_id);
        assert!(usage_data.is_some());
        let data = usage_data.unwrap();
        assert!(data.total_watt_hours >= 0);
        assert!(data.current_cycle_watt_hours >= 0);
        assert!(data.peak_usage_watt_hours >= 0);
    }
}

#[test]
fn test_precision_factor_extremes() {
    let extreme_precision_factors: [i128; 5] =
        [1, 1000, 1_000_000, 1_000_000_000, i128::MAX / 1000];

    let test_usage = 1_000_000_000i128;

    for &precision in extreme_precision_factors.iter() {
        let precise_consumption = test_usage.saturating_mul(precision);
        assert!(precise_consumption >= 0);

        if precision != 0 {
            let display = test_usage / precision;
            assert!(display >= 0);
        }
    }
}

#[test]
fn test_arithmetic_edge_cases() {
    let edge_cases: [i128; 7] = [i128::MAX, i128::MIN, i128::MAX - 1, i128::MIN + 1, 0, -1, 1];

    for &value in edge_cases.iter() {
        let _a = value.saturating_add(1);
        let _b = value.saturating_mul(1000);
        let _c = value.saturating_sub(1);

        if value != 0 {
            let _d = 1000i128 / value;
        }
    }
}

#[test]
fn test_cumulative_extreme_usage() {
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
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);
    token_admin_client.mint(&user, &i128::MAX);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id =
        client.register_meter(&user, &provider, &100, &token_address, &device_public_key);

    client.top_up(&meter_id, &1_000_000_000_000i128);

    let extreme_usage = 1_000_000_000i128;

    for i in 0u64..10 {
        let cumulative_usage = extreme_usage.saturating_mul((i + 1) as i128);
        client.update_usage(&meter_id, &cumulative_usage);

        let usage_data = client.get_usage_data(&meter_id);
        assert!(usage_data.is_some());

        let data = usage_data.unwrap();
        assert!(data.total_watt_hours >= 0);
        assert!(data.current_cycle_watt_hours >= 0);
        assert!(data.peak_usage_watt_hours >= 0);
    }
}

#[test]
fn test_debt_calculation_underflow_protection() {
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
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);

    // Test PostPaid meter with extreme debt scenarios
    let meter_id = client.register_meter_with_mode(
        &user,
        &provider,
        &1000, // High rate
        &token_address,
        &BillingType::PostPaid,
        &device_public_key,
    );

    // Test 1: High rate, long duration, zero balance scenario
    client.top_up(&meter_id, &1000000); // Initial collateral

    // Pair the meter for usage deduction
    let challenge = client.initiate_pairing(&meter_id);
    let signature = BytesN::from_array(&env, &[2u8; 64]);
    client.complete_pairing(&meter_id, &signature);

    // Simulate extreme usage that would cause massive debt accumulation
    let extreme_usage = SignedUsageData {
        meter_id,
        timestamp: env.ledger().timestamp(),
        watt_hours_consumed: 100_000_000_000i128, // 100 billion Wh
        units_consumed: 10_000_000i128,           // 10 million units
        signature: BytesN::from_array(&env, &[3u8; 64]),
        public_key: device_public_key.clone(),
    };

    // This should not panic even with extreme values
    client.deduct_units(&extreme_usage);

    let meter = client.get_meter(&meter_id).unwrap();

    // Verify debt is non-negative (critical underflow protection)
    assert!(meter.debt >= 0, "Debt should never be negative");

    // Verify debt accumulated correctly
    assert!(meter.debt > 0, "Debt should be positive after usage");

    // Test 2: Verify debt clears correctly on top-up
    let debt_to_clear = meter.debt;
    token_admin_client.mint(&user, &debt_to_clear);

    client.top_up(&meter_id, &debt_to_clear);

    let meter_after_settlement = client.get_meter(&meter_id).unwrap();

    // Debt should be fully cleared
    assert_eq!(
        meter_after_settlement.debt, 0,
        "Debt should be fully cleared after sufficient top-up"
    );

    // Collateral should be properly updated
    assert!(
        meter_after_settlement.collateral_limit >= 0,
        "Collateral limit should remain non-negative"
    );

    // Test 3: Test with maximum safe values to ensure no underflow
    let max_safe_usage = SignedUsageData {
        meter_id,
        timestamp: env.ledger().timestamp(),
        watt_hours_consumed: i128::MAX / 1_000_000, // Safe maximum
        units_consumed: i128::MAX / 1_000_000_000,  // Safe maximum
        signature: BytesN::from_array(&env, &[4u8; 64]),
        public_key: device_public_key.clone(),
    };

    // Should handle maximum values without panicking
    client.deduct_units(&max_safe_usage);

    let max_meter = client.get_meter(&meter_id).unwrap();

    // All values should remain within valid i128 range
    assert!(max_meter.debt >= 0 && max_meter.debt <= i128::MAX);
    assert!(max_meter.collateral_limit >= 0 && max_meter.collateral_limit <= i128::MAX);
    assert!(max_meter.balance >= i128::MIN && max_meter.balance <= i128::MAX);
}

#[test]
fn test_prepaid_negative_balance_handling() {
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

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);

    // Test PrePaid meter with zero balance
    let meter_id = client.register_meter_with_mode(
        &user,
        &provider,
        &1000, // High rate
        &token_address,
        &BillingType::PrePaid,
        &device_public_key,
    );

    // Pair the meter
    let challenge = client.initiate_pairing(&meter_id);
    let signature = BytesN::from_array(&env, &[2u8; 64]);
    client.complete_pairing(&meter_id, &signature);

    // Try to deduct units with zero balance - should handle gracefully
    let zero_balance_usage = SignedUsageData {
        meter_id,
        timestamp: env.ledger().timestamp(),
        watt_hours_consumed: 10_000_000i128,
        units_consumed: 100_000i128,
        signature: BytesN::from_array(&env, &[3u8; 64]),
        public_key: device_public_key.clone(),
    };

    // This should not panic, even with zero balance
    client.deduct_units(&zero_balance_usage);

    let meter = client.get_meter(&meter_id).unwrap();

    // Balance should be negative or zero, but never cause underflow
    assert!(
        meter.balance <= 0,
        "Balance should be zero or negative with insufficient funds"
    );

    // Meter should be inactive with negative balance
    assert!(
        !meter.is_active,
        "Meter should be inactive with negative balance"
    );

    // Balance should be within valid i128 range
    assert!(meter.balance >= i128::MIN && meter.balance <= i128::MAX);
}
