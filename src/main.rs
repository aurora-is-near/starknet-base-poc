use alloy::hex::FromHex;
use alloy::primitives::{Address, address};
use starknet::core::types::Felt;
use std::env;

use crate::intents::{QuoteResponse, StepResponse};

mod intents;
mod utils;

const USDC_ON_BASE: Address = address!("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913");

/// The example demonstrates how to use AURORA SWAP API for swapping STRK to USDC and getting
/// them on the Base chain, and INTENT CONNECT API for executing the custom logic of the intent
/// on the Base chain. The flow requires a `.env` file in the root of the project with such vars:
/// `STARKNET_RPC_URL` - URL for the Starknet RPC node.
/// `STRK_VAULT_ADDRESS` - address for incoming deposits on the Starknet.
/// `STRK_VAULT_PRIVATE_KEY` - private key for the vault account on the Starknet.
/// Required for sending STRK tokens to deposit address provided by AURORA SWAP API.
/// `BASE_RPC_URL` - URL for the Base RPC node.
/// `VAULT_ON_EVM` - address for the vault account on the EVM. Final destination of the deposit.
/// `BACKEND_EVM_ADDRESS` - address for the backend EVM address.
/// `BACKEND_EVM_PRIVATE_KEY` - a private key for the backend EVM address. Required for signing the
/// payload of the intent for executing the custom logic on the Base chain.
/// `INTENT_CONNECT_API_URL` - URL for the INTENT CONNECT API.
/// `AURORA_SWAP_API_URL` - URL for the AURORA SWAP API.
/// `AURORA_SWAP_API_KEY` - API key for the AURORA SWAP API.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    let provider = utils::create_provider()?;
    let vault_address = utils::read_strk_address("STRK_VAULT_ADDRESS")?;

    // 1. Wait for balance increasing on the starknet account.

    // Check balance in a loop and break when the balance is increased.
    println!(">>> Start waiting for balance increasing on the starknet account");
    let deposit = utils::wait_for_strk_deposit(&provider, vault_address).await?;
    println!(">>> Received deposit: {} STRK", deposit.low() as f64 / 1e18);

    let start = std::time::Instant::now();

    // 2. At this point we've received the funds from the user and are ready to move them to the
    // intermediary EVM address via Aurora Swap API.

    // Get intermediary EVM address.
    let intermediary_address = intents::fetch_intermediary_address().await?;

    println!(">>> Intermediary EVM address: {intermediary_address}");

    // Create Aurora Swap API quote request.
    let quote_request = intents::create_quote_request(false, deposit, intermediary_address)?;
    // Send the request to the Aurora Swap API and get a quote.
    let QuoteResponse {
        deposit_address, ..
    } = intents::execute_quote_request(&quote_request).await?;

    // Make a transfer to the deposit address provided by the Aurora Swap API on Starknet.
    let transfer_result = utils::transfer_strk(
        &provider,
        utils::read_strk_address("STRK_VAULT_PRIVATE_KEY")?,
        vault_address,
        deposit_address
            .and_then(|a| Felt::from_hex(&a).ok())
            .ok_or_else(|| anyhow::anyhow!("deposit address not found"))?,
        deposit,
    )
    .await?;

    println!(
        ">>> Transfer is completed, transaction hash: {}",
        transfer_result.transaction_hash
    );

    // 3. Wait for the transfer to be confirmed on the EVM network.

    // Check balance in a loop and break when the balance is increased. In real life, could be
    // replaced with listening to the event on the EVM network.
    println!(">>> Start waiting for balance increasing on the intermediary EVM address");

    let base_provider = utils::create_base_provider()?;
    let usdc_deposit =
        utils::wait_for_base_deposit(&base_provider, USDC_ON_BASE, intermediary_address).await?;

    println!(">>> Received USDC deposit: {usdc_deposit}");

    // 4. At this point we've received the funds on the intermediary EVM address and are ready
    // to move them to the custody address or execute another custom logic.

    // The first request must be dry. We want to get the fee amount to substruct it from the deposit amount.
    let vault = Address::from_hex(&env::var("VAULT_ON_EVM")?)?;
    let steps_request = intents::create_steps_request(true, usdc_deposit, vault)?;
    let steps_response = intents::execute_steps_request(&steps_request).await?;

    let usdc_deposit_without_fee =
        usdc_deposit.saturating_sub(alloy::primitives::U256::from(steps_response.fee));

    anyhow::ensure!(
        usdc_deposit_without_fee > alloy::primitives::U256::ZERO,
        "Nothing to deposit after fee deduction"
    );

    println!(
        ">>> Amount of USDC without fee: {usdc_deposit_without_fee}, and fee: {}",
        steps_response.fee
    );

    // The second request is for getting payload of intent for signing.
    let steps_request = intents::create_steps_request(false, usdc_deposit_without_fee, vault)?;
    let StepResponse { id, payload, .. } = intents::execute_steps_request(&steps_request).await?;

    // Sign the payload with the private key of the backend EVM address.
    let signature = utils::sign_message_erc191(
        &env::var("BACKEND_EVM_PRIVATE_KEY")?,
        payload
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Payload is empty"))?,
    )?;
    println!(">>> Signature: {signature}");

    intents::submit_signature(
        id.as_deref()
            .ok_or_else(|| anyhow::anyhow!("ID is empty"))?,
        &signature,
    )
    .await?;

    println!(">>> Start waiting for balance increasing on the VAULT EVM address");

    let final_deposit = utils::wait_for_base_deposit(&base_provider, USDC_ON_BASE, vault).await?;

    println!(
        ">>> Received final deposit on a VAULT: {final_deposit}, the full flow took: {} seconds",
        start.elapsed().as_secs()
    );

    Ok(())
}
