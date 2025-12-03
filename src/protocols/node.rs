use crate::journal::Logger;
use std::collections::HashMap;

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
    pub seq_num: usize,          
    #[allow(dead_code)]
    pub prev_hash: [u8; 32],     
    pub signature: [u8; 64],     
    #[allow(dead_code)]
    pub dest: u32,               
    pub payload: String,         
}

/// Structure d'un nœud PeerReview
pub struct PeerReviewNode {
    pub node_id: u32,
    pub logger: Logger,
    pub prev_hash: [u8; 32],       
    #[allow(dead_code)]
    pub private_key: [u8; 32],     
    pub peer_public_keys: HashMap<u32, ed25519_dalek::PublicKey>, 
}

impl PeerReviewNode {
    /// Crée un nouveau nœud PeerReview
    pub fn new(node_id: u32, mut logger: Logger, private_key: [u8; 32]) -> Self {
        // Récupérer le hash initial du logger (dernier hash enregistré ou HASH_INIT)
        let prev_hash = if logger.s_k == 0 {
            // Pas encore de logs, utiliser HASH_INIT du Logger
            const HASH_INIT: [u8; 32] = [
                0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8, 0x5F, 0xA2, 0x39, 0x6C, 0x00, 0x4D, 0x8B, 0x17,
                0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE, 0x29, 0x74, 0x66, 0x01, 0xB8, 0x42, 0xDA, 0x10,
            ];
            HASH_INIT
        } else {
            // Récupérer le dernier hash du logger
            match logger.get_log(1) {
                Ok(logs) if !logs.is_empty() => logs[0].hash,
                _ => [0u8; 32], // Fallback sur hash nul si erreur
            }
        };
        
        Self {
            node_id,
            logger,
            prev_hash,
            private_key,
            peer_public_keys: HashMap::new(),
        }
    }

    /// Enregistre la clé publique d'un autre nœud
    pub fn register_peer(&mut self, peer_id: u32, public_key: ed25519_dalek::PublicKey) {
        self.peer_public_keys.insert(peer_id, public_key);
    }
}
