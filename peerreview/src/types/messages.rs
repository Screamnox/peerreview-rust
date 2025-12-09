use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerReviewMsg {
    /// Messages simples pour tester le réseau PR (ping/pong)
    Ping { from: u32, payload: String },
    Pong { from: u32, payload: String },

    /// Messages PeerReview "réels" (placeholders pour l'équipe PR)
    Send {
        seq_num: u64,
        prev_hash: Vec<u8>,   // TODO: pourra devenir [u8; 32]
        signature: Vec<u8>,   // TODO: pourra devenir [u8; 64]
        dest: u32,
        payload: String,
    },
    AuditRequest {
        min_seq: u64,
        max_seq: u64,
    },
    AuditResponse {
        // TODO: ajouter entries: Vec<LogEntry> plus tard
        prev_hash: Vec<u8>,   // TODO: [u8; 32] plus tard
    },
    SendChallenge {
        message: String,
        sender_sig: Vec<u8>,  // TODO: [u8; 64] plus tard
    },
    AuditChallenge {
        min_auth: Vec<u8>,    // TODO: [u8; 64]
        max_auth: Vec<u8>,    // TODO: [u8; 64]
    },
    ChallengeResponse {
        // TODO: à définir
    },
    EvidenceRequest,
    EvidenceResponse {
        // TODO: ajouter challenges + proofs plus tard
    },
    AuthenticatorBroadcast {
        node_id: u32,
        authenticator: Vec<u8>, // TODO: [u8; 64] plus tard
    },
}
