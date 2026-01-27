import os
import base64
import toml
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric import ed25519
from cryptography.hazmat.primitives import serialization

def generate_node_configs(num_nodes: int, base_port=5000, output_dir="nodes"):
    os.makedirs(output_dir, exist_ok=True)

    peers_list = []

    for node_id in range(1, num_nodes + 1):
        node_dir = Path(output_dir) / f"node{node_id}"
        node_dir.mkdir(parents=True, exist_ok=True)

        private_key = ed25519.Ed25519PrivateKey.generate()
        public_key = private_key.public_key()

        key_file = node_dir / f"node{node_id}.key"
        private_bytes_raw = private_key.private_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PrivateFormat.Raw,
            encryption_algorithm=serialization.NoEncryption()
        )
        with open(key_file, "wb") as f:
            f.write(private_bytes_raw)

        pub_bytes = public_key.public_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PublicFormat.Raw
        )
        pub_b64 = base64.b64encode(pub_bytes).decode()
        peers_list.append({
            "id": node_id,
            "address": f"node{node_id}:{base_port + node_id}",
            "public_key": pub_b64,
            "witnesses": [i for i in range(1, num_nodes + 1) if i != node_id][:2]  # first 2 others
        })

        # Create nodeX.toml
        node_config = {
            "node": {
                "id": node_id,
                "log_file": str(node_dir / f"node{node_id}.log"),
                "log_max_lines": 1000,
                "log_min_line_size": 256,
                "keypair_file": str(key_file),
                "state_file": str(node_dir / f"node{node_id}_state.json")
            },
            "network": {
                "listen_address": f"0.0.0.0:{base_port + node_id}",
                "peers_file": str(Path(output_dir) / "peers.toml"),
                "connection_timeout_secs": 10,
                "ack_timeout_secs": 5
            },
            "timers": {
                "audit_interval_secs": 30,
                "consistency_check_secs": 60,
                "evidence_transfer_secs": 120
            },
            "witnesses": {
                "list": [i for i in range(1, num_nodes + 1) if i != node_id][:2]
            },
            "watched": {
                "list": [i for i in range(1, num_nodes + 1) if i != node_id][:2]
            }
        }

        with open(node_dir / f"node{node_id}.toml", "w") as f:
            toml.dump(node_config, f)

    peers_toml = {"peers": peers_list}
    with open(Path(output_dir) / "peers.toml", "w") as f:
        toml.dump(peers_toml, f)

    print(f"Generated {num_nodes} node configs and peers.toml in '{output_dir}'")

if __name__ == "__main__":
    generate_node_configs(10)
