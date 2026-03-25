# Quick Reference - Variable Rate Tariffs

## Peak Hours
- **Peak**: 18:00 - 21:00 UTC (6 PM - 9 PM)
- **Off-peak**: All other hours
- **Multiplier**: Peak rate = Off-peak rate × 1.5

## Constants
```rust
PEAK_HOUR_START: u64 = 64800 seconds   // 18:00 UTC
PEAK_HOUR_END: u64 = 75600 seconds     // 21:00 UTC
```

## Meter Registration
```rust
// Register with 10 tokens per second off-peak rate
// Peak rate will automatically be 15 (10 × 1.5)
let meter_id = client.register_meter(&user, &provider, &10, &token_address);

// Verify rates
let meter = client.get_meter(&meter_id).unwrap();
assert_eq!(meter.off_peak_rate, 10);
assert_eq!(meter.peak_rate, 15);
```

## Important Functions

### `is_peak_hour(timestamp: u64) -> bool`
Checks if a timestamp is during peak hours.

### `get_effective_rate(meter: &Meter, timestamp: u64) -> i128`
Returns the appropriate rate (peak or off-peak) for a given timestamp.

## Cost Examples (Off-peak rate = 10 tokens/sec)

### Off-peak Cost Calculation
- 1 second: 10 tokens
- 1 minute: 600 tokens
- 1 hour: 36,000 tokens

### Peak Cost Calculation
- 1 second: 15 tokens
- 1 minute: 900 tokens
- 1 hour: 54,000 tokens

## Testing UTC Times

```rust
// Off-peak example (10:00 UTC)
env.ledger().set_timestamp(36000);
let effective_rate = get_effective_rate(&meter, 36000);  // Returns off_peak_rate

// Peak example (19:00 UTC)
env.ledger().set_timestamp(68400);
let effective_rate = get_effective_rate(&meter, 68400);  // Returns peak_rate

// Off-peak example (08:00 UTC)
env.ledger().set_timestamp(28800);
let effective_rate = get_effective_rate(&meter, 28800);  // Returns off_peak_rate
```

## UTC Timestamp Conversion

To convert UTC time to test timestamp:
```
Timestamp = Hours * 3600 + Minutes * 60 + Seconds

Examples:
00:00 UTC = 0
06:00 UTC = 21,600
12:00 UTC = 43,200
18:00 UTC = 64,800 (peak starts)
21:00 UTC = 75,600 (peak ends)
23:59 UTC = 86,399
```

## Migration Guide

### Old Code (Before Update)
```rust
let meter = client.get_meter(&meter_id).unwrap();
let annual_cost = meter.rate_per_second * 365 * 24 * 3600;
```

### New Code (After Update)
```rust
let meter = client.get_meter(&meter_id).unwrap();
// Use off-peak rate as baseline
let annual_cost = meter.off_peak_rate * 365 * 24 * 3600;

// Or use dynamic rate if you know the timestamp
let current_rate = get_effective_rate(&meter, env.ledger().timestamp());
```

## Rate Calculation Logic
```rust
fn get_effective_rate(meter: &Meter, timestamp: u64) -> i128 {
    if is_peak_hour(timestamp) {
        meter.peak_rate      // 1.5x off-peak rate
    } else {
        meter.off_peak_rate  // Standard rate
    }
}

fn is_peak_hour(timestamp: u64) -> bool {
    let seconds_in_day = timestamp % 86400;  // 86400 = 24 hours in seconds
    seconds_in_day >= 64800 && seconds_in_day < 75600
}
```

## API Changes Summary

| Function | Change |
|----------|--------|
| `register_meter()` | Now accepts `off_peak_rate` parameter |
| `Meter` struct | Added `peak_rate`, renamed `rate_per_second` to `off_peak_rate` |
| `claim()` | Now uses `get_effective_rate()` |
| `deduct_units()` | Now uses `get_effective_rate()` |
| `calculate_expected_depletion()` | Uses `off_peak_rate` for conservative estimate |

## Common Pitfalls

❌ **Don't**: Assume constant rates
```rust
let cost = elapsed * meter.off_peak_rate;  // May not be accurate if peak hours involved
```

✅ **Do**: Use the effective rate
```rust
let cost = elapsed * get_effective_rate(&meter, now);
```

❌ **Don't**: Forget about peak hours in calculations
```rust
// This assumes all hours have same rate
let annual_cost = meter.off_peak_rate * 365 * 24 * 3600;
```

✅ **Do**: Account for higher peak hour costs
```rust
// More accurate: ~20% higher due to peak hours
let peak_hours_per_year = 365 * 3;  // 18-21 UTC = 3 hours/day
let off_peak_hours = (365 * 24) - peak_hours_per_year;
let annual_cost = (meter.off_peak_rate * off_peak_hours) 
                + (meter.peak_rate * peak_hours_per_year);
```

## Debugging Tips

**Check if timestamp is peak or off-peak:**
```rust
env.ledger().set_timestamp(some_time);
let is_peak = is_peak_hour(some_time);
let rate = get_effective_rate(&meter, some_time);
```

**Verify rates are set correctly:**
```rust
let meter = client.get_meter(&meter_id).unwrap();
assert_eq!(meter.peak_rate, (meter.off_peak_rate * 3) / 2);
```

**Calculate expected claim amount:**
```rust
let elapsed = now - meter.last_update;
let rate = get_effective_rate(&meter, now);
let expected_claim = elapsed * rate;
```
