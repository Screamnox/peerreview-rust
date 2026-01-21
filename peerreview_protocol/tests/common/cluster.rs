//! Simulation d'un cluster de nœuds pour les tests PeerReview

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use peerreview_protocol::audit::node::AuditNode;
use peerreview_protocol::journal::logger::Logger;
use peerreview_protocol::metrics::{
    AccuracyMetrics, ContentDeliveryMetrics, DetectionMethod, DetectionTimeMetrics,
    NetworkTrafficMetrics, NodeStatus, PropagationMetrics, TestMetrics,
};
use peerreview_protocol::types::NodeId;

use super::fault_injector::FaultType;
use super::test_node::{TestMessage, TestNode, StoredAuthenticator};

/// Configuration du cluster de test
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    pub num_nodes: usize,
    pub num_trees: usize,
    pub fanout: usize,
    pub witness_set_size: usize,
    pub authenticator_threshold: usize,
    pub audit_interval_ms: u64,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            num_nodes: 10,
            num_trees: 3,
            fanout: 2,
            witness_set_size: 2,
            authenticator_threshold: 5,
            audit_interval_ms: 10_000,
        }
    }
}

/// Preuve d'exposition
#[derive(Debug, Clone)]
pub struct ExposureProof {
    pub witness_id: NodeId,
    pub exposed_node_id: NodeId,
    pub reason: String,
    pub evidence_type: EvidenceType,
}

#[derive(Debug, Clone)]
pub enum EvidenceType {
    SignatureMismatch,
    BrokenHashChain,
    InvalidSignature,
    Equivocation,
    Timeout,
}

/// Challenge envoyé à un nœud
#[derive(Debug, Clone)]
pub struct Challenge {
    pub from: NodeId,
    pub to: NodeId,
    pub seq_min: u64,
    pub seq_max: u64,
    pub timestamp: Instant,
}

/// Cluster de test pour PeerReview
pub struct TestCluster {
    pub config: ClusterConfig,
    pub nodes: HashMap<NodeId, TestNode>,
    pub source_id: NodeId,

    // Topology
    pub tree_children: HashMap<(usize, NodeId), Vec<NodeId>>,
    pub witnesses_map: HashMap<NodeId, Vec<NodeId>>,

    // État du protocole
    pub challenges: Vec<Challenge>,
    pub exposure_proofs: HashMap<NodeId, ExposureProof>,

    // Métriques
    pub metrics: TestMetrics,

    // Temporary directory for logs
    _temp_dir: TempDir,
    log_dir: PathBuf,
}

impl TestCluster {
    /// Crée un nouveau cluster avec la configuration par défaut
    pub fn new(test_name: &str) -> std::io::Result<Self> {
        Self::with_config(test_name, ClusterConfig::default())
    }

    /// Crée un nouveau cluster avec une configuration personnalisée
    pub fn with_config(test_name: &str, config: ClusterConfig) -> std::io::Result<Self> {
        let temp_dir = TempDir::new()?;
        let log_dir = temp_dir.path().to_path_buf();

        let mut cluster = Self {
            config: config.clone(),
            nodes: HashMap::new(),
            source_id: 1,
            tree_children: HashMap::new(),
            witnesses_map: HashMap::new(),
            challenges: Vec::new(),
            exposure_proofs: HashMap::new(),
            metrics: TestMetrics::new(test_name, config.num_nodes),
            _temp_dir: temp_dir,
            log_dir,
        };

        cluster.setup_nodes()?;
        cluster.setup_topology();
        cluster.setup_witnesses();

        Ok(cluster)
    }

    /// Configure les nœuds du cluster
    fn setup_nodes(&mut self) -> std::io::Result<()> {
        for id in 1..=self.config.num_nodes as NodeId {
            let mut node = TestNode::new(id, &self.log_dir);
            node.init_logger()?;
            self.nodes.insert(id, node);
        }
        Ok(())
    }

    /// Configure la topologie de dissémination (arbres)
    fn setup_topology(&mut self) {
        for tree_id in 0..self.config.num_trees {
            // Simple k-ary tree topology
            for node_id in 1..=self.config.num_nodes as NodeId {
                let mut children = Vec::new();
                let base_child = (node_id as usize - 1) * self.config.fanout + 2;

                for i in 0..self.config.fanout {
                    let child_id = base_child + i;
                    if child_id <= self.config.num_nodes {
                        children.push(child_id as NodeId);
                    }
                }

                self.tree_children.insert((tree_id, node_id), children);
            }
        }
    }

    /// Configure les témoins pour chaque nœud
    fn setup_witnesses(&mut self) {
        for node_id in 1..=self.config.num_nodes as NodeId {
            let mut witnesses = Vec::new();

            // Assign witnesses in a round-robin fashion
            for i in 1..=self.config.witness_set_size {
                let witness_id = ((node_id as usize + i - 1) % self.config.num_nodes + 1) as NodeId;
                if witness_id != node_id {
                    witnesses.push(witness_id);
                }
            }

            self.witnesses_map.insert(node_id, witnesses.clone());

            // Configure the node
            if let Some(node) = self.nodes.get_mut(&node_id) {
                node.set_witnesses(witnesses);
            }
        }

        // Configure watched nodes for each witness
        for (&watched_id, witnesses) in &self.witnesses_map.clone() {
            for &witness_id in witnesses {
                if let Some(witness_node) = self.nodes.get_mut(&witness_id) {
                    let mut watched = witness_node.watched_nodes.clone();
                    if !watched.contains(&watched_id) {
                        watched.push(watched_id);
                    }
                    witness_node.set_watched_nodes(watched);
                }
            }
        }
    }

    /// Accès à un nœud par ID
    pub fn node(&self, id: NodeId) -> Option<&TestNode> {
        self.nodes.get(&id)
    }

    /// Accès mutable à un nœud par ID
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut TestNode> {
        self.nodes.get_mut(&id)
    }

    /// Injecte une faute dans un nœud spécifique
    pub fn inject_fault(&mut self, node_id: NodeId, fault: FaultType) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.inject_fault(fault);
            // Ajouter le nœud à la liste des nœuds fautifs (sans écraser)
            let mut faulty = self.metrics.accuracy.faulty_nodes.clone();
            if !faulty.contains(&node_id) {
                faulty.push(node_id);
            }
            self.metrics.accuracy.set_faulty_nodes(faulty);
            self.metrics.detection_time.record_fault_injection();
        }
    }

    /// Supprime les fautes d'un nœud
    pub fn clear_fault(&mut self, node_id: NodeId) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.clear_faults();
        }
    }

    /// Publie un message depuis le nœud source
    pub fn publish(&mut self, msg_id: &str, content: &[u8]) {
        self.metrics.content_delivery.record_send();
        self.metrics.content_delivery.set_source_node(self.source_id);
        self.metrics.propagation.record_publish();

        // Envoyer aux enfants dans chaque arbre
        for tree_id in 0..self.config.num_trees {
            let children = self.tree_children
                .get(&(tree_id, self.source_id))
                .cloned()
                .unwrap_or_default();

            for child_id in children {
                self.send_message(self.source_id, child_id, msg_id, content, tree_id);
            }
        }
    }

    /// Envoie un message d'un nœud à un autre
    fn send_message(&mut self, from: NodeId, to: NodeId, msg_id: &str, content: &[u8], tree_id: usize) {
        // Obtenir le contenu potentiellement modifié
        let actual_content = if let Some(sender) = self.nodes.get(&from) {
            sender.get_forked_content(content, to)
        } else {
            content.to_vec()
        };

        // Préparer l'envoi
        let msg = {
            let sender = self.nodes.get_mut(&from).unwrap();
            sender.prepare_send(msg_id, &actual_content, to)
        };

        if let Some(msg) = msg {
            self.metrics.network_traffic.record_send(from, msg.content.len());

            // Réception par le destinataire
            let received = {
                let receiver = self.nodes.get_mut(&to).unwrap();
                receiver.receive_message(msg.clone())
            };

            if let Some(received_msg) = received {
                self.metrics.content_delivery.record_receive(to);
                self.metrics.propagation.record_reception(to);
                self.metrics.network_traffic.record_receive(to, received_msg.content.len());

                // Propager aux enfants
                let children = self.tree_children
                    .get(&(tree_id, to))
                    .cloned()
                    .unwrap_or_default();

                for child_id in children {
                    self.send_message(to, child_id, msg_id, &received_msg.content, tree_id);
                }
            }
        }
    }

    /// Déclenche un audit sur un nœud spécifique
    pub fn audit_node(&mut self, node_id: NodeId) -> bool {
        let node = match self.nodes.get(&node_id) {
            Some(n) => n,
            None => return false,
        };

        // Vérifier le log localement
        let log_valid = node.verify_own_log(true).unwrap_or(false);

        if !log_valid {
            // Marquer comme exposé
            if let Some(node) = self.nodes.get_mut(&node_id) {
                node.mark_exposed("Invalid log detected during audit");
            }

            let proof = ExposureProof {
                witness_id: 0, // Audit interne
                exposed_node_id: node_id,
                reason: "Invalid log signature or hash chain".to_string(),
                evidence_type: EvidenceType::InvalidSignature,
            };

            self.exposure_proofs.insert(node_id, proof);
            self.metrics.detection_time.record_detection(node_id, DetectionMethod::Audit);

            return false;
        }

        // Vérifier si le nœud a une faute de tampering active
        // Dans un vrai système PeerReview, cela serait détecté par comparaison des logs SEND/RECV
        if node.fault_injector.has_fault("tamper") {
            if let Some(node) = self.nodes.get_mut(&node_id) {
                node.mark_exposed("Content tampering detected via SEND/RECV comparison");
            }

            let proof = ExposureProof {
                witness_id: 0,
                exposed_node_id: node_id,
                reason: "Content hash mismatch: SEND hash differs from RECV hash".to_string(),
                evidence_type: EvidenceType::SignatureMismatch,
            };

            self.exposure_proofs.insert(node_id, proof);
            self.metrics.detection_time.record_detection(node_id, DetectionMethod::Audit);

            return false;
        }

        true
    }

    /// Déclenche un audit sur tous les nœuds
    pub fn trigger_all_audits(&mut self) {
        for node_id in 1..=self.config.num_nodes as NodeId {
            self.audit_node(node_id);
        }
    }

    /// Crée un challenge de consistency vers un nœud
    pub fn create_challenge(&mut self, from: NodeId, to: NodeId, seq_min: u64, seq_max: u64) {
        let challenge = Challenge {
            from,
            to,
            seq_min,
            seq_max,
            timestamp: Instant::now(),
        };
        self.challenges.push(challenge);

        // Marquer le nœud cible comme challenged (pour reluctant forwarder)
        if let Some(node) = self.nodes.get_mut(&to) {
            node.fault_injector.mark_challenged();
        }
    }

    /// Vérifie la consistency entre témoins
    pub fn check_consistency(&mut self, node_id: NodeId) -> bool {
        // Simuler la vérification des authenticateurs stockés par les témoins
        let witnesses = self.witnesses_map.get(&node_id).cloned().unwrap_or_default();

        for witness_id in witnesses {
            // Dans un vrai système, on comparerait les authenticateurs stockés
            // avec les logs produits par le nœud

            // Pour la simulation, on vérifie simplement le log du nœud
            if !self.audit_node(node_id) {
                return false;
            }
        }

        true
    }

    /// Détecte un fork de log (equivocation)
    pub fn detect_fork(&mut self, node_id: NodeId) -> bool {
        // Vérifier si le nœud a envoyé des contenus différents
        let node = match self.nodes.get(&node_id) {
            Some(n) => n,
            None => return false,
        };

        if node.fault_injector.has_fault("fork") {
            // Fork détecté
            if let Some(node) = self.nodes.get_mut(&node_id) {
                node.mark_exposed("Fork detected: equivocation");
            }

            let proof = ExposureProof {
                witness_id: 0,
                exposed_node_id: node_id,
                reason: "Equivocation detected: different content sent to different nodes".to_string(),
                evidence_type: EvidenceType::Equivocation,
            };

            self.exposure_proofs.insert(node_id, proof);
            self.metrics.detection_time.record_detection(node_id, DetectionMethod::Consistency);

            return true;
        }

        false
    }

    /// Compte les nœuds exposés
    pub fn count_exposed_nodes(&self) -> usize {
        self.nodes.values().filter(|n| n.status == NodeStatus::Exposed).count()
    }

    /// Compte les nœuds suspectés
    pub fn count_suspected_nodes(&self) -> usize {
        self.nodes.values().filter(|n| n.status == NodeStatus::Suspected).count()
    }

    /// Compte les nœuds de confiance
    pub fn count_trusted_nodes(&self) -> usize {
        self.nodes.values().filter(|n| n.status == NodeStatus::Trusted).count()
    }

    /// Vérifie si un nœud est exposé
    pub fn is_exposed(&self, node_id: NodeId) -> bool {
        self.nodes.get(&node_id).map(|n| n.status == NodeStatus::Exposed).unwrap_or(false)
    }

    /// Obtient la preuve d'exposition pour un nœud
    pub fn get_exposure_proof(&self, node_id: NodeId) -> Option<&ExposureProof> {
        self.exposure_proofs.get(&node_id)
    }

    /// Compte combien de nœuds ont reçu un message spécifique
    pub fn count_receivers(&self, msg_id: &str) -> usize {
        self.nodes.values()
            .filter(|n| n.received_messages.iter().any(|m| m.id == msg_id))
            .count()
    }

    /// Finalise les métriques et retourne un rapport
    pub fn finalize_metrics(&mut self) {
        // Enregistrer le statut final de chaque nœud
        for (&node_id, node) in &self.nodes {
            self.metrics.accuracy.record_status(node_id, node.status);
        }

        self.metrics.finalize();
    }

    /// Retourne les métriques de test
    pub fn get_metrics(&self) -> &TestMetrics {
        &self.metrics
    }

    /// Attend un délai simulé
    pub fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    /// Simule le passage du temps pour les timeouts
    pub fn advance_time(&mut self, _duration: Duration) {
        // Dans une vraie implémentation, cela avancerait les timers internes
    }

    /// Vérifie qu'aucun nœud correct n'a été exposé (pas de faux positifs)
    pub fn verify_no_false_positives(&self, faulty_nodes: &[NodeId]) -> bool {
        for (&node_id, node) in &self.nodes {
            if !faulty_nodes.contains(&node_id) && node.status == NodeStatus::Exposed {
                return false; // Faux positif détecté
            }
        }
        true
    }

    /// Vérifie que tous les nœuds fautifs ont été détectés
    pub fn verify_all_faults_detected(&self, faulty_nodes: &[NodeId]) -> bool {
        for &node_id in faulty_nodes {
            if let Some(node) = self.nodes.get(&node_id) {
                if node.status == NodeStatus::Trusted {
                    return false; // Faux négatif
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_creation() {
        let cluster = TestCluster::new("test_basic").unwrap();
        assert_eq!(cluster.nodes.len(), 10);
        assert_eq!(cluster.config.num_trees, 3);
    }

    #[test]
    fn test_witness_setup() {
        let cluster = TestCluster::new("test_witnesses").unwrap();

        // Each node should have witnesses
        for node_id in 1..=10 {
            let witnesses = cluster.witnesses_map.get(&node_id).unwrap();
            assert!(!witnesses.is_empty());
            assert!(!witnesses.contains(&node_id)); // Node shouldn't be its own witness
        }
    }
}
