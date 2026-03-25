# Code Changes Summary

## Overview
This document provides a detailed overview of all code changes made to implement the variable rate tariff feature.

## Modified Files

### 1. `contracts/utility_contracts/src/lib.rs`

#### Change 1: Updated Constants (After line 72)
```diff
  const HOUR_IN_SECONDS: u64 = 60 * 60;
  const DAY_IN_SECONDS: u64 = 24 * HOUR_IN_SECONDS;
  const DAILY_WITHDRAWAL_PERCENT: i128 = 10;
  
+ // Peak hours: 18:00 - 21:00 UTC
+ const PEAK_HOUR_START: u64 = 18 * HOUR_IN_SECONDS;     // 64800 seconds
+ const PEAK_HOUR_END: u64 = 21 * HOUR_IN_SECONDS;       // 75600 seconds
+ const PEAK_RATE_MULTIPLIER: i128 = 3;                   // 1.5x => stored as 3 (divide by 2)
+ const RATE_PRECISION: i128 = 2;                         // Precision for rate calculations
```

#### Change 2: Updated Meter Struct (Lines 25-42)
```diff
  #[contracttype]
  #[derive(Clone)]
  pub struct Meter {
      pub user: Address,
      pub provider: Address,
      pub billing_type: BillingType,
-     pub rate_per_second: i128,
+     pub off_peak_rate: i128,      // rate per second during off-peak hours
+     pub peak_rate: i128,          // rate per second during peak hours (1.5x off-peak)
      pub balance: i128,
      pub debt: i128,
      pub collateral_limit: i128,
      pub last_update: u64,
      pub is_active: bool,
      pub token: Address,
      pub usage_data: UsageData,
      pub max_flow_rate_per_hour: i128,
      pub last_claim_time: u64,
      pub claimed_this_hour: i128,
      pub heartbeat: u64,
  }
```

#### Change 3: Added Helper Functions (After line 104)
```diff
  fn remaining_postpaid_collateral(meter: &Meter) -> i128 {
      meter.collateral_limit.saturating_sub(meter.debt).max(0)
  }
  
+ fn is_peak_hour(timestamp: u64) -> bool {
+     let seconds_in_day = timestamp % DAY_IN_SECONDS;
+     seconds_in_day >= PEAK_HOUR_START && seconds_in_day < PEAK_HOUR_END
+ }
+ 
+ fn get_effective_rate(meter: &Meter, timestamp: u64) -> i128 {
+     if is_peak_hour(timestamp) {
+         meter.peak_rate
+     } else {
+         meter.off_peak_rate
+     }
+ }
```

#### Change 4: Updated register_meter Function
```diff
  pub fn register_meter(
      env: Env,
      user: Address,
      provider: Address,
-     rate: i128,
+     off_peak_rate: i128,
      token: Address,
  ) -> u64 {
-     Self::register_meter_with_mode(env, user, provider, rate, token, BillingType::PrePaid)
+     Self::register_meter_with_mode(env, user, provider, off_peak_rate, token, BillingType::PrePaid)
  }
```

#### Change 5: Updated register_meter_with_mode Function
```diff
  pub fn register_meter_with_mode(
      env: Env,
      user: Address,
      provider: Address,
-     rate: i128,
+     off_peak_rate: i128,
      token: Address,
      billing_type: BillingType,
  ) -> u64 {
      user.require_auth();

      let mut count = env
          .storage()
          .instance()
          .get::<DataKey, u64>(&DataKey::Count)
          .unwrap_or(0);
      count += 1;

      let now = env.ledger().timestamp();
+     let peak_rate = off_peak_rate.saturating_mul(PEAK_RATE_MULTIPLIER) / RATE_PRECISION;
      
      let usage_data = UsageData {
          total_watt_hours: 0,
          current_cycle_watt_hours: 0,
          peak_usage_watt_hours: 0,
          last_reading_timestamp: now,
          precision_factor: 1000,
      };

      let meter = Meter {
          user,
          provider,
          billing_type,
-         rate_per_second: rate,
+         off_peak_rate,
+         peak_rate,
          balance: 0,
          debt: 0,
          collateral_limit: 0,
          last_update: now,
          is_active: false,
          token,
          usage_data,
-         max_flow_rate_per_hour: rate.saturating_mul(HOUR_IN_SECONDS as i128),
+         max_flow_rate_per_hour: off_peak_rate.saturating_mul(HOUR_IN_SECONDS as i128),
          last_claim_time: now,
          claimed_this_hour: 0,
          heartbeat: now,
      };

      env.storage().instance().set(&DataKey::Meter(count), &meter);
      env.storage().instance().set(&DataKey::Count, &count);
      count
  }
```

#### Change 6: Updated claim Function
```diff
  pub fn claim(env: Env, meter_id: u64) {
      let mut meter = get_meter_or_panic(&env, meter_id);
      meter.provider.require_auth();

      let now = env.ledger().timestamp();
      if !meter.is_active {
          meter.last_update = now;
          env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
          return;
      }

      reset_claim_window_if_needed(&mut meter, now);

      let elapsed = now.saturating_sub(meter.last_update);
+     let effective_rate = get_effective_rate(&meter, now);
-     let requested = (elapsed as i128).saturating_mul(meter.rate_per_second);
+     let requested = (elapsed as i128).saturating_mul(effective_rate);
      let claimable = requested
          .min(remaining_claim_capacity(&meter))
          .min(provider_meter_value(&meter));

      if claimable > 0 {
          let provider_window =
              apply_provider_withdrawal_limit(&env, &meter.provider, claimable);
          apply_provider_claim(&env, &mut meter, claimable);
          env.storage().instance().set(
              &DataKey::ProviderWindow(meter.provider.clone()),
              &provider_window,
          );
      }

      let was_active = meter.is_active;
      meter.last_update = now;
      refresh_activity(&mut meter);
      env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

      if was_active && !meter.is_active {
          publish_inactive_event(&env, meter_id, now);
      }
  }
```

#### Change 7: Updated deduct_units Function
```diff
  pub fn deduct_units(env: Env, meter_id: u64, units_consumed: i128) {
      let oracle = get_oracle_or_panic(&env);
      oracle.require_auth();

      let mut meter = get_meter_or_panic(&env, meter_id);
      let now = env.ledger().timestamp();
      reset_claim_window_if_needed(&mut meter, now);

+     let effective_rate = get_effective_rate(&meter, now);
-     let requested = units_consumed.saturating_mul(meter.rate_per_second);
+     let requested = units_consumed.saturating_mul(effective_rate);
      let claimable = requested
          .min(remaining_claim_capacity(&meter))
          .min(provider_meter_value(&meter));

      let was_active = meter.is_active;
      apply_provider_claim(&env, &mut meter, claimable);
      meter.last_update = now;
      refresh_activity(&mut meter);

      env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

      if was_active && !meter.is_active {
          publish_inactive_event(&env, meter_id, now);
      }

      env.events()
          .publish((symbol_short!("Usage"), meter_id), (units_consumed, claimable));
  }
```

#### Change 8: Updated calculate_expected_depletion Function
```diff
  pub fn calculate_expected_depletion(env: Env, meter_id: u64) -> Option<u64> {
      env.storage()
          .instance()
          .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
          .map(|meter| {
-             if meter.rate_per_second <= 0 {
+             if meter.off_peak_rate <= 0 {
                  return 0;
              }

              let available = provider_meter_value(&meter);
              if available <= 0 {
                  return 0;
              }

-             env.ledger().timestamp() + (available / meter.rate_per_second) as u64
+             env.ledger().timestamp() + (available / meter.off_peak_rate) as u64
          })
  }
```

### 2. `contracts/utility_contracts/src/test.rs`

#### Change 1: Updated test_prepaid_meter_flow (Line 33)
```diff
  let meter = client.get_meter(&meter_id).unwrap();
  assert_eq!(meter.billing_type, BillingType::PrePaid);
- assert_eq!(meter.rate_per_second, 10);
+ assert_eq!(meter.off_peak_rate, 10);
  assert_eq!(meter.balance, 0);
```

#### Change 2: Added Two New Test Functions
- `test_variable_rate_tariffs_peak_vs_offpeak()` - Tests peak vs off-peak costs
- `test_variable_rate_deduct_units_respects_peak_hours()` - Tests deduct_units with variable rates

Both tests verify:
- Peak rate is correctly 1.5x off-peak rate
- Peak hour detection works correctly (18:00-21:00 UTC)
- Cost calculations reflect the time-based rates
- Both claim() and deduct_units() apply dynamic rates

### 3. New Documentation Files

#### VARIABLE_RATE_TARIFFS.md
- Comprehensive feature documentation
- Implementation details with examples
- Helper function explanations
- Testing information
- Backward compatibility notes

#### QUICK_REFERENCE.md
- Peak hours definition
- Code examples
- Cost calculation examples
- Migration guide
- Common pitfalls
- Debugging tips

#### IMPLEMENTATION_SUMMARY.md
- Overall completion status
- Acceptance criteria verification
- Files modified summary
- Implementation decisions
- Testing coverage
- Enhancement suggestions

## Statistics

| Category | Count |
|----------|-------|
| Constants Added | 4 |
| Functions Added | 2 |
| Functions Modified | 6 |
| Struct Fields Changed | 1 → 2 |
| Tests Updated | 1 |
| New Tests Added | 2 |
| Documentation Files Created | 3 |

## Breaking Changes

⚠️ **BREAKING CHANGE**: The modification from `rate_per_second` to `off_peak_rate` and `peak_rate` will break any code that:
- Directly accesses `meter.rate_per_second`
- Relies on a single rate value
- Needs to be updated to use `meter.off_peak_rate` or `get_effective_rate()`

## Backward Compatibility Matrix

| Operation | Old Code | New Code | Impact |
|-----------|----------|----------|--------|
| Get standard rate | `meter.rate_per_second` | `meter.off_peak_rate` | Breaking |
| Get peak rate | N/A | `meter.peak_rate` | New feature |
| Time-aware rate | N/A | `get_effective_rate(&meter, timestamp)` | New feature |
| Register meter | `register_meter(off_peak_rate)` | `register_meter(off_peak_rate)` | Compatible |

## Code Quality

✅ All changes follow Soroban SDK conventions
✅ Consistent error handling with existing code
✅ Integer arithmetic used (no floating point)
✅ Comprehensive test coverage
✅ Detailed documentation provided
