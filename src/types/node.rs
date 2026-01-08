use bincode::{Decode, Encode};
use ed25519_dalek::{Keypair, PublicKey};
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use crate::journal::Logger;
use super::messages::Authenticator;

pub type NodeId = u32;

/// Status d'un noeud pair
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum PeerStatus {
    Trusted,
    Suspected,
    Exposed,
}

/// Authenticator stocké par un témoin (extension de Authenticator)
/// Inclut l'ID du nœud surveillé pour la gestion des témoins
#[derive(Debug, Clone)]
pub struct StoredAuthenticator {
    pub node_id: u32,           // Nœud surveillé
    pub authenticator: Authenticator,  // Authenticator de base (seq, hash, sig)
}

impl StoredAuthenticator {
    /// Crée un nouveau StoredAuthenticator à partir d'un Authenticator
    pub fn new(node_id: u32, authenticator: Authenticator) -> Self {
        Self { node_id, authenticator }
    }
    
    /// Crée directement depuis les composants
    pub fn from_components(node_id: u32, seq_num: usize, hash: [u8; 32], signature: [u8; 64]) -> Self {
        Self {
            node_id,
            authenticator: Authenticator {
                seq: seq_num,
                hash,
                sig: signature,
            }
        }
    }
    
    /// Accesseurs pour rétro-compatibilité
    pub fn seq_num(&self) -> usize {
        self.authenticator.seq
    }
    
    pub fn signature(&self) -> [u8; 64] {
        self.authenticator.sig
    }
}

/// Challenge en attente pour un nœud suspect
#[derive(Debug, Clone)]
pub struct PendingChallenge {
    pub target_node_id: u32,
    pub seq_nums: Vec<usize>,
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
/// - Authenticators stockés (en tant que témoin)
/// - États de détection des autres nœuds
/// - Challenges en attente
pub struct Node {
    pub id: NodeId,
    pub node_id: NodeId,  // Alias pour compatibilité
    pub keypair: Keypair,
    pub logger: Logger,
    pub peers: HashMap<NodeId, PeerInfo>,
    pub witnesses: Vec<NodeId>,
    
    // Champs spécifiques au protocole PeerReview
    pub peer_public_keys: HashMap<u32, PublicKey>,
    pub witnesses_map: HashMap<u32, Vec<u32>>,
    pub stored_authenticators: HashMap<u32, Vec<StoredAuthenticator>>,
    pub challenge_threshold: usize,
    pub exposed_nodes: Vec<u32>,
    pub detection_states: HashMap<u32, PeerStatus>,
    pub pending_challenges: HashMap<u32, Vec<PendingChallenge>>,
    pub snapchot_list_witness: HashMap<u32, Vec<crate::protocols::audit::Snapchot>>,
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
            node_id: id,
            keypair,
            logger,
            peers: HashMap::new(),
            witnesses,
            peer_public_keys: HashMap::new(),
            witnesses_map: HashMap::new(),
            stored_authenticators: HashMap::new(),
            challenge_threshold: 5,
            exposed_nodes: Vec::new(),
            detection_states: HashMap::new(),
            pending_challenges: HashMap::new(),
            snapchot_list_witness: HashMap::new(),
        }
    }
    
    /// Crée un nouveau nœud PeerReview avec configuration complète (pour les protocoles)
    pub fn new_peerreview(
        node_id: u32,
        logger: Logger,
        keypair: Keypair,
        witnesses_map: HashMap<u32, Vec<u32>>,
        peer_public_keys: HashMap<u32, PublicKey>,
    ) -> Self {
        Self {
            id: node_id,
            node_id,
            keypair,
            logger,
            peers: HashMap::new(),
            witnesses: witnesses_map.get(&node_id).cloned().unwrap_or_default(),
            peer_public_keys,
            witnesses_map,
            stored_authenticators: HashMap::new(),
            challenge_threshold: 5,
            exposed_nodes: Vec::new(),
            detection_states: HashMap::new(),
            pending_challenges: HashMap::new(),
            snapchot_list_witness: HashMap::new(),
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
    
    // === Méthodes spécifiques au protocole PeerReview ===
    
    /// Retourne les témoins d'un nœud donné
    pub fn get_witnesses(&self, node_id: u32) -> Vec<u32> {
        self.witnesses_map
            .get(&node_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Envoie un authenticator (signature) aux témoins d'un nœud
    pub fn send_authenticator_to_witnesses(
        &self,
        observed_node_id: u32,
        seq_num: usize,
        signature: [u8; 64],
    ) -> std::io::Result<()> {
        let witnesses = self.get_witnesses(observed_node_id);

        if witnesses.is_empty() {
            println!(
                "[Nœud {}] Aucun témoin configuré pour le nœud {}",
                self.node_id, observed_node_id
            );
            return Ok(());
        }

        println!(
            "[Nœud {}] Envoi de l'authenticator (seq={}) du nœud {} aux {} témoin(s)",
            self.node_id,
            seq_num,
            observed_node_id,
            witnesses.len()
        );

        for witness_id in &witnesses {
            println!(
                "[Nœud {}] → Témoin {} : authenticator seq={} sig={}",
                self.node_id,
                witness_id,
                seq_num,
                hex::encode(&signature[..8])
            );
        }

        Ok(())
    }

    /// Stocke un authenticator reçu en tant que témoin
    pub fn store_authenticator(
        &mut self,
        observed_node_id: u32,
        seq_num: usize,
        signature: [u8; 64],
    ) {
        let hash = [0u8; 32];
        
        let auth = StoredAuthenticator::from_components(
            observed_node_id,
            seq_num,
            hash,
            signature,
        );

        self.stored_authenticators
            .entry(observed_node_id)
            .or_insert_with(Vec::new)
            .push(auth);

        let count = self.stored_authenticators.get(&observed_node_id).unwrap().len();
        println!(
            "[Témoin {}] Authenticator stocké pour nœud {} (seq={}). Total: {}",
            self.node_id, observed_node_id, seq_num, count
        );
    }

    /// Vérifie si le seuil d'authenticators est atteint pour challenger un nœud
    pub fn should_challenge(&self, observed_node_id: u32) -> bool {
        if let Some(auths) = self.stored_authenticators.get(&observed_node_id) {
            auths.len() >= self.challenge_threshold
        } else {
            false
        }
    }
}
