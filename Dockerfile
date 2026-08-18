# ============================================================
# BUILD STAGE
# ============================================================
FROM rust:1.89-bookworm AS builder

WORKDIR /app

# Install protobuf compiler required by prost/tonic
RUN apt-get update && \
    apt-get install -y --no-install-recommends protobuf-compiler && \
    rm -rf /var/lib/apt/lists/*

# Copy dependency files
COPY Cargo.toml Cargo.lock ./

# Copy protobuf definitions
COPY proto ./proto

# Copy build script
COPY build.rs ./

# Copy source
COPY src ./src

# Build production binary
RUN cargo build --release


# ============================================================
# RUNTIME STAGE
# ============================================================
FROM debian:bookworm-slim

WORKDIR /app

# Runtime CA certificates
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy compiled binary
COPY --from=builder /app/target/release/verification_engine /app/server

# gRPC port
EXPOSE 50051

# Start gRPC server
CMD ["/app/server"]
