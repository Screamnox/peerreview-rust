use bincode::{Decode, Encode};

use super::node::NodeId;

#[derive(Debug, Clone, Encode, Decode)]
pub enum PeerReviewMsg {
    Ping { from: NodeId, counter: u64 },
    Pong { from: NodeId, counter: u64 },

    /// Placeholder : quand “audit” arrive, on aura un enum plus riche
    /// (IHAVE/REQUEST/BATCH, etc.). En attendant on peut transporter du brut.
    Raw { from: NodeId, bytes: Vec<u8> },
}
