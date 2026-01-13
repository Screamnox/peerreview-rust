use crate::config::NodeId;

/// Représente un “commitment” minimal lié à un événement applicatif.
/// (Article: les témoins servent à empêcher qu’un nœud réécrive l’histoire seul)
#[derive(Debug, Clone)]
pub struct Commitment {
    pub msg_id: String,
    pub app_hash32_hex: String,
    pub from: NodeId,
    pub ts_ms: u64,
    pub witnesses: Vec<NodeId>,
}

impl Commitment {
    pub fn to_pr_line(&self) -> String {
        format!(
            "commit msg_id={} from={} ts_ms={} hash={} witnesses={:?}",
            self.msg_id, self.from, self.ts_ms, self.app_hash32_hex, self.witnesses
        )
    }
}
