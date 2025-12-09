use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerReviewMsg {
    /// Message de base (écho, ping/pong, etc.) pour les tests réseau
    Ping { from: u32, payload: String },
    Pong { from: u32, payload: String },

    /// Messages PeerReview "réels" (à compléter par l’équipe PR)
    Send {
        seq_num: usize,
        prev_hash: [u8; 32],
        signature: [u8; 64],
        dest: u32,
        payload: String,
    },
    AuditRequest {
        min_seq: usize,
        max_seq: usize,
    },
    AuditResponse {
        // TODO: à compléter quand la struct LogEntry sera définie
        // entries: Vec<LogEntry>,
        prev_hash: [u8; 32],
    },
    SendChallenge {
        message: String,
        sender_sig: [u8; 64],
    },
    AuditChallenge {
        min_auth: [u8; 64],
        max_auth: [u8; 64],
    },
    ChallengeResponse {
        // TODO: à définir par l’équipe PeerReview
    },
    EvidenceRequest,
    EvidenceResponse {
        // TODO: à définir (challenges, preuves, etc.)
    },
    AuthenticatorBroadcast {
        node_id: u32,
        authenticator: [u8; 64],
    },
}
