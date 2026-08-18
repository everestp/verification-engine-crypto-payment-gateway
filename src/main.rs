use std::collections::HashMap;
use std::env;
use std::sync::Arc;

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

// ============================================================
// NETWORK VERIFIER TRAIT
// ------------------------------------------------------------
// This is the ONLY contract a blockchain integration needs to
// satisfy to be plugged into the settlement engine.
//
// 👉 TO ADD A NEW NETWORK (e.g. Bitcoin, Tron, Base, Arbitrum):
//   1. Create a new module under `rpc/` (e.g. rpc/tron.rs) with
//      your verification logic — mirror rpc/solana.rs or
//      rpc/polygon.rs (EVM) as a template.
//   2. Create a small struct here that implements
//      `NetworkVerifier` and calls into that module.
//   3. Register it in `build_verifier_registry()` below with
//      the network name(s) it should respond to.
//   4. That's it — no other file in this service needs to
//      change. The gRPC handler is 100% network-agnostic.
// ============================================================

/// Any blockchain integration (Solana, EVM chains, future chains)
/// implements this trait. It's the single seam between the
/// gRPC layer and chain-specific verification logic.
#[tonic::async_trait]
pub trait NetworkVerifier: Send + Sync {
    /// Verify that `req` corresponds to a real, finalized,
    /// successful on-chain transfer of the exact expected
    /// amount from sender -> receiver.
    ///
    /// Returns:
    ///   Ok(true)  -> verified, safe to settle
    ///   Ok(false) -> verification ran, but transaction is
    ///                invalid/mismatched (reason already logged
    ///                by the verifier itself)
    ///   Err(_)    -> verification could NOT be completed
    ///                (RPC down, network error, etc.) — treated
    ///                as "not settled" but logged separately so
    ///                it can be distinguished from a genuine
    ///                on-chain mismatch/failure.
    async fn verify(
        &self,
        req: &SettlementRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;
}

// --------------------------------------------------
// Solana adapter
// --------------------------------------------------
// Thin wrapper so `rpc::solana::verify_solana_transaction`
// can live behind the `NetworkVerifier` trait.
struct SolanaVerifier;

#[tonic::async_trait]
impl NetworkVerifier for SolanaVerifier {
    async fn verify(
        &self,
        req: &SettlementRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        crate::rpc::solana::verify_solana_transaction(req).await
    }
}

// --------------------------------------------------
// EVM adapter (covers Ethereum, Polygon, and any future
// EVM-compatible chain — the underlying function already
// branches on `req.network` to pick the right RPC/contracts)
// --------------------------------------------------
struct EvmVerifier;

#[tonic::async_trait]
impl NetworkVerifier for EvmVerifier {
    async fn verify(
        &self,
        req: &SettlementRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        crate::rpc::polygon::verify_evm_transaction(req).await
    }
}

/// Maps a lowercase network name (as sent in `SettlementRequest.network`)
/// to the verifier responsible for it.
type VerifierRegistry = HashMap<&'static str, Arc<dyn NetworkVerifier>>;

/// Builds the network -> verifier map once at startup.
///
/// 👉 Adding a new network is a ONE-LINE change here:
///     registry.insert("tron", tron_verifier.clone());
fn build_verifier_registry() -> VerifierRegistry {
    let mut registry: VerifierRegistry = HashMap::new();

    // Solana (native SOL + SPL tokens, handled inside the verifier)
    let solana_verifier: Arc<dyn NetworkVerifier> = Arc::new(SolanaVerifier);
    registry.insert("solana", solana_verifier.clone());

    // EVM family — same verifier, multiple network aliases.
    // "evm" is kept as a generic alias some callers may use.
    let evm_verifier: Arc<dyn NetworkVerifier> = Arc::new(EvmVerifier);
    registry.insert("ethereum", evm_verifier.clone());
    registry.insert("polygon", evm_verifier.clone());
    registry.insert("evm", evm_verifier.clone());

    // --------------------------------------------------
    // 🔮 FUTURE NETWORKS GO HERE, e.g.:
    //
    // let tron_verifier: Arc<dyn NetworkVerifier> = Arc::new(TronVerifier);
    // registry.insert("tron", tron_verifier);
    //
    // let bitcoin_verifier: Arc<dyn NetworkVerifier> = Arc::new(BitcoinVerifier);
    // registry.insert("bitcoin", bitcoin_verifier);
    // --------------------------------------------------

    registry
}

// ============================================================
// gRPC SERVICE
// ============================================================

pub struct MySettlementService {
    /// Network name -> verifier lookup table, built once at
    /// startup and shared (cheaply, via Arc) across requests.
    verifiers: VerifierRegistry,
}

impl Default for MySettlementService {
    fn default() -> Self {
        Self {
            verifiers: build_verifier_registry(),
        }
    }
}

impl MySettlementService {
    /// Small helper so every basic-validation failure doesn't
    /// need to hand-build a full `SettlementResponse` struct.
    /// Keeps the handler body readable and DRY.
    fn validation_error(req: &SettlementRequest, message: &str) -> SettlementResponse {
        SettlementResponse {
            success: false,
            merchant_id: String::new(),
            receiver_address: req.receiver_address.clone(),
            sender_address: req.sender_address.clone(),
            block_number: req.block_number as f64,
            message: message.to_string(),
        }
    }

    /// Runs the basic, chain-agnostic sanity checks on the
    /// incoming request. Returns Some(error_message) on the
    /// first failed check, or None if everything looks sane.
    fn validate_request(req: &SettlementRequest) -> Option<&'static str> {
        if req.invoice_id.trim().is_empty() {
            return Some("Invoice ID is required");
        }
        if req.tx_hash.trim().is_empty() {
            return Some("Transaction hash is required");
        }
        if req.sender_address.trim().is_empty() {
            return Some("Sender address is required");
        }
        if req.receiver_address.trim().is_empty() {
            return Some("Receiver address is required");
        }
        if req.amount_paid <= 0.0 {
            return Some("Amount must be greater than zero");
        }
        None
    }
}

#[tonic::async_trait]
impl SettlementService for MySettlementService {
    async fn verify_and_settle_transaction(
        &self,
        request: Request<SettlementRequest>,
    ) -> Result<Response<SettlementResponse>, Status> {
        let req = request.into_inner();

        println!("==================================================");
        println!("[Rust gRPC] Settlement verification started");
        println!("[Settlement] Invoice ID : {}", req.invoice_id);
        println!("[Settlement] TX Hash    : {}", req.tx_hash);
        println!("[Settlement] Network    : {}", req.network);
        println!("[Settlement] Currency   : {}", req.currency);
        println!("[Settlement] Amount     : {}", req.amount_paid);
        println!("[Settlement] Sender     : {}", req.sender_address);
        println!("[Settlement] Receiver   : {}", req.receiver_address);
        println!("[Settlement] Block      : {}", req.block_number);

        // --------------------------------------------------
        // 1. Basic, chain-agnostic request validation
        // --------------------------------------------------

        if let Some(error_message) = Self::validate_request(&req) {
            println!("[Settlement] Rejected: {}", error_message);
            return Ok(Response::new(Self::validation_error(&req, error_message)));
        }

        // --------------------------------------------------
        // 2. Look up the right verifier for this network
        // --------------------------------------------------
        //
        // This is the entire "routing" step. No per-network
        // if/else here — just a map lookup. New networks never
        // touch this function.

        let network_key = req.network.to_lowercase();

        let verification_result = match self.verifiers.get(network_key.as_str()) {
            Some(verifier) => verifier.verify(&req).await,

            None => {
                eprintln!(
                    "[Settlement] Unsupported network: {}",
                    req.network
                );

                Err(format!("Unsupported blockchain network: {}", req.network).into())
            }
        };

        // --------------------------------------------------
        // 3. Interpret the verification result
        // --------------------------------------------------

        let is_valid = match verification_result {
            Ok(valid) => valid,

            Err(error) => {
                // NOTE: this branch means verification could not
                // even run (RPC down, bad response, unsupported
                // network, etc.) — distinct from a verifier
                // returning Ok(false) for a genuine on-chain
                // mismatch, which it already logs itself.
                eprintln!(
                    "[Settlement] Blockchain verification error: {}",
                    error
                );

                false
            }
        };

        // --------------------------------------------------
        // 4. Build the settlement response
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

        println!("[Settlement] Verification result: {}", is_valid);
        println!("==================================================");

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
    //
    // IMPORTANT:
    // Use 0.0.0.0 inside Docker so the gRPC server
    // accepts connections from outside the container.
    //
    // You can override these with:
    // SETTLEMENT_GRPC_HOST
    // SETTLEMENT_GRPC_PORT
    //
    let host = env::var("SETTLEMENT_GRPC_HOST")
        .unwrap_or_else(|_| "0.0.0.0".to_string());

    let port = env::var("SETTLEMENT_GRPC_PORT")
        .unwrap_or_else(|_| "50051".to_string());

    let addr = format!("{}:{}", host, port).parse()?;

    // --------------------------------------------------
    // Service
    // --------------------------------------------------

    let service = MySettlementService::default();

    // Print the networks we actually have verifiers for
    let mut supported_networks: Vec<&str> =
        service.verifiers.keys().copied().collect();

    supported_networks.sort();

    println!("==================================================");
    println!("🛡️ Pinecone.xyz Settlement Verification Engine");
    println!("🚀 gRPC Server: {}", addr);
    println!(
        "⛓️ Supported Networks: {}",
        supported_networks.join(", ")
    );
    println!("💰 Supported Types: Native + ERC20/SPL verification");
    println!("==================================================");

    // --------------------------------------------------
    // Start server
    // --------------------------------------------------

    Server::builder()
        .add_service(SettlementServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
