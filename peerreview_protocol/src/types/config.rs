use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::Path;

/// ----
/// Cluster-level config (used by apps + audit tooling)
/// ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterNode {
    pub id: u32,
    pub name: String,
    pub app_addr: String,
    pub pr_addr: String,
    pub http_addr: Option<String>,

    #[serde(default)]
    pub public_key_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub nodes: Vec<ClusterNode>,
    pub fanout: usize,
    pub num_trees: usize,

    /// Dedicated witness nodes (names), ex: ["node1","node2","node3"]
    #[serde(default)]
    pub witnesses: Vec<String>,
}

impl ClusterConfig {
    pub fn from_yaml_file(path: impl AsRef<Path>) -> io::Result<Self> {
        let s = std::fs::read_to_string(path)?;
        let cfg: Self =
            serde_yaml::from_str(&s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(cfg)
    }

    pub fn map_by_name(&self) -> HashMap<String, ClusterNode> {
        self.nodes
            .iter()
            .cloned()
            .map(|n| (n.name.clone(), n))
            .collect()
    }

    pub fn find_by_name(&self, name: &str) -> Option<&ClusterNode> {
        self.nodes.iter().find(|n| n.name == name)
    }

    pub fn find_by_id(&self, id: u32) -> Option<&ClusterNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn is_witness(&self, node_name: &str) -> bool {
        self.witnesses.iter().any(|w| w == node_name)
    }

    /// Minimal witness selection:
    /// - if `witnesses` is set in YAML: use those (excluding the suspect itself)
    /// - else: deterministic ring fallback (next k nodes)
    pub fn witnesses_for(&self, node_id: u32, k: usize) -> Vec<ClusterNode> {
        if !self.witnesses.is_empty() {
            return self
                .witnesses
                .iter()
                .filter(|name| {
                    self.find_by_name(name)
                        .map(|n| n.id != node_id)
                        .unwrap_or(false)
                })
                .filter_map(|name| self.find_by_name(name).cloned())
                .collect();
        }

        let mut ids = self.nodes.iter().map(|n| n.id).collect::<Vec<_>>();
        ids.sort_unstable();
        if ids.is_empty() {
            return vec![];
        }

        let pos = ids.iter().position(|x| *x == node_id).unwrap_or(0);
        let mut out = Vec::new();
        for off in 1..=k {
            let idx = (pos + off) % ids.len();
            if let Some(n) = self.find_by_id(ids[idx]).cloned() {
                if n.id != node_id {
                    out.push(n);
                }
            }
        }
        out
    }
}

/// ----
/// Legacy/small network config types used by some modules (bootstrap)
/// Keep them to avoid breaking existing code.
/// ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfigEntry {
    pub id: u32,
    pub addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    pub peers: Vec<PeerConfigEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: u32,
    pub addr: String,
}
