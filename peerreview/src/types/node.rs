use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use ed25519_dalek::{Keypair, PublicKey};
use serde::{Serialize, Deserialize};

use crate::types::messages::PeerReviewMsg;

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
    pub keypair: Keypair,
    // logger sera ajouté plus tard par l'équipe PR
    // pub logger: Logger,
    pub peers: HashMap<u32, PeerInfo>,
    pub witnesses: Vec<u32>,
}

impl Node {
    pub fn new(id: u32, keypair: Keypair) -> Self {
        Self {
            id,
            keypair,
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

    /// Exemple : envoyer un PeerReviewMsg à un peer donné
    pub fn send_to_peer(
        &self,
        peer_id: u32,
        msg: &PeerReviewMsg,
        net: &crate::network::tcp::NetworkLayer,
    ) -> std::io::Result<()> {
        if let Some(peer) = self.peers.get(&peer_id) {
            let mut guard = peer.socket.lock().unwrap();
            net.send_message(&mut guard, msg)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("peer {} not found", peer_id),
            ))
        }
    }
}
