use bincode::{Decode, Encode};
use ed25519_dalek::{Keypair, PublicKey};
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use crate::journal::Logger;

pub type NodeId = u32;

/// Status d'un noeud pair
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum PeerStatus {
    Trusted,
    Suspected,
    Exposed,
}

/// Information sur un noeud pair
pub struct PeerInfo {
    pub id: NodeId,
    pub public_key: PublicKey,
    pub socket: Arc<Mutex<TcpStream>>,
    pub status: PeerStatus,
    pub witnesses: Vec<NodeId>,
}

/// Noeud PeerReview
///
/// Contient toutes les informations nécessaires pour participer au protocole :
/// - Identité cryptographique (keypair)
/// - Journal local (logger)
/// - Connexions réseau (peers)
/// - Liste des témoins (witnesses)
pub struct Node {
    pub id: NodeId,
    pub keypair: Keypair,
    pub logger: Logger,
    pub peers: HashMap<NodeId, PeerInfo>,
    pub witnesses: Vec<NodeId>,
}

impl Node {
    /// Crée un nouveau noeud PeerReview
    ///
    /// # Arguments
    /// * `id` - Identifiant du noeud
    /// * `keypair` - Paire de clés Ed25519 pour les signatures
    /// * `logger` - Journal initialisé
    /// * `witnesses` - Liste des IDs des témoins (doit être un sous-ensemble des peers)
    pub fn new(id: NodeId, keypair: Keypair, logger: Logger, witnesses: Vec<NodeId>) -> Self {
        Self {
            id,
            keypair,
            logger, // TODO: Ouverture via le path
            peers: HashMap::new(),
            witnesses,
        }
    }

    /// Récupère la clé publique de ce noeud
    pub fn get_public_key(&self) -> &PublicKey {
        &self.keypair.public
    }

    /// Récupère le dernier hash du logger de ce noeud
    pub fn get_last_hash(&self) -> [u8; 32] {
        self.logger.get_current_hash()
    }

    /// Ajoute un pair connu après connexion TCP
    ///
    /// # Arguments
    /// * `id` - ID du pair
    /// * `public_key` - Clé publique Ed25519 du pair
    /// * `socket` - Socket TCP connectée
    pub fn add_peer(
        &mut self,
        id: NodeId,
        public_key: PublicKey,
        socket: TcpStream,
        witnesses: Vec<NodeId>,
    ) {
        self.peers.insert(
            id,
            PeerInfo {
                id,
                public_key,
                socket: Arc::new(Mutex::new(socket)),
                status: PeerStatus::Trusted,
                witnesses,
            },
        );
        println!("[Noeud {}] Pair {} ajouté (statut: Trusted)", self.id, id);
    }

    /// Récupère la clé publique d'un pair (pour vérifications cryptographiques)
    pub fn get_peer_public_key(&self, peer_id: NodeId) -> Option<&PublicKey> {
        self.peers.get(&peer_id).map(|p| &p.public_key)
    }

    /// Récupère la socket TCP d'un pair
    pub fn get_peer_socket(&self, peer_id: NodeId) -> Option<Arc<Mutex<TcpStream>>> {
        self.peers.get(&peer_id).map(|p| Arc::clone(&p.socket))
    }

    /// Récupère le statut d'un pair
    pub fn get_peer_status(&self, peer_id: NodeId) -> Option<PeerStatus> {
        self.peers.get(&peer_id).map(|p| p.status)
    }

    /// Modifie le statut d'un pair
    pub fn set_peer_status(&mut self, peer_id: NodeId, status: PeerStatus) {
        if let Some(peer) = self.peers.get_mut(&peer_id) {
            println!(
                "[Noeud {}] Statut du pair {} modifié: {:?} -> {:?}",
                self.id, peer_id, peer.status, status
            );
            peer.status = status;
        }
    }

    /// Vérifie si un pair est un témoin de ce noeud
    pub fn is_witness(&self, peer_id: NodeId) -> bool {
        self.witnesses.contains(&peer_id)
    }

    /// Récupère tous les pairs ayant un statut spécifique
    pub fn get_peers_by_status(&self, status: PeerStatus) -> Vec<NodeId> {
        self.peers
            .iter()
            .filter(|(_, info)| info.status == status)
            .map(|(id, _)| *id)
            .collect()
    }
}
