#!/usr/bin/env bash
set -euo pipefail

echo "[1] build"
cargo test -p peerreview_protocol
cargo build --release -p gossip_node
cargo build -p peerreview_protocol --bin pr_audit_live

echo "[2] start cluster"
docker compose -f docker/docker-compose.yml up --build -d

echo "[3] wait a bit and show witnesses stats"
sleep 3
for p in 8081 8082 8083; do
  echo "== witness $p ==";
  curl -s "http://localhost:$p/stats" | jq '.me,.is_witness,.pr_commit_recv'
done

echo "[4] generate activity"
curl -s -X POST http://localhost:8084/publish_text -H 'Content-Type: application/json' -d '{"text":"demo ok node4"}' | jq
curl -s -X POST http://localhost:8090/publish_text -H 'Content-Type: application/json' -d '{"text":"demo ok node10"}' | jq
sleep 3

echo "[5] audit suspect node10 (should be OK)"
docker run --rm --network docker_gossip_net \
  -v "$PWD:/w" -w /w \
  debian:bookworm-slim \
  ./target/debug/pr_audit_live --cluster configs/docker/cluster.yaml --suspect node10
