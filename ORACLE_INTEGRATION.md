# XLM to USD Oracle Integration

This implementation adds oracle functionality to the Utility Drip Contracts, allowing users to pay in XLM while utilities are priced in USD.

## Overview

The solution consists of two main components:

1. **Price Oracle Contract** - Manages XLM to USD exchange rates
2. **Enhanced Utility Contract** - Integrates with the oracle for price conversion

## Features

### Price Oracle Contract (`contracts/price_oracle/`)

- **Price Management**: Stores current XLM to USD exchange rate
- **Staleness Protection**: Rejects prices older than 5 minutes
- **Access Control**: Admin and updater roles for secure price updates
- **Conversion Functions**: Direct XLM↔USD conversion methods

#### Key Functions:
- `initialize()` - Set up oracle with admin, updater, and initial price
- `update_price()` - Update exchange rate (authorized only)
- `xlm_to_usd_cents()` - Convert XLM amount to USD cents
- `usd_cents_to_xlm()` - Convert USD cents to XLM amount
- `get_fresh_price()` - Get price with staleness check

### Enhanced Utility Contract (`contracts/utility_contracts/`)

- **Automatic Conversion**: Detects XLM payments and converts to USD
- **Provider Withdrawals**: Allows providers to withdraw earnings in XLM
- **Conversion Events**: Emits events for transparency
- **Backward Compatibility**: Works with existing custom tokens

#### New Functions:
- `top_up()` - Enhanced to handle XLM→USD conversion
- `withdraw_earnings()` - New function for USD→XLM conversion
- `get_current_rate()` - Get current exchange rate

## Usage Flow

### User Pays with XLM
1. User calls `top_up()` with XLM amount
2. Contract detects native token (XLM)
3. Calls oracle to convert XLM→USD cents
4. Credits meter account in USD cents
5. Emits conversion event

### Provider Withdraws in XLM
1. Provider calls `withdraw_earnings()` with USD amount
2. Contract calls oracle to convert USD→XLM
3. Transfers XLM to provider
4. Updates meter balance/debt
5. Emits conversion event

## Price Data Structure

```rust
pub struct PriceData {
    pub price: i128,        // Price in smallest units (cents for USD)
    pub decimals: u32,      // Number of decimal places
    pub last_updated: u64,  // Timestamp of last update
}
```

## Error Handling

New error types added:
- `PriceConversionFailed` - Oracle conversion failed
- `InvalidTokenAmount` - Zero or negative amounts
- `StalePrice` - Oracle price too old (oracle contract)

## Security Features

1. **Oracle Authorization**: Only authorized updater can modify prices
2. **Staleness Checks**: Rejects old price data
3. **Access Control**: Admin controls updater role
4. **Event Logging**: All conversions emit events for transparency

## Testing

Comprehensive tests included for:
- Oracle initialization and price updates
- XLM to USD conversion in top-ups
- USD to XLM conversion in withdrawals
- Rate retrieval functionality
- Error conditions

## Deployment

1. Deploy Price Oracle contract
2. Initialize with admin, updater, and initial price
3. Deploy Utility Contract (or upgrade existing)
4. Set oracle address in Utility Contract
5. Set up price updater service for real-time rates

## Integration Notes

- Native token detection uses address pattern matching
- Custom tokens (starting with "CA") bypass conversion
- All internal accounting remains in USD cents
- Oracle calls include freshness validation

## Example Price Update

```rust
// Update price to $1.75 per XLM (175 cents)
oracle.update_price(175);

// User tops up 100 XLM
utility.top_up(meter_id, 100);

// Internally: 100 XLM * 175 cents = 17500 cents ($175.00)
```

This implementation provides a robust, secure solution for XLM payments while maintaining USD-based utility pricing.
