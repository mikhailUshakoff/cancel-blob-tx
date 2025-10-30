use alloy::{
    consensus::Transaction,
    network::EthereumWallet,
    primitives::{TxHash, TxKind, U256},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
    transports::http::reqwest,
};
use anyhow::{Context, Result, bail};
use dotenvy::dotenv;
use std::str::FromStr;

use alloy::consensus::{SidecarBuilder, SimpleCoder};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let rpc_url = std::env::var("RPC_URL").context("RPC_URL environment variable not set")?;
    let private_key =
        std::env::var("PRIVATE_KEY").context("PRIVATE_KEY environment variable not set")?;
    let tx_hash: TxHash = std::env::var("TX_HASH")
        .context("TX_HASH environment variable not set")?
        .parse()
        .context("Invalid transaction hash format")?;

    let signer =
        PrivateKeySigner::from_str(private_key.as_str()).context("Invalid private key format")?;
    let from = signer.address();
    let wallet: EthereumWallet = signer.into();
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url.parse::<reqwest::Url>()?)
        .erased();

    // Test connection and get current state
    let current_block = provider
        .get_block_number()
        .await
        .context("Failed to connect to RPC endpoint")?;
    println!("Connected to RPC. Current block number: {current_block}");

    // Fetch and validate original transaction
    let Some(orig_tx) = provider
        .get_transaction_by_hash(tx_hash)
        .await
        .context("Failed to fetch transaction")?
    else {
        bail!("Transaction not found: {tx_hash}");
    };

    let tx = orig_tx
        .inner
        .as_eip4844()
        .context("Transaction is not an EIP-4844 blob transaction")?;

    println!(
        "Found EIP-4844 transaction to {:?} with nonce {}",
        tx.to(),
        tx.nonce()
    );

    // Check if transaction is still pending
    if let Some(receipt) = provider
        .get_transaction_receipt(tx_hash)
        .await
        .context("Failed to check transaction receipt")?
    {
        println!(
            "⚠️  Transaction already confirmed in block {}",
            receipt.block_number.unwrap_or_default()
        );
        return Ok(());
    }

    // Log current fee parameters
    println!("Original transaction fees:");
    println!(
        "  Max fee per gas: {} gwei",
        tx.max_fee_per_gas() / 1_000_000_000
    );
    println!(
        "  Max priority fee per gas: {} gwei",
        tx.max_priority_fee_per_gas().unwrap() / 1_000_000_000
    );
    println!("  Gas limit: {}", tx.gas_limit());
    println!(
        "  Max fee per blob gas: {} gwei",
        tx.max_fee_per_blob_gas().unwrap() / 1_000_000_000
    );

    // Calculate new fees - simply double the original fees
    let new_max_fee = tx.max_fee_per_gas() * 2;
    let new_priority_fee = tx.max_priority_fee_per_gas().unwrap() * 2;

    println!("New transaction fees:");
    println!("  Max fee per gas: {} gwei", new_max_fee / 1_000_000_000);
    println!(
        "  Max priority fee per gas: {} gwei",
        new_priority_fee / 1_000_000_000
    );

    // Create blob sidecar (empty to cancel the original blob transaction)
    let sidecar: SidecarBuilder<SimpleCoder> = SidecarBuilder::from_slice(b"Hello, world!");
    let sidecar = sidecar.build().context("Failed to build blob sidecar")?;

    // Build replacement transaction
    let replacement_tx = TransactionRequest {
        to: Some(TxKind::Call(from)), // Send to self to cancel
        value: Some(U256::ZERO),
        gas: Some(21_000), // Standard transfer gas limit
        max_fee_per_gas: Some(new_max_fee),
        max_priority_fee_per_gas: Some(new_priority_fee),
        nonce: Some(tx.nonce()),
        blob_versioned_hashes: Some(vec![]), // Empty to cancel blob data
        sidecar: Some(sidecar),
        ..Default::default()
    };

    println!("Sending replacement transaction...");
    let pending_tx = provider
        .send_transaction(replacement_tx)
        .await
        .context("Failed to send replacement transaction")?;

    println!("Replacement transaction sent: {}", pending_tx.tx_hash());

    Ok(())
}
