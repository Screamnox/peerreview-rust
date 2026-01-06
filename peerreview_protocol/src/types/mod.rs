pub mod config;
pub mod messages;
pub mod node;

pub type NodeId = u32;

// --- Evénements côté application (APP -> PR) ---
#[derive(Debug, Clone)]
pub enum AppEvent {
    Send {
        to: NodeId,
        msg_id: String,
        hash32: [u8; 32],
        ts_ms: u64,
    },
    Recv {
        from: NodeId,
        msg_id: String,
        hash32: [u8; 32],
        ts_ms: u64,
    },
    Deliver {
        msg_id: String,
        hash32: [u8; 32],
        ts_ms: u64,
    },
}

// Ré-exports utiles
pub use config::{PeerConfig, PeerConfigEntry, PeerInfo};
pub use messages::PeerReviewMsg;
