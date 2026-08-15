use tonic::{transport::Server, Request, Response, Status};

// Import your modules
mod config;
mod error;
mod client;
mod rpc;

// Include the generated proto definitions from settlement.proto
pub mod settlement {
    tonic::include_proto!("settlement");
}

use settlement::settlement_service_server::{SettlementService, SettlementServiceServer};
use settlement::{SettlementRequest, SettlementResponse};

// Define the gRPC Service implementation struct
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
            "[Rust gRPC] Processing settlement: invoice_id={}, tx_hash={}, network={}, amount={} {}",
            req.invoice_id, req.tx_hash, req.network, req.amount_paid, req.currency
        );

        // Dispatch verification based on network type using your rpc modules
        let is_valid = match req.network.to_lowercase().as_str() {
            "solana" => {
                match rpc::solana::verify_solana_transaction(&req) {
                    Ok(valid) => valid,
                    Err(e) => {
                        eprintln!("[Solana Verification Error]: {}", e);
                        false
                    }
                }
            }
            "polygon" | "ethereum" => {
                match rpc::polygon::verify_evm_transaction(&req) {
                    Ok(valid) => valid,
                    Err(e) => {
                        eprintln!("[EVM/Polygon Verification Error]: {}", e);
                        false
                    }
                }
            }
            _ => {
                eprintln!("[Verification Error] Unsupported network: {}", req.network);
                false
            }
        };

        let message = if is_valid {
            "Transaction verified and settled successfully".to_string()
        } else {
            "Cryptographic signature or transaction verification failed".to_string()
        };

        let reply = SettlementResponse {
            success: is_valid,
            merchant_id: "merchant_default_123".to_string(), // Adjust or populate based on your logic
            message,
        };

        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let service = MySettlementService::default();

    println!("==================================================");
    println!("🛡️  Pinecone.xyz Rust Verification Engine running on {}", addr);
    println!("==================================================");

    Server::builder()
        .add_service(SettlementServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
