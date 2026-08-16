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

// keccak256(
//   "Transfer(address,address,uint256)"
// )
const ERC20_TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a3a9c4f9a5";

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

    if !req.sender_address.starts_with("0x")
        || req.sender_address.len() != 42
    {
        eprintln!(
            "[EVM Validation] Invalid sender address: {}",
            req.sender_address
        );

        return Ok(false);
    }

    if !req.receiver_address.starts_with("0x")
        || req.receiver_address.len() != 42
    {
        eprintln!(
            "[EVM Validation] Invalid receiver address: {}",
            req.receiver_address
        );

        return Ok(false);
    }

    if !req.tx_hash.starts_with("0x")
        || req.tx_hash.len() != 66
    {
        eprintln!(
            "[EVM Validation] Invalid transaction hash: {}",
            req.tx_hash
        );

        return Ok(false);
    }

    if !req.amount_paid.is_finite() || req.amount_paid <= 0.0 {
        eprintln!(
            "[EVM Validation] Invalid amount: {}",
            req.amount_paid
        );

        return Ok(false);
    }

    // --------------------------------------------------
    // 2. Select RPC
    // --------------------------------------------------

    let rpc_url = match req.network.to_lowercase().as_str() {
        "ethereum" | "evm" => {
            env::var("ETHEREUM_RPC_URL")
                .unwrap_or_else(|_| {
                    "https://eth.llamarpc.com".to_string()
                })
        }

        "polygon" => {
            env::var("POLYGON_RPC_URL")
                .unwrap_or_else(|_| {
                    "https://polygon-rpc.com".to_string()
                })
        }

        network => {
            eprintln!(
                "[EVM Validation] Unsupported network: {}",
                network
            );

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
        eprintln!(
            "[EVM RPC] HTTP error: {}",
            response.status()
        );

        return Ok(false);
    }

    let body = response.text().await?;

    let tx_res: EvmRpcResponse =
        match serde_json::from_str(&body) {
            Ok(parsed) => parsed,
            Err(e) => {
                eprintln!(
                    "[EVM RPC] JSON parse error: {}",
                    e
                );

                return Ok(false);
            }
        };

    let tx = match tx_res.result {
        Some(tx) => tx,

        None => {
            eprintln!(
                "[EVM] Transaction not found: {}",
                req.tx_hash
            );

            return Ok(false);
        }
    };

    // --------------------------------------------------
    // 4. Verify sender
    // --------------------------------------------------

    if !tx.from.eq_ignore_ascii_case(
        &req.sender_address
    ) {
        eprintln!(
            "[EVM Security] Sender mismatch. Expected={} actual={}",
            req.sender_address,
            tx.from
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
        return Ok(false);
    }

    let receipt_body = receipt_response.text().await?;

    let receipt_res: EvmReceiptResponse =
        match serde_json::from_str(&receipt_body) {
            Ok(parsed) => parsed,
            Err(e) => {
                eprintln!(
                    "[EVM Receipt] JSON parse error: {}",
                    e
                );

                return Ok(false);
            }
        };

    let receipt = match receipt_res.result {
        Some(receipt) => receipt,

        None => {
            eprintln!(
                "[EVM] Receipt not available yet: {}",
                req.tx_hash
            );

            return Ok(false);
        }
    };

    // --------------------------------------------------
    // 6. Transaction must succeed
    // --------------------------------------------------

    let status_hex =
        receipt.status.strip_prefix("0x")
            .unwrap_or(&receipt.status);

    let status =
        u64::from_str_radix(status_hex, 16)
            .unwrap_or(0);

    if status != 1 {
        eprintln!(
            "[EVM] Transaction failed/reverted"
        );

        return Ok(false);
    }

    // --------------------------------------------------
    // 7. Native ETH / MATIC
    // --------------------------------------------------

    let currency = req.currency.to_uppercase();

    if currency == "ETH" || currency == "MATIC" {

        // Native transaction must go directly
        // to the expected receiver.
        let actual_receiver = match tx.to {
            Some(to) => to,

            None => {
                eprintln!(
                    "[EVM] Native transaction has no receiver"
                );

                return Ok(false);
            }
        };

        if !actual_receiver.eq_ignore_ascii_case(
            &req.receiver_address
        ) {
            eprintln!(
                "[EVM Security] Receiver mismatch. Expected={} actual={}",
                req.receiver_address,
                actual_receiver
            );

            return Ok(false);
        }

        // --------------------------------------------------
        // Native amount
        // --------------------------------------------------

        let value_hex =
            tx.value.strip_prefix("0x")
                .unwrap_or(&tx.value);

        let actual_wei =
            u128::from_str_radix(value_hex, 16)
                .unwrap_or(0);

        let expected_wei =
            (req.amount_paid * 1_000_000_000_000_000_000.0)
                .round() as u128;

        println!(
            "[EVM] Native expected={} wei actual={} wei",
            expected_wei,
            actual_wei
        );

        // EXACT amount
        if actual_wei != expected_wei {
            eprintln!(
                "[EVM Security] Native amount mismatch"
            );

            return Ok(false);
        }

        println!(
            "[EVM] ✅ Native transaction verified"
        );

        return Ok(true);
    }

    // --------------------------------------------------
    // 8. ERC20 USDT / USDC
    // --------------------------------------------------

    if currency != "USDT" && currency != "USDC" {
        eprintln!(
            "[EVM] Unsupported currency: {}",
            currency
        );

        return Ok(false);
    }

    // --------------------------------------------------
    // Expected token decimals
    // --------------------------------------------------

    let expected_units =
        (req.amount_paid * 1_000_000.0)
            .round() as u128;

    // --------------------------------------------------
    // Search Transfer event
    // --------------------------------------------------

    for log in &receipt.logs {

        // Transfer event
        if log.topics.len() != 3 {
            continue;
        }

        if !log.topics[0]
            .eq_ignore_ascii_case(
                ERC20_TRANSFER_TOPIC
            )
        {
            continue;
        }

        // --------------------------------------------------
        // topic[1] = from address
        // topic[2] = to address
        // --------------------------------------------------

        let log_from =
            format!(
                "0x{}",
                &log.topics[1]
                    .trim_start_matches("0x")
                    .chars()
                    .rev()
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );

        // Better address extraction:
        let from_topic =
            log.topics[1]
                .trim_start_matches("0x");

        let to_topic =
            log.topics[2]
                .trim_start_matches("0x");

        if from_topic.len() < 40
            || to_topic.len() < 40
        {
            continue;
        }

        let actual_from =
            format!(
                "0x{}",
                &from_topic[from_topic.len() - 40..]
            );

        let actual_to =
            format!(
                "0x{}",
                &to_topic[to_topic.len() - 40..]
            );

        // --------------------------------------------------
        // Verify sender
        // --------------------------------------------------

        if !actual_from.eq_ignore_ascii_case(
            &req.sender_address
        ) {
            continue;
        }

        // --------------------------------------------------
        // Verify receiver
        // --------------------------------------------------

        if !actual_to.eq_ignore_ascii_case(
            &req.receiver_address
        ) {
            continue;
        }

        // --------------------------------------------------
        // Decode amount
        // --------------------------------------------------

        let data =
            log.data.trim_start_matches("0x");

        if data.len() != 64 {
            continue;
        }

        let actual_amount =
            u128::from_str_radix(data, 16)
                .unwrap_or(0);

        println!(
            "[EVM ERC20] Token={} From={} To={} Amount={}",
            log.address,
            actual_from,
            actual_to,
            actual_amount
        );

        // --------------------------------------------------
        // EXACT amount
        // --------------------------------------------------

        if actual_amount != expected_units {
            eprintln!(
                "[EVM ERC20] Amount mismatch. Expected={} actual={}",
                expected_units,
                actual_amount
            );

            continue;
        }

        // --------------------------------------------------
        // Verify correct token contract
        // --------------------------------------------------

        let expected_contract =
            match (
                req.network.to_lowercase().as_str(),
                currency.as_str(),
            ) {

                ("ethereum", "USDT") =>
                    env::var("ETHEREUM_USDT_CONTRACT")
                        .unwrap_or_default(),

                ("ethereum", "USDC") =>
                    env::var("ETHEREUM_USDC_CONTRACT")
                        .unwrap_or_default(),

                ("polygon", "USDT") =>
                    env::var("POLYGON_USDT_CONTRACT")
                        .unwrap_or_default(),

                ("polygon", "USDC") =>
                    env::var("POLYGON_USDC_CONTRACT")
                        .unwrap_or_default(),

                _ => String::new(),
            };

        if !expected_contract.is_empty()
            && !log.address.eq_ignore_ascii_case(
                &expected_contract
            )
        {
            continue;
        }

        println!(
            "[EVM] ✅ ERC20 {} transaction verified",
            currency
        );

        return Ok(true);
    }

    eprintln!(
        "[EVM Security] No matching {} Transfer event found",
        currency
    );

    Ok(false)
}
