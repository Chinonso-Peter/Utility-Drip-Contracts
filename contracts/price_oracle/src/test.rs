use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Env, Address};

use crate::{PriceOracle, PriceOracleClient};

#[test]
fn test_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PriceOracle, ());
    let client = PriceOracleClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let updater = Address::generate(&env);
    let initial_price = 150; // $1.50 per XLM in cents
    let decimals = 2;

    client.initialize(&admin, &updater, &initial_price, &decimals);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_updater(), updater);
    
    let price_data = client.get_price();
    assert_eq!(price_data.price, initial_price);
    assert_eq!(price_data.decimals, decimals);
}

#[test]
fn test_price_update() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PriceOracle, ());
    let client = PriceOracleClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let updater = Address::generate(&env);
    client.initialize(&admin, &updater, &100, &2);

    let new_price = 200; // $2.00 per XLM
    client.update_price(&new_price);

    assert_eq!(client.get_price_value(), new_price);
}

#[test]
fn test_xlm_to_usd_conversion() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PriceOracle, ());
    let client = PriceOracleClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let updater = Address::generate(&env);
    client.initialize(&admin, &updater, &150, &2); // $1.50 per XLM

    let xlm_amount = 100; // 100 XLM
    let usd_cents = client.xlm_to_usd_cents(&xlm_amount);
    assert_eq!(usd_cents, 15000); // 100 * 150 cents = 15000 cents = $150.00

    let usd_amount = 30000; // $300.00 in cents
    let xlm_needed = client.usd_cents_to_xlm(&usd_amount);
    assert_eq!(xlm_needed, 200); // 30000 / 150 = 200 XLM
}

#[test]
fn test_fresh_price_check() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PriceOracle, ());
    let client = PriceOracleClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let updater = Address::generate(&env);
    client.initialize(&admin, &updater, &100, &2);

    // Should be fresh initially
    assert!(client.is_price_fresh());

    // Advance time beyond staleness threshold
    env.ledger().set_timestamp(env.ledger().timestamp() + 301);
    assert!(!client.is_price_fresh());
}
