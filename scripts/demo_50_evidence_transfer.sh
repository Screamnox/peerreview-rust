#!/usr/bin/env bash
set -euo pipefail

CLUSTER="configs/docker/cluster.yaml"
SUSPECT="${1:-node10}"

echo "[1] Run live audit: suspect=$SUSPECT"
OUT=$(docker run --rm --network docker_gossip_net -v "$PWD:/w" -w /w \
  debian:bookworm-slim \
  ./target/debug/pr_audit_live --cluster "$CLUSTER" --suspect "$SUSPECT")

echo "$OUT"
echo

if echo "$OUT" | grep -q "VERDICT: FAULT"; then
  echo "[2] FAULT detected -> EvidenceTransfer to all nodes"

  TS=$(date +%s%3N)

  # We transfer the raw verdict text as payload (minimal, portable evidence)
  PAYLOAD=$(printf "%s" "$OUT" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')

  for p in 8081 8082 8083 8084 8085 8086 8087 8088 8089 8090; do
    curl -s -X POST "http://localhost:$p/pr/evidence_transfer" \
      -H "Content-Type: application/json" \
      -d "{\"from\":\"auditor\",\"about\":\"$SUSPECT\",\"kind\":\"FAULT_VERDICT\",\"ts_ms\":$TS,\"payload\":$PAYLOAD}" >/dev/null || true
  done

  echo "[3] Evidence stored. Showing node1 evidence list:"
  curl -s http://localhost:8081/pr/evidence | jq '.[-3:]'
else
  echo "[2] Verdict OK -> No evidence transfer"
fi
