fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)  // <-- Ensures VerifierServiceServer is generated
        .build_client(true)
        .compile(&["proto/settlement.proto"], &["proto"])?; // Adjust path to your .proto file
    Ok(())
}
