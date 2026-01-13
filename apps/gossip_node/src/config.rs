use serde::{Deserialize, Serialize};

pub type NodeId = u32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCfg {
    pub node_id: String,         // "node10"
    pub node_num: NodeId,        // 10
    pub http_port: u16,          // 8090
    pub pr_port: u16,            // 8001 (inside container)
    pub app_port: u16,           // 7001 (inside container)

    /// peers are numeric ids (1..10)
    pub peers: Vec<NodeId>,

    /// witness set (article): nodes that witness PR events for this node
    /// (can be empty; verifier still works)
    pub witnesses: Vec<NodeId>,
}

impl NodeCfg {
    /// Deterministic fallback: if witnesses not set, take first 3 peers.
    pub fn normalized(mut self) -> Self {
        if self.witnesses.is_empty() {
            self.witnesses = self.peers.iter().cloned().take(3).collect();
        }
        self
    }
}
