//! Nœud de test pour la simulation PeerReview

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use ed25519_dalek::{SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use peerreview_protocol::journal::logger::Logger;
use peerreview_protocol::journal::entry::LogEntry;
use peerreview_protocol::types::NodeId;
use peerreview_protocol::metrics::NodeStatus;

use super::fault_injector::{FaultType, FaultInjector};

/// Message dans le réseau de test
#[derive(Debug, Clone)]
pub struct TestMessage {
    pub id: String,
    pub content: Vec<u8>,
    pub from: NodeId,
    pub to: NodeId,
    pub timestamp: Instant,
    pub hash: [u8; 32],
}

impl TestMessage {
    pub fn new(id: &str, content: &[u8], from: NodeId, to: NodeId) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(content);
        let hash: [u8; 32] = hasher.finalize().into();

        Self {
            id: id.to_string(),
            content: content.to_vec(),
            from,
            to,
            timestamp: Instant::now(),
            hash,
        }
    }
}

/// Authenticateur stocké par un témoin
#[derive(Debug, Clone)]
pub struct StoredAuthenticator {
    pub seq: u64,
    pub hash: [u8; 32],
    pub sig: [u8; 64],
    pub node_id: NodeId,
}

/// Nœud de test avec capacités PeerReview
pub struct TestNode {
    pub id: NodeId,
    pub name: String,
    pub signing_key: SigningKey,
    pub verifying_key: VerifyingKey,

    // Journal
    pub log_dir: PathBuf,
    pub logger: Option<Logger>,

    // État PeerReview
    pub status: NodeStatus,
    pub seq: u64,
    pub prev_hash: [u8; 32],

    // Messages
    pub received_messages: Vec<TestMessage>,
    pub sent_messages: Vec<TestMessage>,
    pub pending_outbox: Vec<TestMessage>,

    // Témoins et authenticateurs
    pub witnesses: Vec<NodeId>,
    pub watched_nodes: Vec<NodeId>,
    pub stored_authenticators: HashMap<NodeId, Vec<StoredAuthenticator>>,

    // Injection de fautes
    pub fault_injector: FaultInjector,

    // Statistiques
    pub bytes_sent: usize,
    pub bytes_received: usize,
    pub messages_dropped: usize,
}

impl TestNode {
    pub fn new(id: NodeId, log_dir: &PathBuf) -> Self {
        // Clé déterministe basée sur l'ID du nœud
        let mut seed = [0u8; 32];
        seed[0..4].copy_from_slice(&id.to_be_bytes());
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();

        let name = format!("node{}", id);

        Self {
            id,
            name,
            signing_key,
            verifying_key,
            log_dir: log_dir.clone(),
            logger: None,
            status: NodeStatus::Trusted,
            seq: 0,
            prev_hash: [0u8; 32],
            received_messages: Vec::new(),
            sent_messages: Vec::new(),
            pending_outbox: Vec::new(),
            witnesses: Vec::new(),
            watched_nodes: Vec::new(),
            stored_authenticators: HashMap::new(),
            fault_injector: FaultInjector::new(),
            bytes_sent: 0,
            bytes_received: 0,
            messages_dropped: 0,
        }
    }

    /// Initialise le logger du nœud
    pub fn init_logger(&mut self) -> std::io::Result<()> {
        let logger = Logger::open(self.id, &self.log_dir, self.signing_key.clone())?;
        self.logger = Some(logger);
        Ok(())
    }

    /// Retourne le chemin du fichier de log
    pub fn log_file_path(&self) -> PathBuf {
        Logger::log_path_for(self.id, &self.log_dir)
    }

    /// Injecte une faute dans ce nœud
    pub fn inject_fault(&mut self, fault: FaultType) {
        self.fault_injector.inject(fault);
    }

    /// Supprime toutes les fautes
    pub fn clear_faults(&mut self) {
        self.fault_injector.clear();
    }

    /// Simule la réception d'un message
    pub fn receive_message(&mut self, msg: TestMessage) -> Option<TestMessage> {
        self.bytes_received += msg.content.len();

        // Vérifier si on doit dropper le message (faute)
        if self.fault_injector.should_drop() {
            self.messages_dropped += 1;
            return None;
        }

        // Log RECV
        if let Some(ref mut logger) = self.logger {
            let _ = logger.log_app(
                "RECV",
                Some(msg.from),
                msg.id.clone(),
                msg.hash,
                msg.timestamp.elapsed().as_millis() as u64,
            );
        }

        // Appliquer tampering si nécessaire
        let processed_msg = if let Some(tampered) = self.fault_injector.tamper_content(&msg.content) {
            let mut new_msg = msg.clone();
            new_msg.content = tampered;
            // Recalculer le hash
            let mut hasher = Sha256::new();
            hasher.update(&new_msg.content);
            new_msg.hash = hasher.finalize().into();
            new_msg
        } else {
            msg.clone()
        };

        self.received_messages.push(processed_msg.clone());

        // Log DELIVER
        if let Some(ref mut logger) = self.logger {
            let _ = logger.log_app(
                "DELIVER",
                None,
                processed_msg.id.clone(),
                processed_msg.hash,
                processed_msg.timestamp.elapsed().as_millis() as u64,
            );
        }

        Some(processed_msg)
    }

    /// Prépare un message pour envoi (avec potentielle modification par faute)
    pub fn prepare_send(&mut self, msg_id: &str, content: &[u8], to: NodeId) -> Option<TestMessage> {
        // Vérifier si on doit dropper les messages sortants (faute silent)
        if self.fault_injector.should_drop_outgoing() {
            self.messages_dropped += 1;
            return None;
        }

        // Appliquer tampering si nécessaire
        let final_content = self.fault_injector.tamper_content(content)
            .unwrap_or_else(|| content.to_vec());

        let msg = TestMessage::new(msg_id, &final_content, self.id, to);

        // Log SEND
        if let Some(ref mut logger) = self.logger {
            let _ = logger.log_app(
                "SEND",
                Some(to),
                msg.id.clone(),
                msg.hash,
                msg.timestamp.elapsed().as_millis() as u64,
            );
        }

        self.bytes_sent += msg.content.len();
        self.sent_messages.push(msg.clone());

        Some(msg)
    }

    /// Configure les témoins de ce nœud
    pub fn set_witnesses(&mut self, witnesses: Vec<NodeId>) {
        self.witnesses = witnesses;
    }

    /// Configure les nœuds que ce nœud surveille (en tant que témoin)
    pub fn set_watched_nodes(&mut self, nodes: Vec<NodeId>) {
        self.watched_nodes = nodes;
    }

    /// Stocke un authenticateur en tant que témoin
    pub fn store_authenticator(&mut self, auth: StoredAuthenticator) {
        self.stored_authenticators
            .entry(auth.node_id)
            .or_default()
            .push(auth);
    }

    /// Vérifie si le seuil d'authenticateurs est atteint pour déclencher un challenge
    pub fn should_challenge(&self, node_id: NodeId, threshold: usize) -> bool {
        self.stored_authenticators
            .get(&node_id)
            .map(|auths| auths.len() >= threshold)
            .unwrap_or(false)
    }

    /// Marque ce nœud comme exposé
    pub fn mark_exposed(&mut self, reason: &str) {
        self.status = NodeStatus::Exposed;
        println!("[Node {}] EXPOSED: {}", self.id, reason);
    }

    /// Marque ce nœud comme suspecté
    pub fn mark_suspected(&mut self) {
        if self.status != NodeStatus::Exposed {
            self.status = NodeStatus::Suspected;
        }
    }

    /// Vérifie l'intégrité du log de ce nœud
    pub fn verify_own_log(&self, strict_chain: bool) -> std::io::Result<bool> {
        let log_file = self.log_file_path();
        Logger::verify_log_file(&log_file, &self.verifying_key, strict_chain)
            .map(|_| true)
            .or_else(|e| {
                println!("[Node {}] Log verification failed: {}", self.id, e);
                Ok(false)
            })
    }

    /// Crée un fork de log (pour test 4)
    pub fn create_forked_log(&mut self, branch_a: &str, branch_b: &str,
                             targets_a: &[NodeId], targets_b: &[NodeId]) {
        self.fault_injector.inject(FaultType::ForkLog {
            branch_a_content: branch_a.to_string(),
            branch_b_content: branch_b.to_string(),
            branch_a_targets: targets_a.to_vec(),
            branch_b_targets: targets_b.to_vec(),
        });
    }

    /// Retourne le contenu du message basé sur le fork si applicable
    pub fn get_forked_content(&self, original: &[u8], target: NodeId) -> Vec<u8> {
        self.fault_injector.get_forked_content(original, target)
    }

    /// Nombre total de messages reçus
    pub fn received_count(&self) -> usize {
        self.received_messages.len()
    }

    /// Nombre de messages uniques reçus (par ID)
    pub fn unique_received_count(&self) -> usize {
        let ids: HashSet<_> = self.received_messages.iter().map(|m| &m.id).collect();
        ids.len()
    }
}

impl Drop for TestNode {
    fn drop(&mut self) {
        // Cleanup: supprimer le fichier de log temporaire
        let log_file = self.log_file_path();
        if log_file.exists() {
            let _ = std::fs::remove_file(&log_file);
        }
    }
}
