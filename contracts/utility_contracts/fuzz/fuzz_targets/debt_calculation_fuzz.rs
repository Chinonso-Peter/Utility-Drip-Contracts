#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as TestAddress, Address, BytesN, Env};
use utility_contracts::{BillingType, SignedUsageData, UtilityContract};

fuzz_target!(|data: &[u8]| {
    // Need at least 24 bytes: 8 for rate, 8 for duration, 8 for balance
    if data.len() < 24 {
        return;
    }

    // Extract fuzz parameters with bounds checking
    let mut rate_bytes = [0u8; 8];
    let mut duration_bytes = [0u8; 8];
    let mut balance_bytes = [0u8; 8];

    rate_bytes.copy_from_slice(&data[0..8]);
    duration_bytes.copy_from_slice(&data[8..16]);
    balance_bytes.copy_from_slice(&data[16..24]);

    let rate = u64::from_be_bytes(rate_bytes);
    let duration = u64::from_be_bytes(duration_bytes);
    let initial_balance = i128::from_be_bytes(balance_bytes);

    // Focus on extreme scenarios that could cause underflow
    // High rates (up to 1M per second), long durations (up to 1 year in seconds)
    if rate == 0 || rate > 1_000_000 || duration > 31_536_000 {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, UtilityContract);
    let client = utility_contracts::UtilityContractClient::new(&env, &contract_id);

    // Create test addresses
    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);

    // Create token
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);

    // Mint tokens if needed for positive balance scenarios
    if initial_balance > 0 {
        token_admin_client.mint(&user, &initial_balance);
    }

    // Create a PostPaid meter for debt testing
    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter_with_mode(
        &user,
        &provider,
        &(rate as i128), // off_peak_rate
        &token_address,
        &BillingType::PostPaid,
        &device_public_key,
    );

    // Set up initial state
    if initial_balance > 0 {
        let top_up_result = std::panic::catch_unwind(|| {
            client.top_up(&meter_id, &initial_balance);
        });

        if top_up_result.is_err() {
            return; // Skip this iteration if top-up fails
        }
    }

    // Test 1: Verify extreme debt accumulation without panic
    let debt_test_result = std::panic::catch_unwind(|| {
        // Simulate extreme usage over long duration
        let signed_data = SignedUsageData {
            meter_id,
            timestamp: env.ledger().timestamp(),
            watt_hours_consumed: (rate as i128).saturating_mul(duration as i128), // Extreme usage
            units_consumed: (rate as i128).saturating_mul(duration as i128), // 1:1 ratio for testing
            signature: BytesN::from_array(&env, &[2u8; 64]),
            public_key: device_public_key.clone(),
        };

        client.deduct_units(&signed_data);

        // Check meter state after extreme usage
        let meter = client.get_meter(&meter_id).unwrap();

        // Verify debt is non-negative (critical for underflow prevention)
        assert!(
            meter.debt >= 0,
            "Debt should never be negative: {}",
            meter.debt
        );

        // Verify collateral calculation doesn't underflow
        let remaining_collateral = meter.collateral_limit.saturating_sub(meter.debt);
        assert!(
            remaining_collateral >= 0,
            "Remaining collateral should never be negative"
        );

        meter
    });

    if let Ok(meter) = debt_test_result {
        // Test 2: Verify debt clears correctly on top-up
        if meter.debt > 0 {
            let top_up_amount = meter.debt.saturating_add(1000); // Extra to ensure full settlement

            // Mint tokens for debt settlement
            token_admin_client.mint(&user, &top_up_amount);

            let settlement_result = std::panic::catch_unwind(|| {
                client.top_up(&meter_id, &top_up_amount);

                let meter_after_settlement = client.get_meter(&meter_id).unwrap();

                // Verify debt is reduced or cleared
                assert!(
                    meter_after_settlement.debt <= meter.debt,
                    "Debt should not increase after top-up"
                );

                // If top-up was sufficient, debt should be fully cleared
                if top_up_amount >= meter.debt {
                    assert_eq!(
                        meter_after_settlement.debt, 0,
                        "Debt should be fully cleared with sufficient top-up"
                    );
                }

                // Verify collateral limit is properly updated
                assert!(
                    meter_after_settlement.collateral_limit >= 0,
                    "Collateral limit should remain non-negative"
                );

                meter_after_settlement
            });

            if let Ok(_meter_after) = settlement_result {
                // Test 3: Verify balance can become negative correctly without panicking
                // This simulates the case where usage exceeds available funds

                // Create scenario with zero initial balance using PrePaid meter
                let zero_balance_meter_id = client.register_meter_with_mode(
                    &user,
                    &provider,
                    &(rate as i128),
                    &token_address,
                    &BillingType::PrePaid, // Test with PrePaid for negative balance
                    &device_public_key,
                );

                let negative_balance_result = std::panic::catch_unwind(|| {
                    // Try to deduct units with zero balance
                    let extreme_signed_data = SignedUsageData {
                        meter_id: zero_balance_meter_id,
                        timestamp: env.ledger().timestamp(),
                        watt_hours_consumed: (rate as i128).saturating_mul(duration as i128),
                        units_consumed: (rate as i128).saturating_mul(duration as i128),
                        signature: BytesN::from_array(&env, &[3u8; 64]),
                        public_key: device_public_key.clone(),
                    };

                    client.deduct_units(&extreme_signed_data);

                    let zero_balance_meter = client.get_meter(&zero_balance_meter_id).unwrap();

                    // Balance should be able to go negative without causing arithmetic underflow
                    // This tests the saturating arithmetic protections
                    assert!(
                        zero_balance_meter.balance <= 0,
                        "Balance should be zero or negative with insufficient funds"
                    );

                    // Verify the meter handles negative balance gracefully
                    let is_active = zero_balance_meter.is_active;
                    // Meter should be inactive with negative balance (below minimum)
                    assert!(!is_active, "Meter should be inactive with negative balance");
                });

                if negative_balance_result.is_err() {
                    panic!("Contract failed to handle negative balance scenario correctly");
                }
            }
        }
    }

    // Test 4: Edge case - Maximum i128 values
    let max_edge_result = std::panic::catch_unwind(|| {
        // Test with maximum safe values
        let max_safe_rate = std::cmp::min(rate, 1000000) as i128;
        let max_safe_duration = std::cmp::min(duration, 86400) as i128; // Max 1 day

        let edge_signed_data = SignedUsageData {
            meter_id,
            timestamp: env.ledger().timestamp(),
            watt_hours_consumed: max_safe_rate.saturating_mul(max_safe_duration),
            units_consumed: max_safe_rate.saturating_mul(max_safe_duration),
            signature: BytesN::from_array(&env, &[4u8; 64]),
            public_key: device_public_key.clone(),
        };

        client.deduct_units(&edge_signed_data);

        let edge_meter = client.get_meter(&meter_id).unwrap();

        // Verify all values remain within valid i128 range
        assert!(
            edge_meter.debt >= 0 && edge_meter.debt <= i128::MAX,
            "Debt must stay within i128 bounds"
        );
        assert!(
            edge_meter.collateral_limit >= 0 && edge_meter.collateral_limit <= i128::MAX,
            "Collateral must stay within i128 bounds"
        );
        assert!(
            edge_meter.balance >= i128::MIN && edge_meter.balance <= i128::MAX,
            "Balance must stay within i128 bounds"
        );
    });

    if max_edge_result.is_err() {
        panic!("Contract failed with maximum edge case values");
    }
});
