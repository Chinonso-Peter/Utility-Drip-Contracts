# Variable Rate Tariffs Implementation

## Overview
This document describes the implementation of variable rate tariffs (peak and off-peak pricing) for the Utility Drip Contracts smart contract.

## Feature Description
Electricity costs vary based on time of day. The contract now supports:
- **Off-peak rate**: Standard hourly rate (e.g., 10 tokens per second)
- **Peak rate**: 1.5x the off-peak rate during peak hours

## Implementation Details

### Constants
```rust
// Peak hours: 18:00 - 21:00 UTC
const PEAK_HOUR_START: u64 = 18 * HOUR_IN_SECONDS;  // 64,800 seconds
const PEAK_HOUR_END: u64 = 21 * HOUR_IN_SECONDS;     // 75,600 seconds
const PEAK_RATE_MULTIPLIER: i128 = 3;                // 1.5x (divided by 2)
const RATE_PRECISION: i128 = 2;                      // Precision divisor
```

### Meter Struct Update
The `Meter` struct now stores both rates instead of a single rate:

```rust
pub struct Meter {
    // ... other fields ...
    pub off_peak_rate: i128,  // rate per second during off-peak hours
    pub peak_rate: i128,      // rate per second during peak hours (1.5x off-peak)
    // ... other fields ...
}
```

### Helper Functions

#### `is_peak_hour(timestamp: u64) -> bool`
Determines if the given timestamp falls within peak hours (18:00-21:00 UTC).
- Extracts the seconds in the current day using modulo
- Checks if it's between PEAK_HOUR_START and PEAK_HOUR_END

```rust
fn is_peak_hour(timestamp: u64) -> bool {
    let seconds_in_day = timestamp % DAY_IN_SECONDS;
    seconds_in_day >= PEAK_HOUR_START && seconds_in_day < PEAK_HOUR_END
}
```

#### `get_effective_rate(meter: &Meter, timestamp: u64) -> i128`
Returns the effective rate based on the current time:
- Returns `peak_rate` if the timestamp is during peak hours
- Returns `off_peak_rate` otherwise

```rust
fn get_effective_rate(meter: &Meter, timestamp: u64) -> i128 {
    if is_peak_hour(timestamp) {
        meter.peak_rate
    } else {
        meter.off_peak_rate
    }
}
```

### Modified Functions

#### `register_meter()` and `register_meter_with_mode()`
Now calculate and store both rates:
- Accepts `off_peak_rate` as parameter
- Automatically calculates `peak_rate = off_peak_rate * 1.5`
- Stores both in the Meter struct

#### `claim()`
Now uses the effective rate based on the current block timestamp:
```rust
let effective_rate = get_effective_rate(&meter, now);
let requested = (elapsed as i128).saturating_mul(effective_rate);
```

#### `deduct_units()`
Similarly updated to use the effective rate based on the claim timestamp:
```rust
let effective_rate = get_effective_rate(&meter, now);
let requested = units_consumed.saturating_mul(effective_rate);
```

#### `calculate_expected_depletion()`
Uses the `off_peak_rate` as a conservative estimate for depletion time (since peak rate is higher, the meter will deplete faster during peak hours).

## Acceptance Criteria

✅ **Define peak hours (e.g., 18:00 - 21:00 UTC)**
- Peak hours are defined as 18:00-21:00 UTC (constants PEAK_HOUR_START and PEAK_HOUR_END)
- Validation is done through the `is_peak_hour()` function

✅ **Logic: if (now is peak) cost = rate * 1.5 else cost = rate**
- The `get_effective_rate()` function implements this logic
- Peak rate is stored as 1.5x the off-peak rate
- Applied in both `claim()` and `deduct_units()` functions

✅ **Update Meter struct to store multiple rates**
- Meter struct now has `off_peak_rate` and `peak_rate` fields
- Both rates are calculated at registration time
- Used dynamically based on the timestamp of each operation

## Usage Example

```rust
// Register a meter with an off-peak rate of 10 tokens per second
// Peak rate will automatically be set to 15 (10 * 1.5)
let meter_id = client.register_meter(&user, &provider, &10, &token_address);

// At 15:00 UTC (off-peak): 5 seconds elapsed = 50 tokens
// At 19:00 UTC (peak): 5 seconds elapsed = 75 tokens
client.claim(&meter_id);
```

## Time-based Behavior

### Off-peak Period (00:00 - 18:00 UTC, 21:00 - 24:00 UTC)
- Costs are calculated at the off-peak rate
- Example: 10 tokens/second × 3600 seconds = 36,000 tokens per hour

### Peak Period (18:00 - 21:00 UTC)
- Costs are calculated at 1.5x the off-peak rate
- Example: 15 tokens/second × 3600 seconds = 54,000 tokens per hour

## Testing
All existing tests have been updated to use the new field names:
- `meter.off_peak_rate` (instead of `meter.rate_per_second`)
- `meter.peak_rate` (for assertions involving peak pricing)

## Backward Compatibility
This is a **breaking change**. Existing code using `meter.rate_per_second` must be updated to use either:
- `meter.off_peak_rate` for standard rate operations
- `get_effective_rate(&meter, timestamp)` for timestamp-aware rate retrieval

## Future Enhancements
1. Make peak hours configurable by provider
2. Support multiple peak periods per day
3. Seasonal rate adjustments
4. Integration with external pricing oracles
