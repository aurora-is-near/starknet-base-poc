# Starknet Base POC

Rust proof of concept for moving a user STRK deposit on Starknet into USDC on Base, then executing an Intent Connect transfer to a final EVM vault address.

The binary coordinates two external services:

- Aurora Swap API: quotes and executes the STRK to USDC cross-chain swap.
- Intent Connect API: provides an intermediary EVM address, builds the EVM execution payload, and accepts the backend signature.

## Flow

1. Load runtime configuration from `.env`.
2. Connect to Starknet and wait until `STRK_VAULT_ADDRESS` receives more STRK.
3. Request the backend intermediary EVM address from Intent Connect.
4. Create an Aurora Swap quote for STRK on Starknet to USDC on Base, with the intermediary address as the recipient.
5. Transfer the detected STRK deposit from the Starknet vault account to the Aurora-provided deposit address.
6. Poll Base until the intermediary address receives USDC.
7. Dry-run the Intent Connect step request to calculate the network fee.
8. Build the real Intent Connect step request for `USDC.transfer(VAULT_ON_EVM, amountAfterFee)`.
9. Sign the payload with `BACKEND_EVM_PRIVATE_KEY` using ERC-191 signing.
10. Submit the signature and poll Base until `VAULT_ON_EVM` receives the final USDC transfer.

## Hardcoded Assets

- STRK on Starknet: `0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d`
- USDC on Base: `0x833589fcd6edb6e08f4c7c32d4f71b54bda02913`
- Aurora origin asset: `nep141:starknet.omft.near`
- Aurora destination asset: `nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near`

## Requirements

- Rust toolchain with support for edition 2024.
- A Starknet RPC endpoint.
- A Base RPC endpoint. If `BASE_RPC_URL` is not set, the app defaults to `https://mainnet.base.org`.
- Aurora Swap API URL and API key.
- Intent Connect API URL.
- Funded Starknet vault account that can transfer STRK and pay Starknet transaction fees.
- Backend EVM private key authorized for the configured Intent Connect backend address.

## Configuration

Create a `.env` file in the project root:

```env
STARKNET_RPC_URL=https://your-starknet-rpc.example
STRK_VAULT_ADDRESS=0x...
STRK_VAULT_PRIVATE_KEY=0x...

BASE_RPC_URL=https://your-base-rpc.example
VAULT_ON_EVM=0x...
BACKEND_EVM_ADDRESS=0x...
BACKEND_EVM_PRIVATE_KEY=0x...

INTENT_CONNECT_API_URL=https://intent-connect.example
AURORA_SWAP_API_URL=https://aurora-swap.example
AURORA_SWAP_API_KEY=...
```

Environment variables:

| Name | Required | Description |
| --- | --- | --- |
| `STARKNET_RPC_URL` | Yes | Starknet JSON-RPC endpoint. |
| `STRK_VAULT_ADDRESS` | Yes | Starknet account address that receives the initial STRK deposit and sends STRK to Aurora Swap. |
| `STRK_VAULT_PRIVATE_KEY` | Yes | Private key for `STRK_VAULT_ADDRESS`. |
| `BASE_RPC_URL` | No | Base RPC endpoint. Defaults to `https://mainnet.base.org`. |
| `VAULT_ON_EVM` | Yes | Final EVM destination address for the USDC transfer. |
| `BACKEND_EVM_ADDRESS` | Yes | Backend EVM address registered with Intent Connect. |
| `BACKEND_EVM_PRIVATE_KEY` | Yes | Private key used to sign the Intent Connect payload. |
| `INTENT_CONNECT_API_URL` | Yes | Base URL for the Intent Connect API. |
| `AURORA_SWAP_API_URL` | Yes | Base URL for the Aurora Swap API. |
| `AURORA_SWAP_API_KEY` | Yes | API key used in the Aurora quote endpoint path. |

Do not commit real `.env` files or private keys.

## Build

```sh
cargo check
cargo build
```

## Run

```sh
cargo run
```

After startup, the program waits for an increase in the STRK balance of `STRK_VAULT_ADDRESS`. Send STRK to that account to trigger the flow.

The process logs each major step:

- detected STRK deposit amount;
- Intent Connect intermediary EVM address;
- Starknet transfer transaction hash;
- detected USDC deposit on the intermediary EVM address;
- calculated Intent Connect network fee;
- generated signature;
- final USDC deposit received by `VAULT_ON_EVM`.

Press `Ctrl+C` while the app is polling for deposits to stop it cleanly.

## Project Layout

```text
src/main.rs      End-to-end orchestration for the STRK to USDC flow.
src/intents.rs   Aurora Swap and Intent Connect request/response helpers.
src/utils.rs     Starknet provider, STRK transfer, Base ERC-20 balance, and signing helpers.
Cargo.toml       Rust package metadata and dependencies.
```

## Notes

- This is a single-run POC. It tracks the balance increase observed after startup and processes that deposit amount.
- The polling loops check balances every 2 seconds.
- Amounts are handled in token base units. The console output formats the initial STRK amount as `deposit.low() / 1e18`.
- The Intent Connect request first runs in dry mode to obtain the network fee, then submits a second non-dry request using the deposit amount minus that fee.
- The current step request transfers USDC from the intermediary account to `VAULT_ON_EVM`. Replace `create_steps_request` in `src/intents.rs` if the final Base-side action should be different.

## Troubleshooting

- `quote not found`: inspect the Aurora Swap response, API URL, API key, asset IDs, amount, and sponsor/refund address.
- `intermediary address not found`: verify `INTENT_CONNECT_API_URL` and `BACKEND_EVM_ADDRESS`.
- `networkFee field not found`: the Intent Connect response shape differs from what this POC expects, or the dry-run request failed semantically.
- No progress after startup: confirm that `STRK_VAULT_ADDRESS` received a new STRK balance increase after the process started.
- No USDC detected on Base: check the Starknet transfer transaction, Aurora Swap quote/deposit status, the Base RPC endpoint, and the intermediary EVM address.
