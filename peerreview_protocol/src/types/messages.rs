use bincode::{Decode, Encode};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::NodeId;

fn sha256_32(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut a = [0u8; 32];
    a.copy_from_slice(&out[..32]);
    a
}

/// Authenticator minimal (papier-compatible au niveau “commitment”):
/// signature sur (node_id, seq, head_hash, kind, ts).
///
/// NOTE: `sig` est un `Vec<u8>` (64 bytes) pour compat serde (arrays > 32).
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct Commitment {
    pub node_id: NodeId,
    pub seq: u64,
    pub head_hash: [u8; 32],
    pub kind: String,
    pub ts_ms: u64,
    pub sig: Vec<u8>, // 64 bytes
}

impl Commitment {
    pub fn digest(&self) -> [u8; 32] {
        let mut buf = Vec::with_capacity(4 + 8 + 32 + self.kind.len() + 8);
        buf.extend_from_slice(&self.node_id.to_be_bytes());
        buf.extend_from_slice(&self.seq.to_be_bytes());
        buf.extend_from_slice(&self.head_hash);
        buf.extend_from_slice(self.kind.as_bytes());
        buf.extend_from_slice(&self.ts_ms.to_be_bytes());
        sha256_32(&buf)
    }

    pub fn new_signed(
        node_id: NodeId,
        seq: u64,
        head_hash: [u8; 32],
        kind: impl Into<String>,
        ts_ms: u64,
        sk: &SigningKey,
    ) -> Self {
        let mut c = Self {
            node_id,
            seq,
            head_hash,
            kind: kind.into(),
            ts_ms,
            sig: Vec::new(),
        };
        let d = c.digest();
        let sig: Signature = sk.sign(&d);
        c.sig = sig.to_bytes().to_vec();
        c
    }

    pub fn verify(&self, vk: &VerifyingKey) -> Result<(), String> {
        if self.sig.len() != 64 {
            return Err("commitment sig must be 64 bytes".to_string());
        }
        let mut s64 = [0u8; 64];
        s64.copy_from_slice(&self.sig);
        let sig = Signature::from_bytes(&s64);
        let d = self.digest();
        vk.verify(&d, &sig).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum Evidence {
    Equivocation {
        node_id: NodeId,
        seq: u64,
        head_a: [u8; 32],
        head_b: [u8; 32],
    },
    BadLogSegment {
        node_id: NodeId,
        reason: String,
    },
    NoResponse {
        node_id: NodeId,
        timeout_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum Verdict {
    OK,
    FAULT { evidence: Evidence },
    SUSPECTED { evidence: Evidence },
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum PeerReviewMsg {
    Ping,
    Pong,
    Ack,

    Commitment(Commitment),

    Challenge {
        node_id: NodeId,
        from_seq: u64,
        to_seq: u64,
        a_from: Commitment,
        a_to: Commitment,
    },

    LogSlice {
        node_id: NodeId,
        lines: Vec<String>,
    },

    WitnessQuery { node_id: NodeId },
    WitnessReply {
        node_id: NodeId,
        commits: Vec<Commitment>,
    },

    Verdict { about: NodeId, verdict: Verdict },
}
