use crate::journal::Logger;
use crate::types::{NodeId, PeerReviewMsg};
use ed25519_dalek::VerifyingKey;
use std::collections::HashMap;
use std::io;
use std::path::Path;

#[derive(Debug, Clone)]
pub enum AuditVerdict {
    Ok,
    Fail(String),
}

pub struct AuditNode {
    pub node_id: NodeId,
    pub peer_public_keys: HashMap<u32, VerifyingKey>,
}

impl AuditNode {
    pub fn new(node_id: NodeId, peer_public_keys: HashMap<u32, VerifyingKey>) -> Self {
        Self {
            node_id,
            peer_public_keys,
        }
    }

    /// Vérifie les logs d'un set de noeuds (node<ID>.log) dans un répertoire.
    pub fn verify_logs_dir(&self, dir: impl AsRef<Path>, strict: bool) -> io::Result<AuditVerdict> {
        let dir = dir.as_ref();

        for (peer_id, vk) in &self.peer_public_keys {
            let path = dir.join(format!("node{peer_id}.log"));
            if path.exists() {
                Logger::verify_log_file(&path, vk, strict)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("verify node{peer_id}: {e}")))?;
            }
        }
        Ok(AuditVerdict::Ok)
    }

    /// Placeholder pour checks PR futurs (evidence/consistency/challenge...)
    pub fn inspect_pr_msg(&self, _m: &PeerReviewMsg) -> AuditVerdict {
        AuditVerdict::Ok
    }
}
