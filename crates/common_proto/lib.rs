use serde::{Deserialize, Serialize};

pub type NodeId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MsgKind {
    Heartbeat { counter: u64, tree_id : u8 },
    Publish,
    Ihave { ids: Vec<String> },
    Request { ids: Vec<String> },
    Batch { msgs: Vec<Msg> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Msg {
    pub id: String,
    pub from: NodeId,
    pub kind: MsgKind,
    pub payload: Vec<u8>,
    pub ts_ms: u64,
    pub tree_id: u8,
}
