use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use crate::types::NodeId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub nodes: Vec<ClusterNode>,
    pub fanout: usize,
    pub num_trees: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterNode {
    pub id: NodeId,
    pub name: String,
    pub app_addr: String,
    pub pr_addr: String,
    pub http_addr: Option<String>,
    pub public_key_b64: Option<String>,
}

impl ClusterConfig {
    pub fn from_yaml_file(path: impl AsRef<Path>) -> io::Result<Self> {
        let s = fs::read_to_string(path)?;
        serde_yaml::from_str::<ClusterConfig>(&s)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn node_by_name(&self, name: &str) -> Option<&ClusterNode> {
        self.nodes.iter().find(|n| n.name == name)
    }

    pub fn node_by_id(&self, id: NodeId) -> Option<&ClusterNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

/// Legacy / compatibility types.
/// Certaines parties du code (ou anciens commits) importent PeerConfig / PeerInfo.
/// On garde PeerCfg / PeerCfgEntry comme structures “réelles”, mais on expose
/// des alias pour éviter les erreurs E0432.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerCfg {
    pub peers: Vec<PeerCfgEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerCfgEntry {
    pub id: NodeId,
    pub addr: SocketAddr,
    pub public_key_b64: Option<String>,
}

// --- Aliases attendus par d'autres modules ---
pub type PeerConfig = PeerCfg;
pub type PeerConfigEntry = PeerCfgEntry;
pub type PeerInfo = PeerCfgEntry;

impl PeerCfgEntry {
    pub fn public_key_bytes_32(&self) -> io::Result<Option<[u8; 32]>> {
        let Some(b64) = &self.public_key_b64 else {
            return Ok(None);
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        if bytes.len() != 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("public_key_b64 must decode to 32 bytes, got {}", bytes.len()),
            ));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        Ok(Some(out))
    }
}
