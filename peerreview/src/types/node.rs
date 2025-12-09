use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Clé publique stub (32 octets). Pourra être remplacée par une vraie clé plus tard.
pub type PublicKey = [u8; 32];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PeerStatus {
    Trusted,
    Suspected,
    Exposed,
}

#[derive(Debug)]
pub struct PeerInfo {
    pub id: u32,
    pub public_key: PublicKey,
    pub socket: Arc<Mutex<TcpStream>>,
    pub status: PeerStatus,
}

pub struct Node {
    pub id: u32,
    /// Stub pour la clé privée/clé publique. L'équipe PR pourra remplacer ça par un vrai keypair.
    pub keypair_bytes: Vec<u8>,
    pub peers: HashMap<u32, PeerInfo>,
    pub witnesses: Vec<u32>,
}

impl Node {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            keypair_bytes: Vec::new(),
            peers: HashMap::new(),
            witnesses: Vec::new(),
        }
    }

    pub fn add_peer(&mut self, info: PeerInfo) {
        self.peers.insert(info.id, info);
    }

    pub fn get_peer(&self, id: u32) -> Option<&PeerInfo> {
        self.peers.get(&id)
    }

    pub fn get_peer_mut(&mut self, id: u32) -> Option<&mut PeerInfo> {
        self.peers.get_mut(&id)
    }
}
