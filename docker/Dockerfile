# --- Dockerfile for gossip_node ---
FROM rust:1.91 as builder

WORKDIR /usr/src/peerreview-rs
COPY . .

# Build only the gossip_node binary
RUN cargo build --release -p gossip_node

# --- Runtime image ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /usr/src/peerreview-rs/target/release/gossip_node /usr/local/bin/gossip_node
COPY configs /app/configs

EXPOSE 7001 7002 8081 8082

ENTRYPOINT ["gossip_node"]
