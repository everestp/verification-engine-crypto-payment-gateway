use crate::settlement::SettlementRequest;
use super::VerifiedTxEvent;
use serde::Deserialize;
use std::env;
use tokio::time::{sleep, Duration};

#[derive(Deserialize, Debug)]
struct EvmRpcResponse {
    result: Option<EvmTransaction>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct EvmTransaction {
    from: String,
    to: Option<String>,
    value: String,
    block_number: Option<String>,
}

#[derive(Deserialize, Debug)]
struct EvmReceiptResponse {
    result: Option<EvmReceipt>,
}

#[derive(Deserialize, Debug)]
struct EvmReceipt {
    status: String,

    #[serde(default)]
    logs: Vec<EvmLog>,
}

#[derive(Deserialize, Debug)]
struct EvmLog {
    address: String,
    topics: Vec<String>,
    data: String,
}

// keccak256("Transfer(address,address,uint256)")
const ERC20_TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a3a9c4f9a5";

/// Logs a consistent, single-line failure reason for the audit trail.
fn log_failure(req: &SettlementRequest, reason: &str, detail: &str) {
    eprintln!(
        "[AUDIT] [EVM Engine] VERIFICATION_FAILED \
        | InvoiceID: {} | TxHash: {} | Network: {} | Currency: {} \
        | Reason: {} | Detail: {}",
        req.invoice_id,
        req.tx_hash,
        req.network,
        req.currency,
        reason,
        detail
    );
}

pub async fn verify_evm_transaction(
    req: &SettlementRequest,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {

    println!(
        "[EVM RPC] Verifying tx={} invoice={} network={} currency={} sender={} receiver={} amount={}",
        req.tx_hash,
        req.invoice_id,
        req.network,
        req.currency,
        req.sender_address,
        req.receiver_address,
        req.amount_paid
    );

    // --------------------------------------------------
    // 1. Basic validation
    // --------------------------------------------------
    if !req.sender_address.starts_with("0x") || req.sender_address.len() != 42 {
        log_failure(req, "INVALID_SENDER_ADDRESS", &format!("sender_address={}", req.sender_address));
        return Ok(false);
    }

    if !req.receiver_address.starts_with("0x") || req.receiver_address.len() != 42 {
        log_failure(req, "INVALID_RECEIVER_ADDRESS", &format!("receiver_address={}", req.receiver_address));
        return Ok(false);
    }

    if !req.tx_hash.starts_with("0x") || req.tx_hash.len() != 66 {
        log_failure(req, "INVALID_TX_HASH", &format!("tx_hash={}", req.tx_hash));
        return Ok(false);
    }

    if !req.amount_paid.is_finite() || req.amount_paid <= 0.0 {
        log_failure(req, "INVALID_AMOUNT", &format!("amount_paid={}", req.amount_paid));
        return Ok(false);
    }

    // --------------------------------------------------
    // 2. Select RPC (Ethereum mainnet + Sepolia + Polygon)
    // --------------------------------------------------
    let rpc_url = match req.network.to_lowercase().as_str() {
        "ethereum" | "evm" => {
            env::var("ETHEREUM_RPC_URL")
                .unwrap_or_else(|_| "https://ethereum-sepolia-rpc.publicnode.com".to_string())
        }

        // ✅ Sepolia (devnet / testnet)
        "sepolia" | "ethereum-sepolia" | "eth-sepolia" => {
            env::var("SEPOLIA_RPC_URL")
                .unwrap_or_else(|_| "https://ethereum-sepolia-rpc.publicnode.com".to_string())
        }

        "polygon" => {
            env::var("POLYGON_RPC_URL")
                .unwrap_or_else(|_| "https://polygon-rpc.com".to_string())
        }

        network => {
            log_failure(req, "UNSUPPORTED_NETWORK", &format!("network={}", network));
            return Ok(false);
        }
    };

    let client = reqwest::Client::new();

    // --------------------------------------------------
    // 3. Get transaction
    // --------------------------------------------------
    let tx_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionByHash",
        "params": [&req.tx_hash],
        "id": 1
    });

    let response = client
        .post(&rpc_url)
        .header("Content-Type", "application/json")
        .json(&tx_payload)
        .send()
        .await?;

    if !response.status().is_success() {
        log_failure(
            req,
            "RPC_HTTP_ERROR",
            &format!("status={} rpc_url={}", response.status(), rpc_url),
        );
        return Ok(false);
    }

    let body = response.text().await?;

    let tx_res: EvmRpcResponse = match serde_json::from_str(&body) {
        Ok(parsed) => parsed,
        Err(e) => {
            log_failure(req, "TX_JSON_PARSE_ERROR", &format!("error={}", e));
            return Ok(false);
        }
    };

    let tx = match tx_res.result {
        Some(tx) => tx,
        None => {
            log_failure(req, "TX_NOT_FOUND", "RPC returned null result");
            return Ok(false);
        }
    };

    // --------------------------------------------------
    // 4. Verify sender
    // --------------------------------------------------
    if !tx.from.eq_ignore_ascii_case(&req.sender_address) {
        log_failure(
            req,
            "SENDER_MISMATCH",
            &format!("expected_sender={} actual_sender={}", req.sender_address, tx.from),
        );
        return Ok(false);
    }

    // --------------------------------------------------
    // 5. Get receipt
    // --------------------------------------------------
    let receipt_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [&req.tx_hash],
        "id": 2
    });

    let receipt_response = client
        .post(&rpc_url)
        .header("Content-Type", "application/json")
        .json(&receipt_payload)
        .send()
        .await?;

    if !receipt_response.status().is_success() {
        log_failure(
            req,
            "RECEIPT_HTTP_ERROR",
            &format!("status={}", receipt_response.status()),
        );
        return Ok(false);
    }

    let receipt_body = receipt_response.text().await?;

    let receipt_res: EvmReceiptResponse = match serde_json::from_str(&receipt_body) {
        Ok(parsed) => parsed,
        Err(e) => {
            log_failure(req, "RECEIPT_JSON_PARSE_ERROR", &format!("error={}", e));
            return Ok(false);
        }
    };

    let receipt = match receipt_res.result {
        Some(receipt) => receipt,
        None => {
            log_failure(req, "RECEIPT_NOT_AVAILABLE", "receipt not mined yet");
            return Ok(false);
        }
    };

    // --------------------------------------------------
    // 6. Transaction must succeed
    // --------------------------------------------------
    let status_hex = receipt.status.strip_prefix("0x").unwrap_or(&receipt.status);
    let status = u64::from_str_radix(status_hex, 16).unwrap_or(0);

    if status != 1 {
        log_failure(req, "TX_FAILED_ONCHAIN", &format!("status_hex={}", receipt.status));
        return Ok(false);
    }

    // --------------------------------------------------
    // 7. Native ETH / MATIC (works on Sepolia too)
    // --------------------------------------------------
    let currency = req.currency.to_uppercase();

    if currency == "ETH" || currency == "MATIC" {
        let actual_receiver = match tx.to {
            Some(to) => to,
            None => {
                log_failure(
                    req,
                    "NATIVE_NO_RECEIVER",
                    "transaction 'to' field was null (likely a contract creation)",
                );
                return Ok(false);
            }
        };

        if !actual_receiver.eq_ignore_ascii_case(&req.receiver_address) {
            log_failure(
                req,
                "RECEIVER_MISMATCH",
                &format!(
                    "expected_receiver={} actual_receiver={}",
                    req.receiver_address, actual_receiver
                ),
            );
            return Ok(false);
        }

        let value_hex = tx.value.strip_prefix("0x").unwrap_or(&tx.value);
        let actual_wei = u128::from_str_radix(value_hex, 16).unwrap_or(0);

        let expected_wei = (req.amount_paid * 1_000_000_000_000_000_000.0).round() as u128;

        println!("[EVM] Native expected={} wei actual={} wei", expected_wei, actual_wei);

        if actual_wei != expected_wei {
            log_failure(
                req,
                "NATIVE_AMOUNT_MISMATCH",
                &format!("expected_wei={} actual_wei={}", expected_wei, actual_wei),
            );
            return Ok(false);
        }

        println!("[EVM] ✅ Native transaction verified");
        return Ok(true);
    }

    // --------------------------------------------------
    // 8. ERC20 USDT / USDC (mainnet + polygon only for now)
    // --------------------------------------------------
    if currency != "USDT" && currency != "USDC" {
        log_failure(req, "UNSUPPORTED_CURRENCY", &format!("currency={}", currency));
        return Ok(false);
    }

    let expected_units = (req.amount_paid * 1_000_000.0).round() as u128;

    let mut last_amount_mismatch: Option<(u128, u128)> = None;
    let mut saw_matching_from_to = false;

    for log in &receipt.logs {
        if log.topics.len() != 3 {
            continue;
        }

        if !log.topics[0].eq_ignore_ascii_case(ERC20_TRANSFER_TOPIC) {
            continue;
        }

        let from_topic = log.topics[1].trim_start_matches("0x");
        let to_topic = log.topics[2].trim_start_matches("0x");

        if from_topic.len() < 40 || to_topic.len() < 40 {
            continue;
        }

        let actual_from = format!("0x{}", &from_topic[from_topic.len() - 40..]);
        let actual_to = format!("0x{}", &to_topic[to_topic.len() - 40..]);

        if !actual_from.eq_ignore_ascii_case(&req.sender_address) {
            continue;
        }

        if !actual_to.eq_ignore_ascii_case(&req.receiver_address) {
            continue;
        }

        saw_matching_from_to = true;

        let data = log.data.trim_start_matches("0x");
        if data.len() != 64 {
            continue;
        }

        let actual_amount = u128::from_str_radix(data, 16).unwrap_or(0);

        println!(
            "[EVM ERC20] Token={} From={} To={} Amount={}",
            log.address, actual_from, actual_to, actual_amount
        );

        if actual_amount != expected_units {
            last_amount_mismatch = Some((expected_units, actual_amount));
            continue;
        }

        let expected_contract = match (
            req.network.to_lowercase().as_str(),
            currency.as_str(),
        ) {
            ("ethereum", "USDT") => env::var("ETHEREUM_USDT_CONTRACT").unwrap_or_default(),
            ("ethereum", "USDC") => env::var("ETHEREUM_USDC_CONTRACT").unwrap_or_default(),
            ("polygon", "USDT") => env::var("POLYGON_USDT_CONTRACT").unwrap_or_default(),
            ("polygon", "USDC") => env::var("POLYGON_USDC_CONTRACT").unwrap_or_default(),
            // Add Sepolia contracts here later if you need them
            _ => String::new(),
        };

        if !expected_contract.is_empty()
            && !log.address.eq_ignore_ascii_case(&expected_contract)
        {
            log_failure(
                req,
                "TOKEN_CONTRACT_MISMATCH",
                &format!(
                    "expected_contract={} actual_contract={}",
                    expected_contract, log.address
                ),
            );
            continue;
        }

        println!("[EVM] ✅ ERC20 {} transaction verified", currency);
        return Ok(true);
    }

    if let Some((expected, actual)) = last_amount_mismatch {
        log_failure(
            req,
            "TOKEN_AMOUNT_MISMATCH",
            &format!("expected_units={} actual_units={}", expected, actual),
        );
    } else if saw_matching_from_to {
        log_failure(
            req,
            "TOKEN_TRANSFER_LOG_MALFORMED",
            "found matching from/to Transfer log but data field was not 32 bytes",
        );
    } else {
        log_failure(
            req,
            "NO_MATCHING_TRANSFER_EVENT",
            &format!(
                "no {} Transfer log found from={} to={}",
                currency, req.sender_address, req.receiver_address
            ),
        );
    }

    Ok(false)
}
