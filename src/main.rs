mod config;
mod error;
mod client;
mod rpc;

use config::Config;
use client::GrpcClient;
use rpc::solana::listen_solana_blocks;
use rpc::polygon::listen_polygon_blocks;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Pinecone.xyz Rust Verification Engine...");

    let config = Config::from_env().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Connect gRPC client to Go Backend server
    println!("Connecting to Go gRPC Backend at {}...", config.backend_grpc_url);
    let mut grpc_client = GrpcClient::connect(config.backend_grpc_url.clone()).await?;

    // Spawn Solana listener task
    tokio::spawn(async move {
        listen_solana_blocks(move |event| {
            println!("[Event] Solana Tx Detected: {:?}", event);
            // Example: Dispatch via grpc_client in production with Arc<Mutex<GrpcClient>>
        }).await;
    });

    // Spawn Polygon listener task
    tokio::spawn(async move {
        listen_polygon_blocks(move |event| {
            println!("[Event] Polygon Tx Detected: {:?}", event);
        }).await;
    });

    // Keep the verification engine running
    tokio::signal::ctrl_c().await?;
    println!("Shutting down Pinecone.xyz Verification Engine gracefully.");

    Ok(())
}
