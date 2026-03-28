## Summary

This PR implements four critical safety and monitoring features for the Utility-Drip smart contract:

### Features Added

**#16 - Max Flow Rate Safety Cap**
- Added `max_flow_rate_per_hour` field to prevent hardware errors from draining wallets
- Users can set custom hourly limits via `set_max_flow_rate()` function
- Claim function now respects hourly caps to protect against malfunctioning hardware

**#12 - Expected Depletion Calculation**
- Added `calculate_expected_depletion()` read-only function
- Predicts when user's balance will reach zero based on current consumption rate
- Improves UX by helping users plan their top-ups

**#19 - Emergency Shutdown**
- Added `emergency_shutdown()` function for providers
- Allows immediate disabling of specific meters for leak/hardware tampering scenarios
- Enhances security and provider control

**#20 - Heartbeat Monitoring**
- Added `heartbeat` timestamp tracking to Meter struct
- Implemented `update_heartbeat()` and `is_meter_offline()` functions
- Meters considered offline if heartbeat > 1 hour old
- Enables IoT connectivity monitoring

### Changes Made
- Extended Meter struct with new fields for safety and monitoring
- Added 5 new contract functions
- Updated claim logic to respect flow rate caps
- Added comprehensive test coverage for all new functionality
- All tests passing successfully

### Testing
- Added 5 new test cases covering all functionality
- Tests verify max flow rate enforcement, depletion calculation, emergency shutdown, and heartbeat monitoring
- 100% test pass rate

Fixes: #16, #12, #19, #20
