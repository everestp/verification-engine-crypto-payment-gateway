use std::env;

use tonic::{
    transport::Server,
    Request,
    Response,
    Status,
};

// Modular structure
mod client;
mod config;
mod error;
mod rpc;

// Generated protobuf
pub mod settlement {
    tonic::include_proto!("settlement");
}

use settlement::{
    settlement_service_server::{
        SettlementService,
        SettlementServiceServer,
    },
    SettlementRequest,
    SettlementResponse,
};

#[derive(Default)]
pub struct MySettlementService {}

#[tonic::async_trait]
impl SettlementService for MySettlementService {
    async fn verify_and_settle_transaction(
        &self,
        request: Request<SettlementRequest>,
    ) -> Result<Response<SettlementResponse>, Status> {
        let req = request.into_inner();

        println!(
            "=================================================="
        );

        println!(
            "[Rust gRPC] Settlement verification started"
        );

        println!(
            "[Settlement] Invoice ID : {}",
            req.invoice_id
        );

        println!(
            "[Settlement] TX Hash    : {}",
            req.tx_hash
        );

        println!(
            "[Settlement] Network    : {}",
            req.network
        );

        println!(
            "[Settlement] Currency   : {}",
            req.currency
        );

        println!(
            "[Settlement] Amount     : {}",
            req.amount_paid
        );

        println!(
            "[Settlement] Sender     : {}",
            req.sender_address
        );

        println!(
            "[Settlement] Receiver   : {}",
            req.receiver_address
        );

        println!(
            "[Settlement] Block      : {}",
            req.block_number
        );

        // --------------------------------------------------
        // Basic request validation
        // --------------------------------------------------

        if req.invoice_id.trim().is_empty() {
            return Ok(Response::new(SettlementResponse {
                success: false,
                merchant_id: String::new(),
                receiver_address: req.receiver_address,
                sender_address: req.sender_address,
                block_number: req.block_number as f64,
                message: "Invoice ID is required".to_string(),
            }));
        }

        if req.tx_hash.trim().is_empty() {
            return Ok(Response::new(SettlementResponse {
                success: false,
                merchant_id: String::new(),
                receiver_address: req.receiver_address,
                sender_address: req.sender_address,
                block_number: req.block_number as f64,
                message: "Transaction hash is required".to_string(),
            }));
        }

        if req.sender_address.trim().is_empty() {
            return Ok(Response::new(SettlementResponse {
                success: false,
                merchant_id: String::new(),
                receiver_address: req.receiver_address,
                sender_address: req.sender_address,
                block_number: req.block_number as f64,
                message: "Sender address is required".to_string(),
            }));
        }

        if req.receiver_address.trim().is_empty() {
            return Ok(Response::new(SettlementResponse {
                success: false,
                merchant_id: String::new(),
                receiver_address: req.receiver_address,
                sender_address: req.sender_address,
                block_number: req.block_number as f64,
                message: "Receiver address is required".to_string(),
            }));
        }

        if req.amount_paid <= 0.0 {
            return Ok(Response::new(SettlementResponse {
                success: false,
                merchant_id: String::new(),
                receiver_address: req.receiver_address,
                sender_address: req.sender_address,
                block_number: req.block_number as f64,
                message: "Amount must be greater than zero".to_string(),
            }));
        }

        // --------------------------------------------------
        // Blockchain verification
        // --------------------------------------------------

        let verification_result =
            match req.network.to_lowercase().as_str() {

                // --------------------------------------------------
                // Solana
                // --------------------------------------------------

                "solana" => {
                    crate::rpc::solana
                        ::verify_solana_transaction(&req)
                        .await
                }

                // --------------------------------------------------
                // Ethereum / Polygon
                // --------------------------------------------------

                "ethereum" | "polygon"   => {
                    crate::rpc::polygon
                        ::verify_evm_transaction(&req)
                        .await
                }

                // --------------------------------------------------
                // Unsupported network
                // --------------------------------------------------

                network => {
                    eprintln!(
                        "[Settlement] Unsupported network: {}",
                        network
                    );

                    Err(
                        format!(
                            "Unsupported blockchain network: {}",
                            network
                        )
                        .into()
                    )
                }
            };

        // --------------------------------------------------
        // Verification result
        // --------------------------------------------------

        let is_valid = match verification_result {
            Ok(valid) => valid,

            Err(error) => {
                eprintln!(
                    "[Settlement] Blockchain verification error: {}",
                    error
                );

                false
            }
        };

        // --------------------------------------------------
        // Settlement response
        // --------------------------------------------------

        let message = if is_valid {
            format!(
                "Transaction verified successfully. {} {} received by the expected receiver.",
                req.amount_paid,
                req.currency
            )
        } else {
            "Transaction verification failed: transaction invalid, failed, sender/receiver mismatch, or exact amount mismatch."
                .to_string()
        };

        // TODO:
        // Resolve this from your database/service instead
        // of hardcoding it.
        let merchant_id = if is_valid {
            "merchant_resolved_xyz123".to_string()
        } else {
            String::new()
        };

        let reply = SettlementResponse {
            success: is_valid,

            merchant_id,

            receiver_address: req.receiver_address,

            sender_address: req.sender_address,

            block_number: req.block_number as f64,

            message,
        };

        println!(
            "[Settlement] Verification result: {}",
            is_valid
        );

        println!(
            "=================================================="
        );

        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env
    dotenvy::dotenv().ok();

    // --------------------------------------------------
    // gRPC server address
    // --------------------------------------------------

    let host =
        env::var("SETTLEMENT_GRPC_HOST")
            .unwrap_or_else(|_| "[::1]".to_string());

    let port =
        env::var("SETTLEMENT_GRPC_PORT")
            .unwrap_or_else(|_| "50051".to_string());

    let addr =
        format!("{}:{}", host, port).parse()?;

    // --------------------------------------------------
    // Service
    // --------------------------------------------------

    let service =
        MySettlementService::default();

    println!(
        "=================================================="
    );

    println!(
        "🛡️ Pinecone.xyz Settlement Verification Engine"
    );

    println!(
        "🚀 gRPC Server: {}",
        addr
    );

    println!(
        "⛓️ Supported Networks: Solana, Ethereum, Polygon"
    );

    println!(
        "💰 Supported Types: Native + ERC20/SPL verification"
    );

    println!(
        "=================================================="
    );

    // --------------------------------------------------
    // Start server
    // --------------------------------------------------

    Server::builder()
        .add_service(
            SettlementServiceServer::new(service)
        )
        .serve(addr)
        .await?;

    Ok(())
}
