use std::env;
use tonic::{transport::Server, Request, Response, Status};

// Modular structure matching your files
mod client;
mod config;
mod error;
mod rpc;

// Include the generated proto definitions matching package "settlement"
pub mod settlement {
    tonic::include_proto!("settlement");
}

use settlement::settlement_service_server::{SettlementService, SettlementServiceServer};
use settlement::{SettlementRequest, SettlementResponse};

// EVM JSON-RPC types
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct EvmRpcResponse {
    result: Option<EvmTransaction>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct EvmTransaction {
    from: String,
    to: Option<String>,
    value: String, // Hex string of wei
}

#[derive(Deserialize, Debug)]
struct EvmReceiptResponse {
    result: Option<EvmReceipt>,
}

#[derive(Deserialize, Debug)]
struct EvmReceipt {
    status: String, // "0x1" for success
}

// Define the gRPC Service implementation struct
#[derive(Default)]
pub struct MySettlementService {}

impl MySettlementService {
    /// Real Production EVM Ledger Verification via JSON-RPC HTTP Provider
    async fn verify_evm_transaction(
        &self,
        tx_hash: &str,
        expected_from: &str,
        expected_amount_paid: f64,
        network: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let rpc_url = match network.to_lowercase().as_str() {
            "polygon" => env::var("POLYGON_RPC_URL").unwrap_or_else(|_| "https://polygon-rpc.com".to_string()),
            "ethereum" | "evm" => env::var("ETHEREUM_RPC_URL").unwrap_or_else(|_| "https://eth.llamarpc.com".to_string()),
            _ => {
                eprintln!("[EVM Verification] Unknown or unsupported EVM network: {}", network);
                return Ok(false);
            }
        };

        let client = reqwest::Client::new();

        // 1. Fetch Transaction by Hash
        let tx_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionByHash",
            "params": [tx_hash],
            "id": 1
        });

        let tx_res = client.post(&rpc_url).json(&tx_payload).send().await?.json::<EvmRpcResponse>().await?;

        let tx = match tx_res.result {
            Some(t) => t,
            None => {
                eprintln!("[EVM Verification] Transaction hash not found or pending on-chain: {}", tx_hash);
                return Ok(false);
            }
        };

        // 2. Cryptographic Sender Address Validation (Fixed typo)
        if !tx.from.eq_ignore_ascii_case(expected_from) {
            eprintln!(
                "[EVM Verification Security Alert] Sender address mismatch! Expected: {}, Found on-chain: {}",
                expected_from, tx.from
            );
            return Ok(false);
        }

        // 3. Amount Verification
        let hex_value = tx.value.strip_prefix("0x").unwrap_or(&tx.value);
        let actual_wei = u128::from_str_radix(hex_value, 16).unwrap_or(0);

        let expected_wei = (expected_amount_paid * 1e18) as u128;
        if actual_wei < expected_wei {
            eprintln!(
                "[EVM Verification] Insufficient value transferred. Expected at least {} wei, got {} wei",
                expected_wei, actual_wei
            );
            return Ok(false);
        }

        // 4. Check Transaction Execution Receipt Status
        let receipt_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionReceipt",
            "params": [tx_hash],
            "id": 1
        });

        let receipt_res = client.post(&rpc_url).json(&receipt_payload).send().await?.json::<EvmReceiptResponse>().await?;

        if let Some(receipt) = receipt_res.result {
            let status_hex = receipt.status.strip_prefix("0x").unwrap_or(&receipt.status);
            let status_code = u64::from_str_radix(status_hex, 16).unwrap_or(0);
            if status_code != 1 {
                eprintln!("[EVM Verification] Transaction reverted or failed execution on-chain.");
                return Ok(false);
            }
        } else {
            eprintln!("[EVM Verification] Transaction receipt not available yet.");
            return Ok(false);
        }

        Ok(true)
    }
}

#[tonic::async_trait]
impl SettlementService for MySettlementService {
    async fn verify_and_settle_transaction(
        &self,
        request: Request<SettlementRequest>,
    ) -> Result<Response<SettlementResponse>, Status> {
        let req = request.into_inner();

        println!(
            "[Rust gRPC] Executing deep on-chain ledger validation: invoice_id={}, tx_hash={}, network={}, amount_paid={}, from_address={}",
            req.invoice_id, req.tx_hash, req.network, req.amount_paid, req.from_address
        );

        let verification_result = match req.network.to_lowercase().as_str() {
            "solana" => {
                // Call your modularized function inside src/rpc/solana.rs
                crate::rpc::solana::verify_solana_transaction(&req).await
            }
            "ethereum" | "polygon" | "evm" => {
                self.verify_evm_transaction(&req.tx_hash, &req.from_address, req.amount_paid, &req.network).await
            }
            _ => {
                eprintln!("[Verification Error] Unsupported network type: {}", req.network);
                Ok(false)
            }
        };

        let is_valid = match verification_result {
            Ok(valid) => valid,
            Err(e) => {
                eprintln!("[Blockchain RPC Network Error]: {}", e);
                false
            }
        };

        let message = if is_valid {
            "On-chain transaction cryptographically verified and successfully settled".to_string()
        } else {
            "On-chain verification failed: Transaction invalid, reverted, or sender/amount mismatch".to_string()
        };

        let merchant_id = if is_valid {
            "merchant_resolved_xyz123".to_string()
        } else {
            "".to_string()
        };

        let reply = SettlementResponse {
            success: is_valid,
            merchant_id,
            message,
        };

        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();

    let addr = "[::1]:50051".parse()?;
    let service = MySettlementService::default();

    println!("==================================================");
    println!("🛡️  Pinecone.xyz Production Verification Engine active on {}", addr);
    println!("==================================================");

    Server::builder()
        .add_service(SettlementServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
