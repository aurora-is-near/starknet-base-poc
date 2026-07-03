use alloy::network::TransactionBuilder;
use alloy::primitives::{Address, U256 as BaseU256};
use alloy::providers::{Provider as BaseProvider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::SignerSync;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::SolCall;
use reqwest::Url;
use starknet::accounts::{Account, ExecutionEncoding, SingleOwnerAccount};
use starknet::core::types::{
    BlockId, BlockTag, Call, Felt, FunctionCall, InvokeTransactionResult, U256,
};
use starknet::core::utils::get_selector_from_name;
use starknet::macros::felt;
use starknet::providers::jsonrpc::HttpTransport;
use starknet::providers::{JsonRpcClient, Provider};
use starknet::signers::{LocalWallet, SigningKey};
use std::env;

const STRK_TOKEN_ADDRESS: Felt =
    felt!("0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d");

pub fn create_provider() -> anyhow::Result<JsonRpcClient<HttpTransport>> {
    let url = Url::parse(&env::var("STARKNET_RPC_URL")?)?;
    Ok(JsonRpcClient::new(HttpTransport::new(url)))
}

pub async fn strk_balance<P>(provider: &P, account_address: Felt) -> anyhow::Result<U256>
where
    P: Provider + Send + Sync,
{
    let raw = provider
        .call(
            FunctionCall {
                contract_address: STRK_TOKEN_ADDRESS,
                entry_point_selector: get_selector_from_name("balanceOf")?,
                calldata: vec![account_address],
            },
            BlockId::Tag(BlockTag::Latest),
        )
        .await?;

    let [low, high] = raw.as_slice() else {
        anyhow::bail!("unexpected STRK balance response: {raw:?}");
    };

    Ok(U256::from_words((*low).try_into()?, (*high).try_into()?))
}

pub async fn wait_for_strk_deposit<P>(provider: &P, deposit_address: Felt) -> anyhow::Result<U256>
where
    P: Provider + Send + Sync,
{
    let current_balance = strk_balance(provider, deposit_address).await?;

    loop {
        let new_balance = strk_balance(provider, deposit_address).await?;

        if new_balance > current_balance {
            return Ok(new_balance - current_balance);
        } else {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => anyhow::bail!("Received SIGINT, exiting..."),
                _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
            }
        }
    }
}

sol! {
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
    }
}

pub fn sign_message_erc191(private_key: &str, message: &[u8]) -> anyhow::Result<String> {
    let signer: PrivateKeySigner = private_key.parse()?;
    let signature = signer.sign_message_sync(message)?;
    // Equivalent to normalizing v from 27/28 to 0/1, then base58 encoding.
    let sig_bytes = signature.as_rsy();

    Ok(format!(
        "secp256k1:{}",
        bs58::encode(sig_bytes).into_string()
    ))
}

pub fn create_base_provider() -> anyhow::Result<impl BaseProvider> {
    let url = env::var("BASE_RPC_URL").unwrap_or_else(|_| "https://mainnet.base.org".to_string());
    Ok(ProviderBuilder::new().connect_http(Url::parse(&url)?))
}

pub async fn base_erc20_balance<P>(
    provider: &P,
    token: Address,
    account: Address,
) -> anyhow::Result<BaseU256>
where
    P: BaseProvider,
{
    let tx = TransactionRequest::default()
        .with_to(token)
        .with_input(IERC20::balanceOfCall { account }.abi_encode());

    let raw = provider.call(tx).await?;

    Ok(BaseU256::from_be_slice(&raw))
}

pub async fn wait_for_base_deposit<P>(
    provider: &P,
    token: Address,
    account: Address,
) -> anyhow::Result<BaseU256>
where
    P: BaseProvider,
{
    let current_balance = base_erc20_balance(provider, token, account).await?;

    loop {
        let new_balance = base_erc20_balance(provider, token, account).await?;

        if new_balance > current_balance {
            return Ok(new_balance - current_balance);
        } else {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => anyhow::bail!("Received SIGINT, exiting..."),
                _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
            }
        }
    }
}

pub async fn transfer_strk<P>(
    provider: P,
    sender_private_key: Felt,
    sender_address: Felt,
    recipient: Felt,
    amount: U256,
) -> anyhow::Result<InvokeTransactionResult>
where
    P: Provider + Send + Sync,
{
    let signer = LocalWallet::from(SigningKey::from_secret_scalar(sender_private_key));
    let chain_id = provider.chain_id().await?;

    let mut account = SingleOwnerAccount::new(
        provider,
        signer,
        sender_address,
        chain_id,
        ExecutionEncoding::New,
    );
    account.set_block_id(BlockId::Tag(BlockTag::Latest));

    let call = Call {
        to: STRK_TOKEN_ADDRESS,
        selector: get_selector_from_name("transfer")?,
        calldata: vec![
            recipient,
            Felt::from(amount.low()),
            Felt::from(amount.high()),
        ],
    };

    let request = account.execute_v3(vec![call]);
    let _fee = request.estimate_fee().await?;

    request.send().await.map_err(Into::into)
}

pub fn read_strk_address(key: &str) -> anyhow::Result<Felt> {
    let value = env::var(key)?;
    Felt::from_hex(&value).map_err(Into::into)
}
