use crate::settlement::SettlementRequest;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

#[derive(Deserialize, Debug)]
struct SolanaResponse {
    result: Option<SolanaTxResult>,
}

#[derive(Deserialize, Debug)]
struct SolanaTxResult {
    meta: Option<SolanaMeta>,
    transaction: Option<SolanaTransactionData>,
    slot: Option<u64>,
}

#[derive(Deserialize, Debug)]
struct SolanaMeta {
    err: Option<serde_json::Value>,
    fee: Option<u64>,
    pre_balances: Option<Vec<u64>>,
    post_balances: Option<Vec<u64>>,
}

#[derive(Deserialize, Debug)]
struct SolanaTransactionData {
    message: Option<SolanaMessage>,
}

#[derive(Deserialize, Debug)]
struct SolanaMessage {
    account_keys: Option<Vec<SolanaAccountKey>>,
}

#[derive(Deserialize, Debug)]
struct SolanaAccountKey {
    pubkey: String,
    signer: bool,
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Verify native SOL transaction.
///
/// Checks:
/// 1. Transaction exists
/// 2. Transaction succeeded
/// 3. Sender exists and signed
/// 4. Receiver exists
/// 5. Receiver received EXACT expected SOL
/// 6. Sender sent at least expected SOL
/// 7. Optional block/slot validation
pub async fn verify_solana_transaction(
    req: &SettlementRequest,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let rpc_url = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| {
            "https://api.mainnet-beta.solana.com".to_string()
        });

    let client = reqwest::Client::new();

    println!(
        "[AUDIT] [Timestamp: {}] [Solana Engine] INIT_VERIFICATION \
        | InvoiceID: {} | TxHash: {} | From: {} | To: {} \
        | ExpectedAmount: {} {}",
        current_timestamp(),
        req.invoice_id,
        req.tx_hash,
        req.sender_address,
        req.receiver_address,
        req.amount_paid,
        req.currency
    );

    // --------------------------------------------------
    // Only native SOL here
    // --------------------------------------------------

    if req.currency.to_uppercase() != "SOL" {
        eprintln!(
            "[Solana Engine] Unsupported currency for native SOL verification: {}",
            req.currency
        );

        return Ok(false);
    }

    // --------------------------------------------------
    // Convert SOL -> lamports
    // --------------------------------------------------

    if !req.amount_paid.is_finite() || req.amount_paid <= 0.0 {
        eprintln!(
            "[Solana Engine] Invalid amount: {}",
            req.amount_paid
        );

        return Ok(false);
    }

    let expected_lamports = (req.amount_paid * 1_000_000_000.0).round() as u64;

    println!(
        "[Solana Engine] Expected amount: {} SOL = {} lamports",
        req.amount_paid,
        expected_lamports
    );

    // --------------------------------------------------
    // RPC getTransaction
    // --------------------------------------------------

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": [
            req.tx_hash,
            {
                "encoding": "jsonParsed",
                "commitment": "finalized",
                "maxSupportedTransactionVersion": 0
            }
        ]
    });

    let res = client
        .post(&rpc_url)
        .json(&payload)
        .send()
        .await?
        .json::<SolanaResponse>()
        .await?;

    // --------------------------------------------------
    // Transaction must exist
    // --------------------------------------------------

    let tx_result = match res.result {
        Some(result) => result,
        None => {
            eprintln!(
                "[AUDIT] [Timestamp: {}] [Solana Engine] \
                TX_NOT_FOUND | InvoiceID: {} | TxHash: {}",
                current_timestamp(),
                req.invoice_id,
                req.tx_hash
            );

            return Ok(false);
        }
    };

    // --------------------------------------------------
    // Meta must exist
    // --------------------------------------------------

    let meta = match tx_result.meta {
        Some(meta) => meta,
        None => {
            eprintln!(
                "[Solana Engine] Transaction metadata missing"
            );

            return Ok(false);
        }
    };

    // --------------------------------------------------
    // Transaction must succeed
    // --------------------------------------------------

    if meta.err.is_some() {
        eprintln!(
            "[AUDIT] [Timestamp: {}] [Solana Engine] \
            TX_FAILED | InvoiceID: {} | TxHash: {} | Error: {:?}",
            current_timestamp(),
            req.invoice_id,
            req.tx_hash,
            meta.err
        );

        return Ok(false);
    }

    // --------------------------------------------------
    // Transaction data
    // --------------------------------------------------

    let transaction = match tx_result.transaction {
        Some(transaction) => transaction,
        None => {
            eprintln!(
                "[Solana Engine] Transaction data missing"
            );

            return Ok(false);
        }
    };

    let message = match transaction.message {
        Some(message) => message,
        None => {
            eprintln!(
                "[Solana Engine] Transaction message missing"
            );

            return Ok(false);
        }
    };

    let account_keys = match message.account_keys {
        Some(keys) => keys,
        None => {
            eprintln!(
                "[Solana Engine] Account keys missing"
            );

            return Ok(false);
        }
    };

    // --------------------------------------------------
    // Find sender and receiver indexes
    // --------------------------------------------------

    let sender_index = account_keys
        .iter()
        .position(|account| {
            account.pubkey == req.sender_address && account.signer
        });

    let sender_index = match sender_index {
        Some(index) => index,
        None => {
            eprintln!(
                "[SECURITY] Sender {} did not sign this transaction",
                req.sender_address
            );

            return Ok(false);
        }
    };

    let receiver_index = account_keys
        .iter()
        .position(|account| {
            account.pubkey == req.receiver_address
        });

    let receiver_index = match receiver_index {
        Some(index) => index,
        None => {
            eprintln!(
                "[SECURITY] Receiver {} not found in transaction",
                req.receiver_address
            );

            return Ok(false);
        }
    };

    // --------------------------------------------------
    // Balance arrays
    // --------------------------------------------------

    let pre_balances = match meta.pre_balances {
        Some(balances) => balances,
        None => {
            eprintln!(
                "[Solana Engine] pre_balances missing"
            );

            return Ok(false);
        }
    };

    let post_balances = match meta.post_balances {
        Some(balances) => balances,
        None => {
            eprintln!(
                "[Solana Engine] post_balances missing"
            );

            return Ok(false);
        }
    };

    if sender_index >= pre_balances.len()
        || sender_index >= post_balances.len()
        || receiver_index >= pre_balances.len()
        || receiver_index >= post_balances.len()
    {
        eprintln!(
            "[Solana Engine] Account balance index out of bounds"
        );

        return Ok(false);
    }

    // --------------------------------------------------
    // Calculate balance changes
    // --------------------------------------------------

    let sender_pre = pre_balances[sender_index];
    let sender_post = post_balances[sender_index];

    let receiver_pre = pre_balances[receiver_index];
    let receiver_post = post_balances[receiver_index];

    let sender_decrease =
        sender_pre.saturating_sub(sender_post);

    let receiver_increase =
        receiver_post.saturating_sub(receiver_pre);

    println!(
        "[Solana Engine] Sender balance change: {} lamports",
        sender_decrease
    );

    println!(
        "[Solana Engine] Receiver balance change: {} lamports",
        receiver_increase
    );

    // --------------------------------------------------
    // EXACT RECEIVER AMOUNT
    // --------------------------------------------------

    if receiver_increase != expected_lamports {
        eprintln!(
            "[SECURITY] AMOUNT_MISMATCH \
            | Expected: {} lamports \
            | Received: {} lamports",
            expected_lamports,
            receiver_increase
        );

        return Ok(false);
    }

    // --------------------------------------------------
    // Sender must have sent at least expected amount
    // --------------------------------------------------

    if sender_decrease < expected_lamports {
        eprintln!(
            "[SECURITY] SENDER_AMOUNT_MISMATCH \
            | Expected: {} lamports \
            | Sender decreased: {} lamports",
            expected_lamports,
            sender_decrease
        );

        return Ok(false);
    }

    // --------------------------------------------------
    // Optional slot validation
    // --------------------------------------------------

    let slot = tx_result.slot.unwrap_or(0);

    if req.block_number > 0
        && slot != req.block_number as u64
    {
        eprintln!(
            "[SECURITY] SLOT_MISMATCH \
            | Expected: {} \
            | Actual: {}",
            req.block_number,
            slot
        );

        return Ok(false);
    }

    // --------------------------------------------------
    // SUCCESS
    // --------------------------------------------------

    let fee = meta.fee.unwrap_or(0);

    println!(
        "[AUDIT] [Timestamp: {}] [Solana Engine] \
        SUCCESS_SETTLED \
        | InvoiceID: {} \
        | TxHash: {} \
        | Slot: {} \
        | Fee: {} lamports \
        | From: {} \
        | To: {} \
        | Amount: {} SOL \
        | Lamports: {}",
        current_timestamp(),
        req.invoice_id,
        req.tx_hash,
        slot,
        fee,
        req.sender_address,
        req.receiver_address,
        req.amount_paid,
        expected_lamports
    );

    Ok(true)
}
