#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as TestAddress, Address, Env};
use utility_contracts::{UtilityContract, UsageData, Meter, BillingType};

fuzz_target!(|data: &[u8]| {
    // Need at least 32 bytes for usage and precision factor
    if data.len() < 32 {
        return;
    }
    
    let mut usage_bytes = [0u8; 16];
    let mut precision_bytes = [0u8; 16];
    usage_bytes.copy_from_slice(&data[0..16]);
    precision_bytes.copy_from_slice(&data[16..32]);
    
    let usage = i128::from_be_bytes(usage_bytes);
    let precision_factor = i128::from_be_bytes(precision_bytes);
    
    // Avoid division by zero in precision factor
    if precision_factor == 0 {
        return;
    }

    let env = Env::default();
    let contract_id = env.register_contract(None, UtilityContract);
    let client = utility_contracts::UtilityContractClient::new(&env, &contract_id);
    
    // Create test addresses
    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token = Address::generate(&env);
    
    // Mock the oracle address
    env.storage().instance().set(&utility_contracts::DataKey::Oracle, &provider);
    
    // Create a meter with custom precision factor
    let meter_id = 1u64;
    let rate_per_second = 1000i128;
    
    client.create_meter(
        &meter_id,
        &user,
        &provider,
        &token,
        &rate_per_second,
        &1000000i128,
        &utility_contracts::BillingType::PostPaid,
    );
    
    // Manually set the precision factor to test extreme values
    if let Some(mut meter) = client.get_meter(&meter_id) {
        meter.usage_data.precision_factor = precision_factor;
        env.storage().instance().set(&utility_contracts::DataKey::Meter(meter_id), &meter);
    } else {
        return;
    }
    
    // Test update_usage with extreme precision factors
    let result = std::panic::catch_unwind(|| {
        client.update_usage(&meter_id, &usage);
    });
    
    if result.is_err() {
        panic!("Contract crashed with usage: {} and precision_factor: {}", usage, precision_factor);
    }
    
    // Test display function with extreme precision factors
    let display_result = std::panic::catch_unwind(|| {
        utility_contracts::UtilityContractClient::get_watt_hours_display(&env, &usage, &precision_factor);
    });
    
    if display_result.is_err() {
        panic!("Display function crashed with usage: {} and precision_factor: {}", usage, precision_factor);
    }
    
    // Test edge case precision factors
    let edge_precision_factors = vec![
        1i128,
        -1i128,
        1000i128,
        -1000i128,
        1_000_000i128,
        i128::MAX,
        i128::MIN,
        i128::MAX / 2,
    ];
    
    for &edge_precision in &edge_precision_factors {
        if edge_precision == 0 {
            continue; // Skip division by zero
        }
        
        // Test multiplication that happens in update_usage
        let mult_result = std::panic::catch_unwind(|| {
            let _precise_consumption = usage.saturating_mul(edge_precision);
        });
        
        if mult_result.is_err() {
            panic!("Precision multiplication crashed with usage: {} and precision: {}", usage, edge_precision);
        }
        
        // Test division that happens in display function
        let div_result = std::panic::catch_unwind(|| {
            let _display = usage / edge_precision;
        });
        
        if div_result.is_err() {
            panic!("Display division crashed with usage: {} and precision: {}", usage, edge_precision);
        }
    }
});
