use bincode::{Decode, Encode};
use std::mem::size_of;

use crate::journal::entry::LogEntry;

use super::node::NodeId;

/// ====================
///   Log / Commitment
/// ====================

/*
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
*/

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

pub type ChallengeId = u64;
pub type ChallengeKey = (ChallengeId, NodeId);

#[derive(Debug, Clone, Encode, Decode)]
pub struct Challenge {
    pub id: ChallengeId,
    pub challenger: NodeId,
    pub target: NodeId,
    pub kind: ChallengeKind,
}

impl Challenge {
    pub fn key(&self) -> ChallengeKey {
        (self.id, self.challenger)
    }
}

/// Challenge Kind
#[derive(Debug, Clone, Encode, Decode)]
pub struct AuditChallenge {
    pub min_auth: Authenticator,
    pub max_auth: Authenticator,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SendChallenge {
    pub message: SendMsg,
    pub sender_auth: Authenticator,
}

#[derive(Debug, Clone, Encode, Decode)]
pub enum ChallengeKind {
    Audit(AuditChallenge),
    Send(SendChallenge),
}

/// Challenge responses
#[derive(Debug, Clone, Encode, Decode)]
pub struct AuditResponse {
    pub entries: Vec<LogEntry>,
    pub prev_hash: [u8; 32],
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SendResponse {
    pub auth: Authenticator,
}

#[derive(Debug, Clone, Encode, Decode)]
pub enum ChallengeAnswer {
    Audit(AuditResponse),
    Send(SendResponse),
}

/// ================
///   Proof object
/// ================

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub enum EvidenceType {
    SignatureMismatch,      // Signature différente de l'authenticator stocké
    BrokenHashChain,        // Chaîne de hash invalide
    InvalidSignature,       // Signature Ed25519 invalide
    InvalidAckResponse,     // Faute au challenge d'envoi
    MissingLogEntries,      // Faute au challenge d'audit
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Proof {
    pub faulty_node: NodeId,
    pub accuser_node: NodeId,
    pub authenticator: Authenticator,       // Authenticator prouvant l'état fautif
    pub log_suffix: Option<Vec<LogEntry>>,  // Suffixe divergent du journal
    pub challenge_key: Option<ChallengeKey>,// Challenge qui a échoué
    pub kind: Option<EvidenceType>,
    pub reason: Option<String>,
}

/// =======================
///   PeerReview messages
/// =======================

/* Commitment */
#[derive(Debug, Clone, Encode, Decode)]
pub struct SendMsg {
    pub seq: usize,
    pub prev_hash: [u8; 32],
    pub sig: [u8; 64],
    pub dest: NodeId,
    pub msg: String,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AuthenticatorBroadcast {
    pub auth: Authenticator,
    pub auth_node: NodeId,
}

/* Audit */
#[derive(Debug, Clone, Encode, Decode)]
pub struct AuditRequest {
    pub min_seq: usize,
    pub max_seq: usize,
}

/* Challenge */
#[derive(Debug, Clone, Encode, Decode)]
pub struct ChallengeRequest {
    pub challenge: Challenge,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct ChallengeResponse {
    pub challenge_key: ChallengeKey,
    pub answer: ChallengeAnswer,
}

/* Evidence Transfer */
#[derive(Debug, Clone, Encode, Decode)]
pub struct EvidenceRequest {
    pub target: NodeId,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct EvidenceResponse {
    pub target: NodeId,
    pub challenges: Vec<Challenge>,
    pub proofs: Vec<Proof>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct ProofBroadcast {
    pub proof: Proof,
}

/// PeerReview messages
#[derive(Debug, Clone, Encode, Decode)]
pub enum PeerReviewMsg {
    /// === Commitment protocol ===
    Send(SendMsg),
    Ack(SendMsg),

    /// === Consistency protocol ===
    ConsistencyRequest(AuditRequest),
    ConsistencyResponse(AuditResponse),

    /// === Consistency protocol ===
    AuthenticatorBroadcast(AuthenticatorBroadcast),

    /// === Audit protocol ===
    AuditRequest(AuditRequest),
    AuditResponse(AuditResponse),

    /// === Challenge protocol ===
    ChallengeRequest(ChallengeRequest),
    ChallengeResponse(ChallengeResponse),

    /// === Evidence transfer protocol ===
    EvidenceRequest(EvidenceRequest),
    EvidenceResponse(EvidenceResponse),
    ProofBroadcast(ProofBroadcast),
}