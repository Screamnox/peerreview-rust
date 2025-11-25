use crate::journal::Logger;

/// Type de message : SEND ou RECV
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MessageType {
    SEND,
    RECV,
}

/// Structure d'un message PeerReview transmis sur le réseau
#[derive(Debug, Clone)]
pub struct PeerReviewMessage {
    pub msg_type: MessageType,
    pub seq_num: usize,          // sk
    pub prev_hash: [u8; 32],     // hk-1
    pub signature: [u8; 32],     // αk (signature MAC)
    pub dest: u32,               // Destinataire
    pub payload: String,         // Message m
}

/// Structure d'un nœud PeerReview
pub struct PeerReviewNode {
    pub node_id: u32,
    pub logger: Logger,
    pub prev_hash: [u8; 32],       // Hash de l'entrée précédente (hk-1)
    pub private_key: [u8; 32],     // Clé privée pour signature MAC
}

impl PeerReviewNode {
    /// Crée un nouveau nœud PeerReview
    pub fn new(node_id: u32, logger: Logger, private_key: [u8; 32]) -> Self {
        Self {
            node_id,
            logger,
            prev_hash: [0u8; 32],  // Initialiser avec un hash nul
            private_key,
        }
    }
}
