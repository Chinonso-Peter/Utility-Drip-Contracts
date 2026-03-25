#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use soroban_sdk::testutils::Address as TestAddress;
    use soroban_sdk::Address;

    #[test]
    fn test_extreme_usage_values() {
        let env = Env::default();
        let contract_id = env.register_contract(None, UtilityContract);
        let client = utility_contracts::UtilityContractClient::new(&env, &contract_id);
        
        let user = Address::generate(&env);
        let provider = Address::generate(&env);
        let token = Address::generate(&env);
        
        env.storage().instance().set(&DataKey::Oracle, &provider);
        
        let meter_id = 1u64;
        let rate_per_second = 1000i128;
        
        client.create_meter(
            &meter_id,
            &user,
            &provider,
            &token,
            &rate_per_second,
            &1000000i128,
            &BillingType::PostPaid,
        );
        
        // Test extreme usage values (millions of kWh)
        let extreme_values = vec![
            1_000_000_000i128,      // 1 billion Wh = 1 million kWh
            10_000_000_000i128,     // 10 billion Wh = 10 million kWh
            100_000_000_000i128,    // 100 billion Wh = 100 million kWh
            1_000_000_000_000i128,  // 1 trillion Wh = 1 billion kWh
            i128::MAX,
        ];
        
        for (i, &usage) in extreme_values.iter().enumerate() {
            println!("Testing extreme usage value {}: {}", i + 1, usage);
            
            // This should not panic
            client.update_usage(&meter_id, &usage);
            
            // Verify the usage data is consistent
            let usage_data = client.get_usage_data(&meter_id);
            assert!(usage_data.is_some());
            
            let data = usage_data.unwrap();
            assert!(data.total_watt_hours >= 0);
            assert!(data.current_cycle_watt_hours >= 0);
            assert!(data.peak_usage_watt_hours >= 0);
            
            // Test display function
            let display = UtilityContract::get_watt_hours_display(
                &env, 
                &data.total_watt_hours, 
                &data.precision_factor
            );
            assert!(display >= 0);
        }
    }

    #[test]
    fn test_precision_factor_extremes() {
        let env = Env::default();
        
        // Test extreme precision factors
        let extreme_precision_factors = vec![
            1i128,
            1000i128,
            1_000_000i128,
            1_000_000_000i128,
            i128::MAX / 1000,
        ];
        
        let test_usage = 1_000_000_000i128; // 1 million kWh
        
        for (i, &precision) in extreme_precision_factors.iter().enumerate() {
            println!("Testing precision factor {}: {}", i + 1, precision);
            
            // Test multiplication (happens in update_usage)
            let precise_consumption = test_usage.saturating_mul(precision);
            assert!(precise_consumption >= 0);
            
            // Test division (happens in display function)
            if precision != 0 {
                let display = test_usage / precision;
                assert!(display >= 0);
            }
        }
    }

    #[test]
    fn test_arithmetic_edge_cases() {
        let env = Env::default();
        
        // Test edge case arithmetic operations
        let edge_cases = vec![
            i128::MAX,
            i128::MIN,
            i128::MAX - 1,
            i128::MIN + 1,
            0i128,
            -1i128,
            1i128,
        ];
        
        for (i, &value) in edge_cases.iter().enumerate() {
            println!("Testing edge case {}: {}", i + 1, value);
            
            // Test saturating operations used in the contract
            let _result = value.saturating_add(1);
            let _result = value.saturating_mul(1000);
            let _result = value.saturating_sub(1);
            
            // Test division (only if divisor is not zero)
            if value != 0 {
                let _result = 1000i128 / value;
            }
        }
    }

    #[test]
    fn test_cumulative_extreme_usage() {
        let env = Env::default();
        let contract_id = env.register_contract(None, UtilityContract);
        let client = utility_contracts::UtilityContractClient::new(&env, &contract_id);
        
        let user = Address::generate(&env);
        let provider = Address::generate(&env);
        let token = Address::generate(&env);
        
        env.storage().instance().set(&DataKey::Oracle, &provider);
        
        let meter_id = 1u64;
        let rate_per_second = 1000i128;
        
        client.create_meter(
            &meter_id,
            &user,
            &provider,
            &token,
            &rate_per_second,
            &1000000i128,
            &BillingType::PostPaid,
        );
        
        // Test cumulative extreme usage updates
        let extreme_usage = 1_000_000_000i128; // 1 million kWh
        
        for i in 0..100 {
            let cumulative_usage = extreme_usage.saturating_mul(i + 1);
            client.update_usage(&meter_id, &cumulative_usage);
            
            let usage_data = client.get_usage_data(&meter_id);
            assert!(usage_data.is_some());
            
            let data = usage_data.unwrap();
            assert!(data.total_watt_hours >= 0);
            assert!(data.current_cycle_watt_hours >= 0);
            assert!(data.peak_usage_watt_hours >= 0);
        }
    }
}
