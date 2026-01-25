import yaml

def generate_compose(num_nodes=4, base_port=5000, image_name="peerreview_node", dockerfile="docker/Dockerfile.node"):
    compose = {
        "version": "3.8",
        "services": {},
        "networks": {
            "peerreview_net": {"driver": "bridge"}
        }
    }

    for node_id in range(1, num_nodes + 1):
        service_name = f"node{node_id}"
        compose["services"][service_name] = {
            "build": {
                "context": "..",
                "dockerfile": dockerfile
            },
            "command": [f"nodes/node{node_id}.toml"],
            "ports": [f"{base_port + node_id}:{base_port + node_id}"],
            "networks": ["peerreview_net"]
        }

    # Write to docker-compose.yml
    with open("docker-compose.generated.yml", "w") as f:
        yaml.dump(compose, f, sort_keys=False)

    print(f"Generated docker-compose.generated.yml with {num_nodes} nodes.")

if __name__ == "__main__":
    N = 4  # change to however many nodes you want
    generate_compose(N)
