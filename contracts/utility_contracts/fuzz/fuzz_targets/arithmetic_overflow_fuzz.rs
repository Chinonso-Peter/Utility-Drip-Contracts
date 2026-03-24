#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as TestAddress, Address, Env};
use utility_contracts::{UtilityContract, UsageData, Meter, BillingType};

fuzz_target!(|data: &[u8]| {
    // Need at least 32 bytes for two i128 values
    if data.len() < 32 {
        return;
    }
    
    // Extract two i128 values from the data
    let mut bytes1 = [0u8; 16];
    let mut bytes2 = [0u8; 16];
    bytes1.copy_from_slice(&data[0..16]);
    bytes2.copy_from_slice(&data[16..32]);
    
    let usage1 = i128::from_be_bytes(bytes1);
    let usage2 = i128::from_be_bytes(bytes2);
    
    // Test with edge case values
    let test_values = vec![
        usage1,
        usage2,
        i128::MAX,
        i128::MIN,
        i128::MAX / 2,
        i128::MIN / 2,
    ];

    let env = Env::default();
    let contract_id = env.register_contract(None, UtilityContract);
    let client = utility_contracts::UtilityContractClient::new(&env, &contract_id);
    
    // Create test addresses
    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token = Address::generate(&env);
    
    // Mock the oracle address
    env.storage().instance().set(&utility_contracts::DataKey::Oracle, &provider);
    
    // Create a meter
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
    
    // Test arithmetic operations with extreme values
    for &test_usage in &test_values {
        // Test update_usage with extreme values
        let result = std::panic::catch_unwind(|| {
            client.update_usage(&meter_id, &test_usage);
        });
        
        if result.is_err() {
            panic!("Contract crashed with usage value: {}", test_usage);
        }
        
        // Test precision factor multiplication
        let precision_factors = vec![1i128, 1000i128, 1_000_000i128, i128::MAX / 1000];
        
        for &precision in &precision_factors {
            let display_result = std::panic::catch_unwind(|| {
                // Test the multiplication that happens in update_usage
                let _precise_consumption = test_usage.saturating_mul(precision);
            });
            
            if display_result.is_err() {
                panic!("Precision multiplication crashed with usage: {} and precision: {}", test_usage, precision);
            }
        }
        
        // Test division in display function
        let precision_factor = 1000i128;
        let division_result = std::panic::catch_unwind(|| {
            let _display = test_usage / precision_factor;
        });
        
        if division_result.is_err() {
            panic!("Division crashed with usage value: {}", test_usage);
        }
    }
    
    // Test cumulative effects
    for i in 0..5 {
        let cumulative_usage = usage1.saturating_mul(i as i128 + 1);
        let result = std::panic::catch_unwind(|| {
            client.update_usage(&meter_id, &cumulative_usage);
        });
        
        if result.is_err() {
            panic!("Cumulative usage crashed at iteration {} with value: {}", i, cumulative_usage);
        }
    }
});
