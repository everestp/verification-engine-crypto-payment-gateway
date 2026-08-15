pub mod solana;
pub mod polygon;

#[derive(Debug, Clone)]
pub struct VerifiedTxEvent {
    pub invoice_id: String,
    pub tx_hash: String,
    pub network: String,
    pub amount_paid: f64,
    pub currency: String,
    pub from_address: String,
    pub block_number: i64,
}
