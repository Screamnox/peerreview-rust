# docker/Dockerfile
FROM debian:bookworm-slim

WORKDIR /app

# Copy prebuilt binary from host build
COPY target/release/gossip_node /app/gossip_node

# Copy configs (docker + local)
COPY configs /app/configs

RUN mkdir -p /app/peerreview_logs

ENTRYPOINT ["/app/gossip_node"]
