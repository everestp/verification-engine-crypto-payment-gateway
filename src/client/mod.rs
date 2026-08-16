pub mod settlement {
    tonic::include_proto!("settlement");
}

use settlement::settlement_service_client::SettlementServiceClient;
use settlement::SettlementRequest;
use tonic::transport::Channel;

pub struct GrpcClient {
    client: SettlementServiceClient<Channel>,
}

impl GrpcClient {
    pub async fn connect(
        endpoint: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let client =
            SettlementServiceClient::connect(endpoint).await?;

        Ok(Self { client })
    }

    pub async fn submit_settlement(
        &mut self,
        invoice_id: String,
        tx_hash: String,
        network: String,
        amount_paid: f64,
        currency: String,
        sender_address: String,
        receiver_address: String,
        block_number: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {

        let request = tonic::Request::new(
            SettlementRequest {
                invoice_id,
                tx_hash,
                network,
                amount_paid,
                currency,
                sender_address,
                receiver_address,
                block_number,
            }
        );

        let response = self
            .client
            .verify_and_settle_transaction(request)
            .await?;

        let inner = response.into_inner();

        if inner.success {
            println!(
                "[gRPC] ✅ Settlement successful!"
            );

            println!(
                "[gRPC] Merchant ID: {}",
                inner.merchant_id
            );

            println!(
                "[gRPC] Sender: {}",
                inner.sender_address
            );

            println!(
                "[gRPC] Receiver: {}",
                inner.receiver_address
            );

            println!(
                "[gRPC] Block: {}",
                inner.block_number
            );

            println!(
                "[gRPC] Message: {}",
                inner.message
            );
        } else {
            eprintln!(
                "[gRPC] ❌ Settlement rejected: {}",
                inner.message
            );
        }

        Ok(())
    }
}
