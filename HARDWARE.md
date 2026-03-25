# Hardware Spec Integration

This project does not connect to MQTT brokers directly from the smart contract. The contract is the settlement layer, while a backend service listens for device telemetry over MQTT, validates the readings, and then submits authenticated transactions to the contract.

## Architecture

```text
+---------+       +--------+       +---------+       +------------------+
|  Meter  | ----> |  MQTT  | ----> | Backend | ----> | Smart Contract   |
| Device  |       | Broker |       | Service |       | (Soroban/Stellar)|
+---------+       +--------+       +---------+       +------------------+
```

## Connection Flow

1. A smart meter or controller running on ESP32 or Raspberry Pi measures energy or utility consumption.
2. The device publishes usage data to an MQTT topic such as `meters/{meter_id}/usage`.
3. The backend service subscribes to the MQTT broker, parses the payload, and validates the device identity and reading format.
4. The backend maps the hardware meter to an on-chain `meter_id`.
5. The backend submits a signed contract call such as `deduct_units` or `update_usage`.
6. The contract updates stored meter data, transfers value when required, and emits events for downstream monitoring.

## MQTT Payload Example

```json
{
  "meter_id": 1,
  "timestamp": 1710000000,
  "watt_hours_consumed": 250,
  "units_consumed": 1,
  "signature": "base64_encoded_64_byte_signature",
  "public_key": "base64_encoded_32_byte_public_key"
}
```

## Security Requirements

### ESP32 Device Authentication

All usage data sent from ESP32 devices must be cryptographically signed to ensure authenticity and prevent tampering. The implementation follows these security principles:

1. **Key Pair Generation**: Each ESP32 device generates an Ed25519 key pair (32-byte private key, 32-byte public key)
2. **Public Key Registration**: The device's public key is registered on-chain during meter registration
3. **Message Signing**: Usage data is signed using the device's private key
4. **Signature Verification**: The smart contract verifies signatures before processing usage data

### Signed Message Format

The message that gets signed includes:
- meter_id (u64)
- timestamp (u64) 
- watt_hours_consumed (i128)
- units_consumed (i128)

These values are concatenated and signed using Ed25519 to produce a 64-byte signature.

### ESP32 Implementation Requirements

ESP32 devices must:
1. Generate and securely store an Ed25519 private key
2. Include the public key during meter registration
3. Sign all usage data messages with the private key
4. Include both the signature and public key in the MQTT payload
5. Use current timestamps to prevent replay attacks (max 5 minutes delay)

### Backend Service Changes

The backend service must:
1. Forward signed usage data to the smart contract
2. No longer validate device identity (handled by contract)
3. Pass through the complete SignedUsageData structure to the contract

## Supported Hardware Devices

- ESP32 development boards with Wi-Fi and MQTT client support
- ESP32-S3 boards for higher-performance edge processing
- Raspberry Pi Zero 2 W for lightweight gateway deployments
- Raspberry Pi 4 for local aggregation and MQTT forwarding
- Raspberry Pi 5 for higher-throughput backend or gateway workloads

## Suggested Responsibilities

- **Meter device**: Generate key pairs, sign usage data, and publish signed telemetry
- **MQTT broker**: Route signed messages reliably between devices and backend services
- **Backend service**: Forward signed usage data and call contract with SignedUsageData
- **Smart contract**: Verify signatures, store meter state, handle balances, and enforce settlement rules

## Notes

- The smart contract now handles device authentication through signature verification
- ESP32 devices must implement Ed25519 signature generation (can use libraries like TinyCrypt)
- Private keys must be securely stored on the ESP32 (consider secure storage elements)
- The contract rejects usage data with invalid signatures, public key mismatches, or old timestamps
- Backend services should maintain compatibility by forwarding the complete signed payload
- Meter users can update device public keys using the `update_device_public_key` function
