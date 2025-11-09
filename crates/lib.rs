use serde::{Deserialize, Serialize};

pub type NodeId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MsgKind {
    Heartbeat { counter: u64 },
    Publish,                     // pour plus tard (injection d'un message)
    Ihave { ids: Vec<String> },  // anti-entropy (plus tard)
    Request { ids: Vec<String> },
    Batch { msgs: Vec<Msg> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Msg {
    pub id: String,       // UUID (String)
    pub from: NodeId,
    pub kind: MsgKind,
    pub payload: Vec<u8>, // utile pour Publish (plus tard)
    pub ts_ms: u64,
}
