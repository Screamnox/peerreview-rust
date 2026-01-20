import subprocess
import time
from dataclasses import dataclass
from typing import Dict, Any, Optional, List

import requests
import streamlit as st
import yaml

st.set_page_config(page_title="PeerReview Demo Dashboard", layout="wide")

# -----------------------------
# Config
# -----------------------------
DEFAULT_CLUSTER_YAML = "../../configs/docker/cluster.yaml"
DEFAULT_SUSPECT = "node10"
DEFAULT_SENDER = "node4"
DEFAULT_RECEIVER = "node10"

PORT_MAP = {
    # docker compose maps nodeX -> localhost:808X (node10 -> 8090)
    "node1": 8081, "node2": 8082, "node3": 8083, "node4": 8084, "node5": 8085,
    "node6": 8086, "node7": 8087, "node8": 8088, "node9": 8089, "node10": 8090,
}

# -----------------------------
# Helpers
# -----------------------------
def sh(cmd: List[str], timeout: int = 60) -> str:
    """Run a command and return combined output."""
    p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=timeout)
    return p.stdout

def load_cluster(path: str) -> Dict[str, Any]:
    with open(path, "r", encoding="utf-8") as f:
        return yaml.safe_load(f)

def node_url(name: str) -> str:
    port = PORT_MAP.get(name)
    if port is None:
        raise ValueError(f"Unknown node name {name}")
    return f"http://localhost:{port}"

def http_get_json(url: str, timeout: float = 1.2) -> Optional[Dict[str, Any]]:
    try:
        r = requests.get(url, timeout=timeout)
        if r.status_code != 200:
            return None
        return r.json()
    except Exception:
        return None

def http_post_json(url: str, payload: Dict[str, Any], timeout: float = 2.5) -> Optional[Dict[str, Any]]:
    try:
        r = requests.post(url, json=payload, timeout=timeout)
        if r.status_code != 200:
            return {"_error": f"HTTP {r.status_code}", "_text": r.text}
        return r.json()
    except Exception as e:
        return {"_error": str(e)}

def is_witness(stats: Dict[str, Any]) -> bool:
    v = stats.get("is_witness")
    return bool(v) if v is not None else False

def fault_str(stats: Dict[str, Any]) -> str:
    # your /stats returns a single string like: "equivocate_head=false drop_from=None forge_recv_from=None"
    v = stats.get("fault")
    return str(v) if v is not None else ""

def safe_int(stats: Dict[str, Any], key: str) -> int:
    try:
        return int(stats.get(key, 0))
    except Exception:
        return 0

def render_cluster_graph(names: List[str], witnesses: List[str], suspect: str) -> str:
    # Graphviz DOT: simple star-ish visualization.
    # You can improve later (edges from config); for demo this is already very visual.
    lines = []
    lines.append("digraph G {")
    lines.append('  rankdir=LR;')
    lines.append('  node [shape=box, style="rounded,filled", fontname="Helvetica"];')

    for n in names:
        if n == suspect:
            color = "#ffcccc"
            label = f"{n}\\n(SUSPECT)"
        elif n in witnesses:
            color = "#cce5ff"
            label = f"{n}\\n(WITNESS)"
        else:
            color = "#d4edda"
            label = n
        lines.append(f'  "{n}" [fillcolor="{color}", label="{label}"];')

    # Draw edges: suspect -> witnesses (commitments)
    for w in witnesses:
        lines.append(f'  "{suspect}" -> "{w}" [label="PR commitment"];')

    # Draw a simple "gossip mesh" feel: chain edges
    for i in range(len(names)-1):
        lines.append(f'  "{names[i]}" -> "{names[i+1]}" [style=dashed, label="gossip"];')

    lines.append("}")
    return "\n".join(lines)

# -----------------------------
# UI Header
# -----------------------------
st.title("PeerReview Visual Demo Dashboard")
st.caption("Cluster 10 nodes (Docker) • Witnesses • Commitments • Live audit • App audit (SEND/RECV/DELIVER invariants)")

colA, colB, colC, colD = st.columns([2, 2, 2, 2])
with colA:
    cluster_path = st.text_input("Cluster YAML", DEFAULT_CLUSTER_YAML)
with colB:
    suspect = st.selectbox("Suspect", list(PORT_MAP.keys()), index=list(PORT_MAP.keys()).index(DEFAULT_SUSPECT))
with colC:
    sender = st.selectbox("Sender (app)", list(PORT_MAP.keys()), index=list(PORT_MAP.keys()).index(DEFAULT_SENDER))
with colD:
    receiver = st.selectbox("Receiver (app)", list(PORT_MAP.keys()), index=list(PORT_MAP.keys()).index(DEFAULT_RECEIVER))

try:
    cluster = load_cluster(cluster_path)
except Exception as e:
    st.error(f"Cannot load cluster yaml: {e}")
    st.stop()

# node list
names = [n["name"] for n in cluster.get("nodes", []) if "name" in n]
names = [n for n in names if n in PORT_MAP]  # keep only demo-known names

# -----------------------------
# Live stats polling
# -----------------------------
with st.sidebar:
    st.subheader("Refresh")
    auto = st.toggle("Auto-refresh (1s)", value=True)
    if st.button("Refresh now"):
        st.session_state["_refresh"] = time.time()

if auto:
    # simple refresh tick
    time.sleep(0.2)
    st.session_state["_refresh"] = time.time()

stats_by_node: Dict[str, Dict[str, Any]] = {}
for n in names:
    s = http_get_json(node_url(n) + "/stats")
    if s is not None:
        stats_by_node[n] = s

witnesses = [n for n in names if n in stats_by_node and is_witness(stats_by_node[n])]

# -----------------------------
# Top: Cluster graph
# -----------------------------
dot = render_cluster_graph(names, witnesses, suspect)
st.graphviz_chart(dot, use_container_width=True)

# -----------------------------
# Nodes table
# -----------------------------
st.subheader("Nodes overview")
cols = st.columns(5)
for i, n in enumerate(names):
    card = cols[i % 5]
    s = stats_by_node.get(n)
    if s is None:
        card.error(f"{n}\nOFFLINE")
        continue

    title = f"**{n}**"
    if n == suspect:
        title += "  🔥"
    if is_witness(s):
        title += "  👁️"

    card.markdown(title)
    card.caption(f"fault: {fault_str(s)}")

    # show PR counters if present
    card.write({
        "pr_commit_sent": safe_int(s, "pr_commit_sent"),
        "pr_commit_recv": safe_int(s, "pr_commit_recv"),
        "recv_total": safe_int(s, "recv_total"),
    })

# -----------------------------
# Actions
# -----------------------------
st.subheader("Live actions")

c1, c2, c3, c4 = st.columns(4)

with c1:
    st.markdown("### App send")
    txt = st.text_input("Text", value="hello app msg")
    if st.button("Send sender → receiver"):
        payload = {"to": receiver, "text": txt}
        res = http_post_json(node_url(sender) + "/send_text", payload)
        st.session_state["last_send"] = res

    if "last_send" in st.session_state:
        st.code(st.session_state["last_send"], language="json")

with c2:
    st.markdown("### Publish (gossip)")
    ptxt = st.text_input("Publish text", value="hello from publish_text")
    pub_node = st.selectbox("Publish node", names, index=names.index(receiver) if receiver in names else 0)
    if st.button("Publish text"):
        payload = {"text": ptxt}
        res = http_post_json(node_url(pub_node) + "/publish_text", payload)
        st.session_state["last_pub"] = res
    if "last_pub" in st.session_state:
        st.code(st.session_state["last_pub"], language="json")

with c3:
    st.markdown("### PeerReview live audit")
    if st.button("Run pr_audit_live (suspect)"):
        out = sh([
            "bash", "-lc",
            f'docker run --rm --network docker_gossip_net -v "$PWD:/w" -w /w '
            f'debian:bookworm-slim ./target/debug/pr_audit_live --cluster {cluster_path} --suspect {suspect}'
        ], timeout=60)
        st.session_state["audit_live_out"] = out
    if "audit_live_out" in st.session_state:
        st.code(st.session_state["audit_live_out"])

with c4:
    st.markdown("### App audit (cross-logs)")
    require_deliver = st.checkbox("Require DELIVER", value=True)
    if st.button("Run pr_audit_app (sender+receiver)"):
        # copy logs from docker then run local cargo (fast dev)
        cmd = (
            "set -euo pipefail; "
            "mkdir -p /tmp/prlogs; "
            f"docker cp {sender}:/app/peerreview_logs/{sender}/{sender}_app.log /tmp/prlogs/{sender}.log || true; "
            f"docker cp {receiver}:/app/peerreview_logs/{receiver}/{receiver}_app.log /tmp/prlogs/{receiver}.log || true; "
            f"cargo run -p peerreview_protocol --bin pr_audit_app -- "
            f"--log {sender}=/tmp/prlogs/{sender}.log --log {receiver}=/tmp/prlogs/{receiver}.log "
            + ("--require-deliver" if require_deliver else "")
        )
        out = sh(["bash", "-lc", cmd], timeout=120)
        st.session_state["audit_app_out"] = out
    if "audit_app_out" in st.session_state:
        st.code(st.session_state["audit_app_out"])

# -----------------------------
# Scenario runner
# -----------------------------
st.subheader("Scenario runner (one-click demos)")

scenario = st.selectbox(
    "Scenario",
    [
        ("OK (clean)", "scripts/demo_10_ok.sh"),
        ("FAULT: Equivocation (fork)", "scripts/demo_20_equivocation.sh"),
        ("FAULT: Drop / Omission", "scripts/demo_30_drop.sh"),
        ("FAULT: Forge / Invented message", "scripts/demo_40_forge.sh"),
    ],
    format_func=lambda x: x[0],
)

if st.button("Run selected scenario"):
    label, path = scenario
    out = sh(["bash", "-lc", f"bash {path}"], timeout=600)
    st.session_state["scenario_out"] = f"=== {label} ===\n" + out

if "scenario_out" in st.session_state:
    st.code(st.session_state["scenario_out"])

st.caption("Tip: keep this dashboard open during demo; it updates live and prints audit verdicts.")
