use crate::journal::Logger;

/// Type de message : Send ou Recv
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum MessageType {
    Send,
    Recv,
}

/// Structure d'un message PeerReview transmis sur le réseau
#[derive(Debug, Clone)]
pub struct PeerReviewMessage {
    pub msg_type: MessageType,
    pub seq_num: usize,          // sk
    #[allow(dead_code)]
    pub prev_hash: [u8; 32],     // hk-1
    pub signature: [u8; 64],     // αk (signature MAC)
    #[allow(dead_code)]
    pub dest: u32,               // Destinataire
    pub payload: String,         // Message m
}

/// Structure d'un nœud PeerReview
pub struct PeerReviewNode {
    pub node_id: u32,
    pub logger: Logger,
    pub prev_hash: [u8; 32],       // Hash de l'entrée précédente (hk-1)
    #[allow(dead_code)]
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
