use bincode::{Decode, Encode};

use super::node::NodeId;

/// Commitment (authenticator) minimal lié à un event APP.
#[derive(Debug, Clone, Encode, Decode)]
pub struct Commitment {
    pub app_event_kind: String, // "APP_SEND" / "APP_RECV" / ...
    pub from: NodeId,           // logger
    pub observed: NodeId,       // nœud observé (destinataire ou source selon event)
    pub msg_id: String,
    pub hash32: [u8; 32],
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Encode, Decode)]
pub enum ChallengeKind {
    Consistency,
    Audit,
}

#[derive(Debug, Clone, Encode, Decode)]
pub enum PeerReviewMsg {
    // Sanity / transport test
    Ping {
        from: NodeId,
        counter: u64,
    },
    Pong {
        from: NodeId,
        counter: u64,
    },

    // Core
    Commitment(Commitment),

    // Challenges / Responses (audit & consistency)
    Challenge {
        from: NodeId,
        to: NodeId,
        kind: ChallengeKind,
        since_seq: u64,
        until_seq: u64,
    },

    // Pour l’instant, réponse minimaliste : lignes brutes (json/texte).
    // (Plus tard: structuré avec preuves exactes)
    Response {
        from: NodeId,
        to: NodeId,
        kind: ChallengeKind,
        lines: Vec<String>,
    },

    // Diffusion d’une preuve d’exposition (placeholder)
    ExposureProof {
        accuser: NodeId,
        accused: NodeId,
        summary: String,
    },
}
