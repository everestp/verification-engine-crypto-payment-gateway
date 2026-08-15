use super::VerifiedTxEvent;
use tokio::time::{sleep, Duration};

pub async fn listen_solana_blocks<F>(mut on_verified: F)
where
    F: FnMut(VerifiedTxEvent) + Send,
{
    println!("[Solana Listener] Connected to Solana RPC WebSocket/Poller...");
    loop {
        sleep(Duration::from_secs(12)).await;

        let event = VerifiedTxEvent {
            invoice_id: "inv_sol_mock_001".to_string(),
            tx_hash: "5V1xSolanaTxHashSignatureMock123456789".to_string(),
            network: "solana".to_string(),
            amount_paid: 2.5,
            currency: "SOL".to_string(),
            from_address: "7V1xA2N9f3Kv8zQyFMZd4FvEqcYdYE7gSZWxrEBRfBsB".to_string(),
            block_number: 254891230,
        };

        on_verified(event);
    }
}
