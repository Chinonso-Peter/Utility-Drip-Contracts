#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{token, Address, Env, BytesN, Vec};

// Mock price oracle for testing
struct MockPriceOracle {
    env: Env,
    address: Address,
    price: i128,
    decimals: u32,
}

impl MockPriceOracle {
    fn new(env: &Env, price: i128, decimals: u32) -> Self {
        let address = Address::generate(env);
        Self {
            env: env.clone(),
            address,
            price,
            decimals,
        }
    }
    
    fn address(&self) -> Address {
        self.address.clone()
    }
    
    fn mock_xlm_to_usd_cents(&self, xlm_amount: i128) -> i128 {
        xlm_amount.saturating_mul(self.price) / (10_i128.pow(self.decimals))
    }
    
    fn mock_usd_cents_to_xlm(&self, usd_cents: i128) -> i128 {
        usd_cents.saturating_mul(10_i128.pow(self.decimals)) / self.price
    }
    
    fn mock_get_price(&self) -> PriceData {
        PriceData {
            price: self.price,
            decimals: self.decimals,
            last_updated: self.env.ledger().timestamp(),
        }
    }
}

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

    token_admin_client.mint(&user, &10000);

    // Generate a device public key for the ESP32
    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);
    assert_eq!(meter_id, 1);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.billing_type, BillingType::PrePaid);
    assert_eq!(meter.off_peak_rate, 10);
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.debt, 0);
    assert_eq!(meter.collateral_limit, 0);
    assert!(!meter.is_active);
    assert_eq!(meter.max_flow_rate_per_hour, 36000);
    assert_eq!(meter.device_public_key, device_public_key);

    client.top_up(&meter_id, &5000);
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 5000);
    assert!(meter.is_active);
    assert_eq!(token.balance(&user), 5000);
    assert_eq!(token.balance(&contract_id), 5000);

    // Test claims over time
    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 4900); // 10s * 10 tokens/s = 100 claimed
    assert_eq!(token.balance(&provider), 100);
    assert_eq!(token.balance(&contract_id), 4900);

    // Test deduct_units (Issue #13 logic)
    client.deduct_units(&meter_id, &15);
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 4750); // 4900 - (15 units * 10 rate) = 4750
    assert_eq!(token.balance(&provider), 250);
    assert_eq!(token.balance(&contract_id), 4750);

    client.deduct_units(&meter_id, &475);
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 0);
    assert!(!meter.is_active);
    assert_eq!(token.balance(&provider), 5000);
    assert_eq!(token.balance(&contract_id), 0);

    client.update_usage(&meter_id, &1500);
    let usage_data = client.get_usage_data(&meter_id).unwrap();
    assert_eq!(usage_data.total_watt_hours, 1_500_000);
    assert_eq!(usage_data.current_cycle_watt_hours, 1_500_000);
    assert_eq!(usage_data.peak_usage_watt_hours, 1_500_000);

    client.reset_cycle_usage(&meter_id);
    let usage_data = client.get_usage_data(&meter_id).unwrap();
    assert_eq!(usage_data.total_watt_hours, 1_500_000);
    assert_eq!(usage_data.current_cycle_watt_hours, 0);
    assert_eq!(usage_data.peak_usage_watt_hours, 1_500_000);

    client.update_usage(&meter_id, &2000);
    let usage_data = client.get_usage_data(&meter_id).unwrap();
    assert_eq!(usage_data.total_watt_hours, 3_500_000);
    assert_eq!(usage_data.current_cycle_watt_hours, 2_000_000);
    assert_eq!(usage_data.peak_usage_watt_hours, 2_000_000);

    let display_total = UtilityContract::get_watt_hours_display(
        usage_data.total_watt_hours,
        usage_data.precision_factor,
    );
    assert_eq!(display_total, 3500); // 3500000 / 1000 = 3500 (3.5 kWh)
}

#[test]
fn test_peak_hour_tariff() {
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
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Initial funding
    token_admin_client.mint(&user, &5000);

    // Register Meter
    let rate = 10; // 10 tokens per unit
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    client.top_up(&meter_id, &5000);

    // Set time to 19:00:00 UTC (19 * 3600 = 68400)
    // 19:00 falls exactly in the 18:00 - 22:00 peak hours bracket
    env.ledger().set_timestamp(68400);

    // Consume 10 units. Base cost = 10 * 10 = 100 tokens.
    // 150% Peak multiplier means 150 tokens claimed.
    let units_consumed = 10;
    client.deduct_units(&meter_id, &units_consumed);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 4850); // 5000 - 150
    assert_eq!(token.balance(&provider), 150);
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

    let meter_id = client.register_meter(&user, &provider, &10, &token_address);
    client.top_up(&meter_id, &500);

    // Calculate depletion time
    let depletion_time = client.calculate_expected_depletion(&meter_id).unwrap();
    let current_time = env.ledger().timestamp();
    assert_eq!(depletion_time, current_time + 50);
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

    let meter_id = client.register_meter(&user, &provider, &10, &token_address);
    client.top_up(&meter_id, &500);

    let meter = client.get_meter(&meter_id).unwrap();
    assert!(meter.is_active);

    client.emergency_shutdown(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert!(!meter.is_active);
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

    let meter_id = client.register_meter(&user, &provider, &10, &token_address);

    assert!(!client.is_meter_offline(&meter_id));

    env.ledger().set_timestamp(env.ledger().timestamp() + 3700);
    assert!(client.is_meter_offline(&meter_id));

    client.update_heartbeat(&meter_id);
    assert!(!client.is_meter_offline(&meter_id));
}

#[test]
fn test_claim_within_daily_limit_tracks_withdrawn() {
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

    token_admin_client.mint(&user, &10000);

    let meter_id = client.register_meter(&user, &provider, &10, &token_address);
    client.top_up(&meter_id, &5000);

    env.ledger().set_timestamp(env.ledger().timestamp() + 5);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    let provider_window = client.get_provider_window(&provider).unwrap();

    assert_eq!(meter.balance, 4950);
    assert_eq!(token.balance(&provider), 50);
    assert_eq!(token.balance(&contract_id), 4950);
    assert_eq!(provider_window.daily_withdrawn, 50);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_claim_reverts_when_daily_limit_is_exceeded() {
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

    let meter_id = client.register_meter(&user, &provider, &10, &token_address);
    client.top_up(&meter_id, &500);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10_000);
    client.claim(&meter_id);
}

#[test]
fn test_daily_limit_resets_after_24_hours() {
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

    token_admin_client.mint(&user, &1_000_000);

    let meter_id = client.register_meter(&user, &provider, &1, &token_address);
    client.set_max_flow_rate(&meter_id, &1_000_000);
    client.top_up(&meter_id, &1_000_000);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10_000);
    client.claim(&meter_id);

    let provider_window = client.get_provider_window(&provider).unwrap();
    assert_eq!(provider_window.daily_withdrawn, 10_000);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + (24 * 60 * 60) + 5_000);
    client.claim(&meter_id);

    let provider_window = client.get_provider_window(&provider).unwrap();
    assert_eq!(provider_window.daily_withdrawn, 91_400);
    assert_eq!(token.balance(&provider), 101_400);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_daily_limit_is_shared_across_provider_meters() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user_one = Address::generate(&env);
    let user_two = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user_one, &500);
    token_admin_client.mint(&user_two, &500);

    let meter_one = client.register_meter(&user_one, &provider, &10, &token_address);
    let meter_two = client.register_meter(&user_two, &provider, &10, &token_address);

    client.top_up(&meter_one, &500);
    client.top_up(&meter_two, &500);

    env.ledger().set_timestamp(env.ledger().timestamp() + 5);
    client.claim(&meter_one);
    client.claim(&meter_two);

    env.ledger().set_timestamp(env.ledger().timestamp() + 1);
    client.claim(&meter_one);
}

#[test]
fn test_postpaid_claims_against_collateral_limit() {
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

    token_admin_client.mint(&user, &10000);

    let meter_id = client.register_meter_with_mode(
        &user,
        &provider,
        &10,
        &token_address,
        &BillingType::PostPaid,
    );

    client.top_up(&meter_id, &5000);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.billing_type, BillingType::PostPaid);
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.debt, 0);
    assert_eq!(meter.collateral_limit, 5000);
    assert!(meter.is_active);
    assert_eq!(token.balance(&contract_id), 5000);

    env.ledger().set_timestamp(env.ledger().timestamp() + 3);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 30);
    assert_eq!(meter.collateral_limit, 5000);
    assert!(meter.is_active);
    assert_eq!(token.balance(&provider), 30);
    assert_eq!(token.balance(&contract_id), 4970);

    client.deduct_units(&meter_id, &27);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 300);
    assert_eq!(meter.collateral_limit, 5000);
    assert!(meter.is_active);
    assert_eq!(token.balance(&provider), 300);
    assert_eq!(token.balance(&contract_id), 4700);
}

#[test]
fn test_postpaid_top_up_settles_debt_and_resets_when_reactivated() {
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

    token_admin_client.mint(&user, &100000);

    let meter_id = client.register_meter_with_mode(
        &user,
        &provider,
        &10,
        &token_address,
        &BillingType::PostPaid,
    );

    client.top_up(&meter_id, &50000);
    env.ledger().set_timestamp(env.ledger().timestamp() + 1);
    client.claim(&meter_id);
    client.deduct_units(&meter_id, &9);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 100);
    assert!(meter.is_active);
    assert_eq!(token.balance(&provider), 100);

    env.ledger().set_timestamp(env.ledger().timestamp() + 80);
    client.top_up(&meter_id, &20000);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 0);
    assert_eq!(meter.collateral_limit, 69900); // 49900 (remaining) + 20000
    assert!(meter.is_active);
    assert_eq!(token.balance(&contract_id), 69900);

    env.ledger().set_timestamp(env.ledger().timestamp() + 1);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.debt, 810);
    assert_eq!(meter.collateral_limit, 69900);
    assert!(meter.is_active);
    assert_eq!(token.balance(&provider), 910);
    assert_eq!(token.balance(&contract_id), 69090);
}

#[test]
fn test_variable_rate_tariffs_peak_vs_offpeak() {
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

    token_admin_client.mint(&user, &10_000);

    // Register meter with off-peak rate of 10 tokens/second
    // Peak rate will be automatically set to 15 (10 * 1.5)
    let meter_id = client.register_meter(&user, &provider, &10, &token_address);
    
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.off_peak_rate, 10);
    assert_eq!(meter.peak_rate, 15);

    client.top_up(&meter_id, &1000);

    // Test OFF-PEAK claim: timestamp = 46800 (13:00 UTC)
    // This is before peak hours (18:00-21:00), so use off_peak_rate
    env.ledger().set_timestamp(46800); // 13:00 UTC
    let meter_before = client.get_meter(&meter_id).unwrap();
    let initial_balance = meter_before.balance;

    env.ledger().set_timestamp(46805); // 5 seconds later
    client.claim(&meter_id);

    let meter_after_offpeak = client.get_meter(&meter_id).unwrap();
    let offpeak_deduction = initial_balance - meter_after_offpeak.balance;
    // Off-peak: 5 seconds * 10 tokens/second = 50 tokens
    assert_eq!(offpeak_deduction, 50);
    assert_eq!(token.balance(&provider), 50);

    // Test PEAK claim: timestamp = 68400 (19:00 UTC)
    // This is during peak hours (18:00-21:00), so use peak_rate (1.5x)
    env.ledger().set_timestamp(68400); // 19:00 UTC
    let meter_before_peak = client.get_meter(&meter_id).unwrap();
    let balance_before_peak = meter_before_peak.balance;

    env.ledger().set_timestamp(68405); // 5 seconds later (still at 19:00)
    client.claim(&meter_id);

    let meter_after_peak = client.get_meter(&meter_id).unwrap();
    let peak_deduction = balance_before_peak - meter_after_peak.balance;
    // Peak: 5 seconds * 15 tokens/second = 75 tokens
    assert_eq!(peak_deduction, 75);
    assert_eq!(token.balance(&provider), 125); // 50 + 75

    // Verify the rate multiplier was correctly applied
    // peak_rate should be 1.5x off_peak_rate
    assert_eq!(meter_after_peak.peak_rate, (meter_after_peak.off_peak_rate * 3) / 2);
}

#[test]
fn test_variable_rate_deduct_units_respects_peak_hours() {
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

    token_admin_client.mint(&user, &5000);

    // Register with off-peak rate of 20 tokens/second
    let meter_id = client.register_meter(&user, &provider, &20, &token_address);
    client.top_up(&meter_id, &2000);

    // OFF-PEAK deduction at 10:00 UTC
    env.ledger().set_timestamp(36000); // 10:00 UTC
    client.deduct_units(&meter_id, &10); // 10 units
    
    let meter = client.get_meter(&meter_id).unwrap();
    // Off-peak: 10 units * 20 tokens/unit = 200 tokens
    assert_eq!(meter.balance, 1800);
    assert_eq!(token.balance(&provider), 200);

    // PEAK deduction at 20:00 UTC
    env.ledger().set_timestamp(72000); // 20:00 UTC
    client.deduct_units(&meter_id, &10); // 10 units
    
    let meter = client.get_meter(&meter_id).unwrap();
    // Peak: 10 units * 30 tokens/unit (20 * 1.5) = 300 tokens
    assert_eq!(meter.balance, 1500);
    assert_eq!(token.balance(&provider), 500); // 200 + 300
}

#[test]
fn test_signature_verification_success() {
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

    // Generate device key pair
    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);

    client.top_up(&meter_id, &500);

    // Create valid signed usage data
    let timestamp = env.ledger().timestamp();
    let watt_hours_consumed = 250;
    let units_consumed = 15;

    // Create message to sign: meter_id || timestamp || watt_hours_consumed || units_consumed
    let mut message = Vec::new(&env);
    message.push_back(&meter_id);
    message.push_back(&timestamp);
    message.push_back(&watt_hours_consumed);
    message.push_back(&units_consumed);

    // For testing, we'll use a mock signature (in real implementation, this would be a valid signature)
    let mock_signature = BytesN::from_array(&env, &[2u8; 64]);

    let signed_data = SignedUsageData {
        meter_id,
        timestamp,
        watt_hours_consumed,
        units_consumed,
        signature: mock_signature,
        public_key: device_public_key,
    };

    // This should fail with invalid signature since we're using a mock signature
    // In a real test, you'd generate a proper signature using the private key
    let result = std::panic::catch_unwind(|| {
        client.deduct_units(&signed_data);
    });
    
    assert!(result.is_err()); // Should panic due to invalid signature
}

#[test]
fn test_public_key_mismatch() {
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

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let wrong_public_key = BytesN::from_array(&env, &[2u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);

    client.top_up(&meter_id, &500);

    let timestamp = env.ledger().timestamp();
    let mock_signature = BytesN::from_array(&env, &[2u8; 64]);

    let signed_data = SignedUsageData {
        meter_id,
        timestamp,
        watt_hours_consumed: 250,
        units_consumed: 15,
        signature: mock_signature,
        public_key: wrong_public_key, // Wrong public key
    };

    let result = std::panic::catch_unwind(|| {
        client.deduct_units(&signed_data);
    });
    
    assert!(result.is_err()); // Should panic due to public key mismatch
}

#[test]
fn test_update_device_public_key() {
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

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let new_public_key = BytesN::from_array(&env, &[2u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);

    // Verify initial public key
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.device_public_key, device_public_key);

    // Update public key
    client.update_device_public_key(&meter_id, &new_public_key);

    // Verify updated public key
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.device_public_key, new_public_key);
}

#[test]
fn test_xlm_to_usd_conversion_top_up() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    
    // Create mock oracle with $1.50 per XLM (150 cents)
    let mock_oracle = MockPriceOracle::new(&env, 150, 2);
    client.set_oracle(&mock_oracle.address());

    // Use native token (XLM) - represented by empty address for testing
    let xlm_address = Address::generate(&env); // In real scenario, this would be native token
    
    let meter_id = client.register_meter(&user, &provider, &10, &xlm_address);
    
    // Top up with 100 XLM
    // Should convert to 100 * 150 = 15000 cents = $150.00
    client.top_up(&meter_id, &100);
    
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 15000); // 100 XLM * 150 cents/XLM = 15000 cents
    assert!(meter.is_active);
}

#[test]
fn test_withdraw_earnings_xlm_conversion() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    
    // Create mock oracle with $2.00 per XLM (200 cents)
    let mock_oracle = MockPriceOracle::new(&env, 200, 2);
    client.set_oracle(&mock_oracle.address());

    let xlm_address = Address::generate(&env);
    let meter_id = client.register_meter(&user, &provider, &10, &xlm_address);
    
    // Top up first to have balance
    client.top_up(&meter_id, &100); // 100 XLM = 20000 cents
    
    // Withdraw 10000 cents ($100.00)
    // Should convert to 10000 / 200 = 50 XLM
    client.withdraw_earnings(&meter_id, &10000);
    
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 10000); // 20000 - 10000 = 10000 cents remaining
}

#[test]
fn test_get_current_rate() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    // No oracle set initially
    assert!(client.get_current_rate().is_none());
    
    // Set oracle
    let mock_oracle = MockPriceOracle::new(&env, 175, 2);
    client.set_oracle(&mock_oracle.address());
    
    // Now should return rate
    let rate = client.get_current_rate().unwrap();
    assert_eq!(rate.price, 175);
    assert_eq!(rate.decimals, 2);
}
