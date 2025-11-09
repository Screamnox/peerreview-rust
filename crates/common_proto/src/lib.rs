pub type NodeId = String;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MsgKind {
    Heartbeat { counter: u64, tree_id: u8 },
    Publish,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Msg {
    pub id: String,
    pub from: NodeId,
    pub kind: MsgKind,
    pub payload: Vec<u8>,
    pub ts_ms: u64,
    pub tree_id: u8,
}
