use crate::config::NodeId;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct AppMsg {
    pub msg_id: String,
    pub from: NodeId,
    pub payload: String,
    pub hash32: [u8; 32],
    pub ts_ms: u64,
}

#[derive(Debug)]
pub struct NodeState {
    pub node_id: String,
    pub node_num: NodeId,

    pub known_count: usize,
    pub pr_app_events_sent: u64,
    pub trees: u32,

    pub last_msgs: VecDeque<String>,
    pub delivered: HashMap<String, AppMsg>,
}

impl NodeState {
    pub fn new(node_id: String, node_num: NodeId) -> Self {
        Self {
            node_id,
            node_num,
            known_count: 0,
            pr_app_events_sent: 0,
            trees: 3,
            last_msgs: VecDeque::with_capacity(50),
            delivered: HashMap::new(),
        }
    }

    pub fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub fn hash_payload(payload: &str) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(payload.as_bytes());
        let digest = h.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest[..]);
        out
    }

    pub fn push_last(&mut self, s: String) {
        if self.last_msgs.len() == 50 {
            self.last_msgs.pop_back();
        }
        self.last_msgs.push_front(s);
    }
}
