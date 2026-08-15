use std::fmt;

#[derive(Debug)]
pub enum EngineError {
    GrpcError(tonic::Status),
    TransportError(tonic::transport::Error),
    ConfigError(String),
    RpcError(String),
}

impl std::error::Error for EngineError {}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::GrpcError(e) => write!(f, "gRPC status error: {}", e),
            EngineError::TransportError(e) => write!(f, "Transport error: {}", e),
            EngineError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            EngineError::RpcError(msg) => write!(f, "RPC error: {}", msg),
        }
    }
}
