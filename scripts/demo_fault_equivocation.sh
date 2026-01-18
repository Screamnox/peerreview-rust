#!/usr/bin/env bash
set -euo pipefail

echo "[1] ensure node10 has fault flag in compose"
grep -n "node10" -A6 docker/docker-compose.yml | sed -n '107,114p'

if ! grep -q 'equivocate_head' docker/docker-compose.yml; then
  echo "[patch] enable equivocation fault for node10"
  sed -i 's#command: \["--name","node10","--cluster","configs/docker/cluster.yaml","--log-dir","peerreview_logs","--http"\]#command: ["--name","node10","--cluster","configs/docker/cluster.yaml","--log-dir","peerreview_logs","--http","--fault","equivocate_head"]#' docker/docker-compose.yml
fi

echo "[2] recreate node10"
docker compose -f docker/docker-compose.yml up -d --build --force-recreate node10

echo "[3] trigger event + wait commitments"
curl -s -X POST http://localhost:8090/publish_text -H 'Content-Type: application/json' -d '{"text":"trigger equivocation"}' | jq
sleep 4

echo "[4] audit suspect node10 (should be FAULT equivocation)"
docker run --rm --network docker_gossip_net \
  -v "$PWD:/w" -w /w \
  debian:bookworm-slim \
  ./target/debug/pr_audit_live --cluster configs/docker/cluster.yaml --suspect node10

echo "[5] witness proof (node1 vs node2 latest commits)"
curl -s -X POST http://localhost:8081/pr/ask_witness -H 'Content-Type: application/json' \
  -d '{"node_name":"node10","witness_name":"node1"}' | jq '.count, .commits[-3:]'

curl -s -X POST http://localhost:8082/pr/ask_witness -H 'Content-Type: application/json' \
  -d '{"node_name":"node10","witness_name":"node2"}' | jq '.count, .commits[-3:]'
