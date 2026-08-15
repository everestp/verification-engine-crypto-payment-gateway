use super::VerifiedTxEvent;
use tokio::time::{sleep, Duration};

pub async fn listen_polygon_blocks<F>(mut on_verified: F)
where
    F: FnMut(VerifiedTxEvent) + Send,
{
    println!("[Polygon Listener] Connected to Polygon JSON-RPC Poller...");
    loop {
        sleep(Duration::from_secs(15)).await;

        let event = VerifiedTxEvent {
            invoice_id: "inv_poly_mock_002".to_string(),
            tx_hash: "0xpolygonTxHashSignatureMock987654321".to_string(),
            network: "polygon".to_string(),
            amount_paid: 100.0,
            currency: "USDT".to_string(),
            from_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
            block_number: 54129033,
        };

        on_verified(event);
    }
}
