use alloy::hex::FromHex;
use alloy::primitives::Address;
use base64::Engine;
use serde::Deserialize;
use std::env;
use std::ops::Add;
use time::format_description::well_known::Rfc3339;

use crate::USDC_ON_BASE;

#[derive(Debug, Deserialize)]
pub struct QuoteResponse {
    #[serde(default, rename = "depositAddress")]
    pub deposit_address: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "amountOut")]
    pub amount: String,
}

#[derive(Debug)]
pub struct StepResponse {
    pub fee: u128,
    pub id: Option<String>,
    pub payload: Option<Vec<u8>>,
}

pub async fn fetch_intermediary_address() -> anyhow::Result<Address> {
    let response: serde_json::Value = reqwest::get(format!(
        "{}/{}/intermediary",
        env::var("INTENT_CONNECT_API_URL")?,
        env::var("BACKEND_EVM_ADDRESS")?
    ))
    .await?
    .json()
    .await?;

    response["result"]["evm"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("intermediary address not found"))
        .map(|a| Address::from_hex(a).unwrap())
}

pub fn create_quote_request(
    is_dry: bool,
    amount: starknet::core::types::U256,
    recipient: Address,
) -> anyhow::Result<serde_json::Value> {
    let deadline = time::OffsetDateTime::now_utc()
        .add(time::Duration::minutes(5))
        .truncate_to_second()
        .format(&Rfc3339)?;

    Ok(serde_json::json!({
        "dry": is_dry,
        "swapType": "EXACT_INPUT",
        "depositType": "ORIGIN_CHAIN",
        "depositMode": "SIMPLE",
        "quoteWaitingTimeMs": 3000,
        "sessionId": "session_abc123",
        "amount": amount.to_string(),
        "originAsset": "nep141:starknet.omft.near", // STRK on Starknet
        "destinationAsset": "nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near", // "nep141:eth-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.omft.near", // USDC on Ethereum
        "slippageTolerance": 100,
        "refundTo": env::var("STRK_VAULT_ADDRESS")?,
        "refundType": "ORIGIN_CHAIN",
        "recipient": recipient,
        "recipientType": "DESTINATION_CHAIN",
        "deadline": deadline,
    }))
}

pub async fn execute_quote_request(
    quote_request: &serde_json::Value,
) -> anyhow::Result<QuoteResponse> {
    let response = reqwest::Client::new()
        .request(
            reqwest::Method::POST,
            format!(
                "{}/api/quote/{}",
                env::var("AURORA_SWAP_API_URL")?,
                env::var("AURORA_SWAP_API_KEY")?
            ),
        )
        .json(&quote_request)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    serde_json::from_value::<QuoteResponse>(
        response
            .get("quote")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("quote not found"))?,
    )
    .map_err(Into::into)
}

pub fn create_steps_request(
    is_dry: bool,
    amount: alloy::primitives::U256,
    recipient: Address,
) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "version": "1.0",
        "type": "evm",
        "dry": is_dry,
        "destinationAsset": "nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near",
        "steps": [
          {
              "to": USDC_ON_BASE,
              "functionSignature": "transfer(address,uint256)",
              "parameters": [
                  recipient.to_string(),
                  amount.to_string()
              ],
              "value": "0"
            }
        ]
    }))
}

pub async fn execute_steps_request(request: &serde_json::Value) -> anyhow::Result<StepResponse> {
    let response = reqwest::Client::new()
        .post(format!(
            "{}/{}/steps",
            env::var("INTENT_CONNECT_API_URL")?,
            env::var("BACKEND_EVM_ADDRESS")?,
        ))
        .json(request)
        .send()
        .await?;

    anyhow::ensure!(
        response.status() == reqwest::StatusCode::OK
            || response.status() == reqwest::StatusCode::CREATED,
        "Failed to get quote: {}",
        response.text().await?
    );

    let response = &response.json::<serde_json::Value>().await?["result"];

    Ok(StepResponse {
        fee: response["details"]["networkFee"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("networkFee field not found"))
            .map(|a| a.parse().unwrap())?,
        id: response["id"].as_str().map(str::to_string),
        payload: response["details"]["payload"]["payload_bytes_base64"]
            .as_str()
            .map(|a| base64::engine::general_purpose::STANDARD.decode(a).unwrap()),
    })
}

pub async fn submit_signature(id: &str, signature: &str) -> anyhow::Result<()> {
    let result = reqwest::Client::new()
        .post(format!(
            "{}/{}/submit",
            env::var("INTENT_CONNECT_API_URL")?,
            env::var("BACKEND_EVM_ADDRESS")?,
        ))
        .json(&serde_json::json!({
            "executionId": id,
            "signature": signature
        }))
        .send()
        .await?;

    anyhow::ensure!(
        result.status() == reqwest::StatusCode::OK,
        "Failed to submit signature: {}",
        result.text().await?
    );

    Ok(())
}
