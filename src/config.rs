use std::env;

pub struct Config {
    pub backend_grpc_url: String,
    pub solana_rpc_url: String,
    pub polygon_rpc_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        dotenvy::dotenv().ok();
        let backend_grpc_url = env::var("BACKEND_GRPC_URL")
            .unwrap_or_else(|_| "http://[::1]:50051".to_string());
        let solana_rpc_url = env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://devnet.helius-rpc.com/?api-key=bbb113c7-30a5-4ffc-8325-70b6efa6115e".to_string());
        let polygon_rpc_url = env::var("POLYGON_RPC_URL")
            .unwrap_or_else(|_| "https://polygon-rpc.com".to_string());

        Ok(Self {
            backend_grpc_url,
            solana_rpc_url,
            polygon_rpc_url,
        })
    }
}
