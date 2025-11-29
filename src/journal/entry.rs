use ed25519_dalek::Signature;

use super::errors::{LogError, Result};

/// Message type: SEND or RECV
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Send = 0,
    Recv = 1,
}

impl TryFrom<i64> for MessageType {
    type Error = LogError;

    fn try_from(v: i64) -> Result<Self> {
        match v {
            0 => Ok(MessageType::Send),
            1 => Ok(MessageType::Recv),
            invalid => Err(LogError::InvalidMessageType(invalid)),
        }
    }
}

// Hash type
pub const HASH_SIZE: usize = 32;

pub type Hash = [u8; HASH_SIZE];

/// Log entry
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub seq: u64,
    pub kind: MessageType,
    pub dest: u32,
    pub hash: Hash,       // Recursive Hash (4.4 - PeerReview)
    pub sig: Signature,   // Authenticator (4.4 - PeerReview)
    pub content: Vec<u8>, // Data (raw bytes)
}
