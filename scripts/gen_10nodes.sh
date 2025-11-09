#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# nodes 1..10 (HTTP 8081..8090, TCP interne 7001)
for i in $(seq 1 10); do
  http=$((8080 + i))
  cat > "configs/docker/node${i}.yaml" <<YAML
id: "node${i}"
listen_addr: "0.0.0.0:7001"
heartbeat_ms: 1000
gossip_ms: 800
anti_entropy_ms: 3000
http_api: ${http}
YAML
done

# cluster avec fanout et multi-trees
cat > configs/docker/cluster.yaml <<YAML
nodes:
$(for i in $(seq 1 10); do echo "  - { id: \"node${i}\", addr: \"node${i}:7001\" }"; done)
fanout: 3
num_trees: 3
YAML

echo "✅ Généré: configs/docker/node{1..10}.yaml + configs/docker/cluster.yaml"
