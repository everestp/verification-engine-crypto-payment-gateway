use tonic::{transport::Server, Request, Response, Status};

// Modular structure placeholders
mod config;
mod error;
mod client;
mod rpc;

// Include the generated proto definitions matching package "settlement"
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
            "[Rust gRPC] Processing settlement: invoice_id={}, tx_hash={}, network={}, amount_paid={}, currency={}, from_address={}, block_number={}",
            req.invoice_id, req.tx_hash, req.network, req.amount_paid, req.currency, req.from_address, req.block_number
        );

        // TODO: Plug in Solana and EVM cryptographic signature verification logic using your rpc/ modules
        let is_valid = match req.network.to_lowercase().as_str() {
            "solana" => {
                // Placeholder: Add Solana cryptographic check here
                true
            }
            "ethereum" | "polygon" | "evm" => {
                // Placeholder: Add EVM cryptographic check here
                true
            }
            _ => {
                eprintln!("[Verification Error] Unsupported network type: {}", req.network);
                false
            }
        };

        let message = if is_valid {
            "Transaction verified and settled successfully".to_string()
        } else {
            "Cryptographic signature verification or transaction validation failed".to_string()
        };

        // Resolve merchant ID dynamically or fallback to a default/lookup value
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
    let addr = "[::1]:50051".parse()?;
    let service = MySettlementService::default();

    println!("==================================================");
    println!("🛡️  Pinecone.xyz Rust Settlement Engine running on {}", addr);
    println!("==================================================");

    Server::builder()
        .add_service(SettlementServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
