use bincode::{Decode, Encode};
use std::mem::size_of;

use super::node::NodeId;

/// ====================
///   Log / Commitment
/// ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum MsgType {
    Send = 0,
    Recv = 1,
}

#[derive(Debug, Clone, Encode, Decode)]
pub enum MsgContent {
    SendContent {
        dest: NodeId,
        message: String,
    },
    RecvContent {
        src: NodeId,
        src_seq: usize,
        message: String,
    },
}

/// Entrée du journal
#[derive(Debug, Clone, Encode, Decode)]
pub struct MsgLogEntry {
    pub seq: usize,
    pub msg_type: MsgType,
    pub dest: NodeId,
    pub hash: [u8; 32],
    pub sig: [u8; 64],
    pub content: MsgContent,
}

/// =================
///   Authenticator
/// =================

#[derive(Debug, Clone, Encode, Decode)]
pub struct Authenticator {
    pub seq: usize,
    pub hash: [u8; 32],
    pub sig: [u8; 64], // Signature de (seq || hash)
}

const SIGNED_DATA_LEN: usize = size_of::<usize>() + size_of::<[u8; 32]>();

impl Authenticator {
    /// Retourne les données signées : seq || hash
    pub fn signed_data(&self) -> [u8; SIGNED_DATA_LEN] {
        const SEQ_LEN: usize = size_of::<usize>();

        let mut buf = [0u8; SIGNED_DATA_LEN];
        buf[..SEQ_LEN].copy_from_slice(&self.seq.to_be_bytes());
        buf[SEQ_LEN..].copy_from_slice(&self.hash);
        buf
    }

    /// Vérifie la signature de l'authenticator avec une clé publique
    pub fn verify(&self, public_key: &ed25519_dalek::PublicKey) -> bool {
        use ed25519_dalek::{Signature, Verifier};

        let signed_data = self.signed_data();
        let signature = match Signature::from_bytes(&self.sig) {
            Ok(sig) => sig,
            Err(_) => return false,
        };

        public_key.verify(&signed_data, &signature).is_ok()
    }
}

/// =====================
///   Challenge objects
/// =====================

#[derive(Debug, Clone, Encode, Decode)]
pub struct Challenge {
    pub challenger: NodeId,
    pub target: NodeId,
    pub kind: ChallengeKind,
}

#[derive(Debug, Clone, Encode, Decode)]
pub enum ChallengeKind {
    /// Challenge d'audit
    Audit {
        min_auth: Authenticator,
        max_auth: Authenticator,
    },
    /// Challenge d'envoi
    Send {
        message: String,
        sender_auth: Authenticator,
    },
}

#[derive(Debug, Clone, Encode, Decode)]
pub enum ChallengeResponse {
    Audit {
        entries: Vec<MsgLogEntry>,
        prev_hash: [u8; 32],
    },
    Send {
        ack_seq: usize,
        ack_prev_hash: [u8; 32],
        ack_signature: [u8; 64],
    },
}

/// ================
///   Proof object
/// ================

#[derive(Debug, Clone, Encode, Decode)]
pub struct Proof {
    pub guilty_node: NodeId,
    pub accuser_node: NodeId,
    pub authenticator: Authenticator, // Authenticator prouvant l'état fautif
    pub log_suffix: Vec<MsgLogEntry>, // Suffixe divergent du journal
}

/// =======================
///   PeerReview messages
/// =======================

#[derive(Debug, Clone, Encode, Decode)]
pub enum PeerReviewMsg {
    /// === Commitment protocol ===
    Send {
        seq: usize,
        prev_hash: [u8; 32],
        sig: [u8; 64],
        dest: NodeId,
        msg: String,
    },

    /// === Consistency protocol ===
    AuthenticatorBroadcast {
        auth: Authenticator,
        auth_node: NodeId,
    },

    /// === Audit protocol ===
    AuditRequest {
        min_seq: usize,
        max_seq: usize,
    },
    AuditResponse {
        entries: Vec<MsgLogEntry>,
        prev_hash: [u8; 32],
    },

    /// === Challenge protocol ===
    ChallengeRequest {
        challenge: Challenge,
    },
    ChallengeResponse {
        response: ChallengeResponse,
    },

    /// === Evidence transfer protocol ===
    EvidenceRequest {
        target: NodeId,
    },
    EvidenceResponse {
        target: NodeId,
        challenges: Vec<Challenge>,
        proofs: Vec<Proof>,
    },
    ProofBroadcast {
        proof: Proof,
    },
}
