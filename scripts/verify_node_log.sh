#!/usr/bin/env bash
set -euo pipefail

NODE="${1:?usage: verify_node_log.sh <nodeName>}"
PUBKEY="${2:?usage: verify_node_log.sh <nodeName> <pubkeyHex32>}"

TMP="/tmp/${NODE}.log"
docker cp "${NODE}:/app/peerreview_logs/${NODE}.log" "${TMP}"

cargo run -p peerreview_protocol --bin pr_verify -- \
  --log "${TMP}" \
  --pubkey "${PUBKEY}" \
  --strict-chain

echo "OK: verified ${TMP}"
