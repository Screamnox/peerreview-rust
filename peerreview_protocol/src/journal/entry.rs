use serde::{Deserialize, Serialize};

use crate::types::NodeId;

/// Entrée de log PeerReview (journal sécurisé local).
/// Hash-chain + signature.
/// 
/// Approche (cohérente avec PeerReview, niveau MVP):
/// - chaque entrée inclut prev_hash
/// - hash = SHA256(prev_hash || seq || ts_ms || node_id || peer || kind || payload)
/// - sig = Sign(hash)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub seq: u64,
    pub ts_ms: u64,

    /// nœud qui écrit cette entrée (propriétaire du journal)
    pub node_id: NodeId,

    /// contrepartie (peer) concernée (send/recv), si applicable
    pub peer: Option<NodeId>,

    /// type stable (APP_SEND, APP_RECV, ...)
    pub kind: String,

    /// texte stable et lisible
    pub payload: String,

    pub prev_hash: [u8; 32],
    pub hash: [u8; 32],

    /// 64 bytes Ed25519 (on stocke en Vec pour éviter limites serde sur arrays)
    pub sig: Vec<u8>,
}

impl LogEntry {
    pub fn to_json_line(&self) -> std::io::Result<String> {
        serde_json::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn from_json_line(line: &str) -> std::io::Result<Self> {
        serde_json::from_str(line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }
}
