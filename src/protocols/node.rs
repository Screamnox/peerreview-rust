use ed25519_dalek::Keypair;
use std::collections::HashMap;

use crate::{journal::Logger, protocols::audit::Snapshot};

/// Type de message : Send ou Recv
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MessageType {
    Send,
    Recv,
}

/// Structure d'un message PeerReview transmis sur le réseau
#[derive(Debug, Clone)]
pub struct PeerReviewMessage {
    pub msg_type: MessageType,
    pub seq_num: usize,
    pub signature: [u8; 64],
    pub prev_hash: [u8; 32],
    pub dest: u32,
    pub payload: String,
}

/// Authenticator stocké par un témoin
#[derive(Debug, Clone)]
pub struct StoredAuthenticator {
    pub node_id: u32,        // Nœud surveillé
    pub seq_num: usize,      // Numéro de séquence
    pub signature: [u8; 64], // Signature Ed25519
}

/// État de détection d'un nœud (Algorithm 15)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetectionState {
    Trusted,   // État par défaut: nœud correct/de confiance
    Suspected, // Nœud suspect (challenge en attente)
    Exposed,   // Nœud définitivement fautif
}

/// Challenge en attente pour un nœud suspect
#[derive(Debug, Clone)]
pub struct PendingChallenge {
    pub target_node_id: u32,
    pub seq_nums: Vec<usize>,
}

/// Structure d'un nœud PeerReview
pub struct PeerReviewNode {
    pub node_id: u32,
    pub logger: Logger,
    pub keypair: Keypair,
    pub peer_public_keys: HashMap<u32, ed25519_dalek::PublicKey>,
    /// Configuration des témoins : HashMap<node_id, Vec<witness_ids>>
    /// Tous les nœuds connaissent les témoins de tous les autres nœuds
    pub witnesses_map: HashMap<u32, Vec<u32>>,
    /// Authenticators stockés en tant que témoin : HashMap<node_id_surveillé, Vec<authenticators>>
    pub stored_authenticators: HashMap<u32, Vec<StoredAuthenticator>>,
    /// Seuil d'authenticators avant de challenger (par défaut: 10)
    pub challenge_threshold: usize,
    /// Nœuds marqués comme EXPOSED (fautifs)
    pub exposed_nodes: Vec<u32>,
    /// États de détection pour chaque nœud (Algorithm 15)
    pub detection_states: HashMap<u32, DetectionState>,
    /// Challenges en attente pour les nœuds suspects
    pub pending_challenges: HashMap<u32, Vec<PendingChallenge>>,
    /// Liste des snapchot des témoins
    pub snapchot_list_witness: HashMap<u32, Vec<Snapshot>>,
}

impl PeerReviewNode {
    /// Crée un nouveau nœud PeerReview avec la configuration des témoins et les clés publiques
    pub fn new(
        node_id: u32,
        logger: Logger,
        keypair: Keypair,
        witnesses_map: HashMap<u32, Vec<u32>>,
        peer_public_keys: HashMap<u32, ed25519_dalek::PublicKey>,
    ) -> Self {
        Self {
            node_id,
            logger,
            keypair,
            peer_public_keys,
            witnesses_map,
            stored_authenticators: HashMap::new(),
            challenge_threshold: 10,
            exposed_nodes: Vec::new(),
            detection_states: HashMap::new(),
            pending_challenges: HashMap::new(),
            snapchot_list_witness: HashMap::new(),
        }
    }

    /// Retourne les témoins d'un nœud donné
    pub fn get_witnesses(&self, node_id: u32) -> Vec<u32> {
        self.witnesses_map
            .get(&node_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Envoie un authenticator (signature) aux témoins d'un nœud
    /// C'est appelé quand on reçoit un message d'un autre nœud
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
            // TODO: Implémenter l'envoi réseau réel de l'authenticator au témoin
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
        let auth = StoredAuthenticator {
            node_id: observed_node_id,
            seq_num,
            signature,
        };

        self.stored_authenticators
            .entry(observed_node_id)
            .or_insert_with(Vec::new)
            .push(auth);

        let count = self
            .stored_authenticators
            .get(&observed_node_id)
            .unwrap()
            .len();
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
