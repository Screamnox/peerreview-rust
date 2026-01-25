use bincode::{Decode, Encode};
use ed25519_dalek::{Keypair, PublicKey};
use std::collections::HashMap;

use crate::{
    journal::Logger,
    network::NetworkLayer,
    protocols::audit::Snapshot,
    types::{
        Challenge, PeerReviewMsg, Proof,
        messages::{Authenticator, ChallengeId, ChallengeKey},
    },
};

/// Seuil d'authenticators avant de challenger
const CHALLENGE_THRESHOLD: usize = 10;

pub type NodeId = u32;

/// Status d'un noeud pair
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum PeerStatus {
    Trusted,
    Suspected,
    Exposed,
}

/// Information sur un noeud pair
#[derive(Clone)]
pub struct PeerInfo {
    pub id: NodeId,
    pub public_key: PublicKey,
    pub status: PeerStatus,
    pub witnesses: Vec<NodeId>,
    pub challenges: HashMap<ChallengeKey, Challenge>, // TODO maybe just vec
    pub proofs: Vec<Proof>,
    pub last_audit_seq: usize,
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

    pub network_layer: NetworkLayer,

    pub next_challenge_id: ChallengeId,

    /// Témoins
    pub stored_authenticators: HashMap<NodeId, Vec<Authenticator>>,
    pub snapshots: HashMap<NodeId, Vec<Snapshot>>,
}

impl Node {
    /// Crée un nouveau noeud PeerReview
    ///
    /// # Arguments
    /// * `id` - Identifiant du noeud
    /// * `keypair` - Paire de clés Ed25519 pour les signatures
    /// * `logger` - Journal initialisé
    /// * `witnesses` - Liste des IDs des témoins (doit être un sous-ensemble des peers)
    pub fn new(
        id: NodeId,
        keypair: Keypair,
        logger: Logger,
        witnesses: Vec<NodeId>,
        network_layer: NetworkLayer,
    ) -> Self {
        Self {
            id,
            keypair,
            logger, // TODO: Ouverture via le path
            peers: HashMap::new(),
            witnesses,
            network_layer,
            next_challenge_id: 0,
            stored_authenticators: HashMap::new(),
            snapshots: HashMap::new(),
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

    /// Récupère les témoins d'un noeud
    pub fn get_witnesses(&self, peer_id: NodeId) -> Vec<NodeId> {
        let witnesses = if peer_id == self.id {
            self.witnesses.clone()
        } else {
            self.peers
                .get(&peer_id)
                .map(|p| p.witnesses.clone())
                .unwrap_or_default()
        };

        // Filter out self - a node cannot be its own witness
        // SHOULD NOT happen
        witnesses.into_iter().filter(|&w| w != self.id).collect()
    }

    /// Ajoute un pair connu après connexion TCP
    ///
    /// # Arguments
    /// * `id` - ID du pair
    /// * `public_key` - Clé publique Ed25519 du pair
    /// * `socket` - Socket TCP connectée
    pub fn add_peer(&mut self, id: NodeId, public_key: PublicKey, witnesses: Vec<NodeId>) {
        self.peers.insert(
            id,
            PeerInfo {
                id,
                public_key,
                status: PeerStatus::Trusted,
                witnesses,
                challenges: HashMap::new(),
                proofs: Vec::new(),
                last_audit_seq: 1,
            },
        );
    }

    /// Récupère la clé publique d'un pair (pour vérifications cryptographiques)
    pub fn get_peer_public_key(&self, peer_id: NodeId) -> Option<&PublicKey> {
        self.peers.get(&peer_id).map(|p| &p.public_key)
    }

    /// Récupère le statut d'un pair
    pub fn get_peer_status(&self, peer_id: NodeId) -> Option<PeerStatus> {
        self.peers.get(&peer_id).map(|p| p.status)
    }

    /// Récupère la dernière séquence d'audit d'un pair
    pub fn get_peer_last_audit_seq(&self, peer_id: NodeId) -> Option<usize> {
        self.peers.get(&peer_id).map(|p| p.last_audit_seq)
    }

    /// Modifie la dernière séquence d'audit d'un pair
    pub fn set_peer_last_audit_seq(&mut self, peer_id: NodeId, seq: usize) {
        if let Some(peer) = self.peers.get_mut(&peer_id) {
            peer.last_audit_seq = seq;
        }
    }

    /// Modifie le statut d'un pair
    pub fn set_peer_status(&mut self, peer_id: NodeId, status: PeerStatus) {
        if let Some(peer) = self.peers.get_mut(&peer_id) {
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

    /// Génrer un challenge ID unique (localement ; pour ce noeud)
    pub fn generate_challenge_id(&mut self) -> ChallengeId {
        let id = self.next_challenge_id;
        self.next_challenge_id += 1;
        id
    }

    /// Récupérer tous les challenges concernant un pair
    pub fn get_challenges(&self, peer_id: NodeId) -> Option<Vec<Challenge>> {
        self.peers
            .get(&peer_id)
            .map(|peer| peer.challenges.values().cloned().collect())
    }

    /// Récupérer un challenge spécifique concernant un pair
    pub fn get_challenge(&self, peer_id: NodeId, key: ChallengeKey) -> Option<Challenge> {
        self.peers
            .get(&peer_id)
            .and_then(|peer| peer.challenges.get(&key).cloned())
    }

    /// Ajouter un challenge
    pub fn add_challenge(&mut self, challenge: Challenge) {
        if let Some(peer) = self.peers.get_mut(&challenge.target) {
            let key = challenge.key();

            // Check if challenge with same key already exists
            if peer.challenges.contains_key(&key) {
                println!(
                    "[Node {}] Warning: Challenge ({}, {}) for peer {} already exists, replacing",
                    self.id, key.0, key.1, challenge.target
                );
            }

            peer.challenges.insert(key, challenge);
        } else {
            println!(
                "[Node {}] Warning: Cannot add challenge for unknown peer {}",
                self.id, challenge.target
            );
        }
    }

    /// Supprimer un challenge spécifique
    pub fn remove_challenge(&mut self, peer_id: NodeId, key: ChallengeKey) -> Option<Challenge> {
        self.peers.get_mut(&peer_id).and_then(|peer| {
            let removed = peer.challenges.remove(&key);
            if removed.is_some() {
                println!(
                    "[Node {}] Removed challenge ({}, {}) for peer {} (remaining: {})",
                    self.id,
                    key.0,
                    key.1,
                    peer_id,
                    peer.challenges.len()
                );
            }
            removed
        })
    }

    /// Supprimer l'ensemble des challenges sur une cible
    pub fn remove_all_challenges(&mut self, peer_id: NodeId) -> usize {
        self.peers
            .get_mut(&peer_id)
            .map(|peer| {
                let count = peer.challenges.len();
                peer.challenges.clear();
                println!(
                    "[Node {}] Cleared {} challenges for peer {}",
                    self.id, count, peer_id
                );
                count
            })
            .unwrap_or(0)
    }

    /// Récupérer le nombre de challenge sur une cible
    pub fn get_challenge_count(&self, peer_id: NodeId) -> usize {
        self.peers
            .get(&peer_id)
            .map(|peer| peer.challenges.len())
            .unwrap_or(0)
    }

    pub fn get_proofs(&self, peer_id: NodeId) -> Option<Vec<Proof>> {
        self.peers.get(&peer_id).map(|peer| peer.proofs.clone())
    }

    pub fn add_proof(&mut self, peer_id: &NodeId, proof: Proof) {
        match self.peers.get_mut(peer_id) {
            Some(peer) => {
                peer.proofs.push(proof);
            }
            None => {
                println!(
                    "[Node {}] Warning: Cannot add proof for unknown peer {}",
                    self.id, peer_id
                );
            }
        }
    }

    /// Stores an authenticator for a peer and checks if challenge threshold is exceeded
    /// Returns true if threshold is exceeded and a challenge should be sent
    /// (send_consistency_challenge)
    pub fn store_authenticator(&mut self, peer_id: NodeId, auth: Authenticator) -> bool {
        let auths = self
            .stored_authenticators
            .entry(peer_id)
            .or_insert_with(Vec::new);

        auths.push(auth);

        auths.len() >= CHALLENGE_THRESHOLD
    }

    /// Clears authenticators for a node
    /// Note: Called after a successful consistency verification
    /// Note: If a fault is detected, authenticators are kept as PROOFS
    /// Returns the number of cleared authenticators
    pub fn clear_authenticators(&mut self, peer_id: NodeId) -> usize {
        if let Some(auths) = self.stored_authenticators.get_mut(&peer_id) {
            let count = auths.len();
            auths.clear();
            count
        } else {
            0
        }
    }

    /// Gets the current count of stored authenticators for a peer
    pub fn get_authenticator_count(&self, peer_id: NodeId) -> usize {
        self.stored_authenticators
            .get(&peer_id)
            .map(|auths| auths.len())
            .unwrap_or(0)
    }

    /// Gets a reference to stored authenticators for a peer
    pub fn get_stored_authenticators(&self, peer_id: NodeId) -> Option<&Vec<Authenticator>> {
        self.stored_authenticators.get(&peer_id)
    }

    // TODO
    pub fn send(&self, peer_id: NodeId, msg: PeerReviewMsg) -> std::io::Result<()> {
        self.network_layer.send(peer_id, &msg)
    }

    /// Note: Blocking call
    pub fn recv(&self) -> std::io::Result<(NodeId, PeerReviewMsg)> {
        self.network_layer.recv()
    }

    pub fn send_to_witnesses(
        &mut self,
        peer_id: NodeId,
        pr_msg: PeerReviewMsg,
    ) -> std::io::Result<()> {
        for witness_id in self.get_witnesses(peer_id) {
            self.send(witness_id, pr_msg.clone())?;
        }

        Ok(())
    }
}
