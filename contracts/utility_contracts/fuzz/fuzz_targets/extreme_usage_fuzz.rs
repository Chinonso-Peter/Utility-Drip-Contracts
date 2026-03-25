#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as TestAddress, Address, Env};
use utility_contracts::{UtilityContract, UsageData, Meter, BillingType};

fuzz_target!(|data: &[u8]| {
    // Convert bytes to i128 for extreme usage values
    if data.len() < 16 {
        return;
    }
    
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&data[..16]);
    let extreme_usage = i128::from_be_bytes(bytes);
    
    // Only test with very large values (millions of kWh equivalent)
    // 1 kWh = 1000 Wh, so millions of kWh = billions of Wh
    if extreme_usage.abs() < 1_000_000_000 {
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
    
    // Create a meter with extreme usage scenario
    let meter_id = 1u64;
    let rate_per_second = 1000i128; // 1000 units per second
    
    // Initialize the meter
    client.create_meter(
        &meter_id,
        &user,
        &provider,
        &token,
        &rate_per_second,
        &1000000i128, // 1M collateral limit
        &utility_contracts::BillingType::PostPaid,
    );
    
    // Test extreme usage update - this should not crash
    let result = std::panic::catch_unwind(|| {
        client.update_usage(&meter_id, &extreme_usage);
    });
    
    if result.is_err() {
        panic!("Contract crashed with extreme usage value: {}", extreme_usage);
    }
    
    // Test multiple extreme updates
    for i in 0..10 {
        let usage_multiplier = extreme_usage.saturating_mul(i as i128 + 1);
        let result = std::panic::catch_unwind(|| {
            client.update_usage(&meter_id, &usage_multiplier);
        });
        
        if result.is_err() {
            panic!("Contract crashed with extreme usage value: {} (iteration {})", usage_multiplier, i);
        }
    }
    
    // Test usage data retrieval after extreme updates
    let usage_data = client.get_usage_data(&meter_id);
    if let Some(data) = usage_data {
        // Verify the data is consistent and not corrupted
        if data.total_watt_hours < 0 || data.current_cycle_watt_hours < 0 || data.peak_usage_watt_hours < 0 {
            panic!("Negative usage values detected after extreme input");
        }
        
        // Test display function with extreme values
        let display_result = std::panic::catch_unwind(|| {
            utility_contracts::UtilityContractClient::get_watt_hours_display(
                &env, 
                &data.total_watt_hours, 
                &data.precision_factor
            );
        });
        
        if display_result.is_err() {
            panic!("Display function crashed with extreme usage values");
        }
    }
});
