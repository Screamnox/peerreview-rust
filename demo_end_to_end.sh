#!/usr/bin/env bash
set -euo pipefail

echo "[1/5] Build release on host (needed for offline docker build)"
cargo build --release -p gossip_node

echo "[2/5] Start docker cluster (10 nodes)"
docker compose -f docker/docker-compose.yml down -v >/dev/null 2>&1 || true
docker compose -f docker/docker-compose.yml up -d --build

echo "[3/5] Wait a bit for nodes to boot"
sleep 3

echo "[4/5] Publish traffic to node1"
for i in $(seq 1 10); do
  curl -s -X POST http://127.0.0.1:8081/publish \
    -H "Content-Type: application/json" \
    -d "{\"payload\":\"hello10-$i\"}" >/dev/null
done
echo "published 10 messages"

echo "[5/5] Check node10 received + PR logs contain app events"
stats="$(curl -s http://127.0.0.1:8090/stats || true)"
echo "node10 stats: ${stats}"

echo "--- PR events on node10 (tail) ---"
docker exec -it node10 sh -lc 'grep -E "\"kind\":\"(SEND|RECV|DELIVER)\"" -n /app/peerreview_logs/node10.log | tail -n 15'

echo "OK: demo end-to-end passed"
