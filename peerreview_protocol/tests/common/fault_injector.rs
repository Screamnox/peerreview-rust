//! Injecteur de fautes byzantines pour les tests PeerReview

use peerreview_protocol::types::NodeId;
use rand::Rng;

/// Types de fautes injectables
#[derive(Debug, Clone)]
pub enum FaultType {
    /// Modification du contenu des messages (Detectably Faulty)
    TamperContent {
        pattern: String,
        replacement: String,
    },

    /// Drop tous les messages sortants (Silent Node - Detectably Ignorant)
    DropAllOutgoing,

    /// Drop aléatoire avec une probabilité donnée
    DropRandomly {
        probability: f64,
    },

    /// Fork de log - envoie différents contenus à différents nœuds (Equivocation)
    ForkLog {
        branch_a_content: String,
        branch_b_content: String,
        branch_a_targets: Vec<NodeId>,
        branch_b_targets: Vec<NodeId>,
    },

    /// Délai des messages
    DelayMessages {
        delay_ms: u64,
    },

    /// Authenticateur invalide
    InvalidAuth,

    /// Forwarder réticent - ne forward que si challenged
    ReluctantForwarder,
}

/// Injecteur de fautes pour un nœud
#[derive(Debug, Clone, Default)]
pub struct FaultInjector {
    faults: Vec<FaultType>,
    drop_outgoing: bool,
    drop_probability: f64,
    tamper_pattern: Option<(String, String)>,
    fork_config: Option<ForkConfig>,
    is_reluctant: bool,
    has_been_challenged: bool,
}

#[derive(Debug, Clone)]
struct ForkConfig {
    branch_a_content: String,
    branch_b_content: String,
    branch_a_targets: Vec<NodeId>,
    branch_b_targets: Vec<NodeId>,
}

impl FaultInjector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Injecte une nouvelle faute
    pub fn inject(&mut self, fault: FaultType) {
        match &fault {
            FaultType::TamperContent { pattern, replacement } => {
                self.tamper_pattern = Some((pattern.clone(), replacement.clone()));
            }
            FaultType::DropAllOutgoing => {
                self.drop_outgoing = true;
            }
            FaultType::DropRandomly { probability } => {
                self.drop_probability = *probability;
            }
            FaultType::ForkLog {
                branch_a_content,
                branch_b_content,
                branch_a_targets,
                branch_b_targets,
            } => {
                self.fork_config = Some(ForkConfig {
                    branch_a_content: branch_a_content.clone(),
                    branch_b_content: branch_b_content.clone(),
                    branch_a_targets: branch_a_targets.clone(),
                    branch_b_targets: branch_b_targets.clone(),
                });
            }
            FaultType::ReluctantForwarder => {
                self.is_reluctant = true;
            }
            _ => {}
        }
        self.faults.push(fault);
    }

    /// Supprime toutes les fautes
    pub fn clear(&mut self) {
        self.faults.clear();
        self.drop_outgoing = false;
        self.drop_probability = 0.0;
        self.tamper_pattern = None;
        self.fork_config = None;
        self.is_reluctant = false;
        self.has_been_challenged = false;
    }

    /// Vérifie si le message doit être droppé (entrant)
    pub fn should_drop(&self) -> bool {
        if self.drop_probability > 0.0 {
            let mut rng = rand::thread_rng();
            return rng.gen::<f64>() < self.drop_probability;
        }
        false
    }

    /// Vérifie si le message sortant doit être droppé
    pub fn should_drop_outgoing(&self) -> bool {
        if self.drop_outgoing {
            return true;
        }
        if self.is_reluctant && !self.has_been_challenged {
            return true;
        }
        false
    }

    /// Applique tampering au contenu si configuré
    pub fn tamper_content(&self, content: &[u8]) -> Option<Vec<u8>> {
        if let Some((pattern, replacement)) = &self.tamper_pattern {
            let content_str = String::from_utf8_lossy(content);
            if content_str.contains(pattern) {
                let tampered = content_str.replace(pattern, replacement);
                return Some(tampered.into_bytes());
            }
        }
        None
    }

    /// Retourne le contenu forké basé sur la cible
    pub fn get_forked_content(&self, original: &[u8], target: NodeId) -> Vec<u8> {
        if let Some(ref config) = self.fork_config {
            if config.branch_a_targets.contains(&target) {
                return config.branch_a_content.as_bytes().to_vec();
            }
            if config.branch_b_targets.contains(&target) {
                return config.branch_b_content.as_bytes().to_vec();
            }
        }
        original.to_vec()
    }

    /// Marque le nœud comme ayant été challenged (pour ReluctantForwarder)
    pub fn mark_challenged(&mut self) {
        self.has_been_challenged = true;
    }

    /// Vérifie si une faute spécifique est active
    pub fn has_fault(&self, fault_type: &str) -> bool {
        self.faults.iter().any(|f| {
            match (fault_type, f) {
                ("tamper", FaultType::TamperContent { .. }) => true,
                ("drop_outgoing", FaultType::DropAllOutgoing) => true,
                ("drop_random", FaultType::DropRandomly { .. }) => true,
                ("fork", FaultType::ForkLog { .. }) => true,
                ("delay", FaultType::DelayMessages { .. }) => true,
                ("invalid_auth", FaultType::InvalidAuth) => true,
                ("reluctant", FaultType::ReluctantForwarder) => true,
                _ => false,
            }
        })
    }

    /// Vérifie si des fautes sont actives
    pub fn is_faulty(&self) -> bool {
        !self.faults.is_empty()
    }

    /// Retourne une description de la faute active
    pub fn fault_description(&self) -> String {
        if self.faults.is_empty() {
            return "None".to_string();
        }

        self.faults
            .iter()
            .map(|f| match f {
                FaultType::TamperContent { pattern, replacement } => {
                    format!("TamperContent({} -> {})", pattern, replacement)
                }
                FaultType::DropAllOutgoing => "DropAllOutgoing".to_string(),
                FaultType::DropRandomly { probability } => {
                    format!("DropRandomly({:.0}%)", probability * 100.0)
                }
                FaultType::ForkLog { .. } => "ForkLog".to_string(),
                FaultType::DelayMessages { delay_ms } => format!("DelayMessages({}ms)", delay_ms),
                FaultType::InvalidAuth => "InvalidAuth".to_string(),
                FaultType::ReluctantForwarder => "ReluctantForwarder".to_string(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tamper_content() {
        let mut injector = FaultInjector::new();
        injector.inject(FaultType::TamperContent {
            pattern: "DATA:".to_string(),
            replacement: "FAKE:".to_string(),
        });

        let original = b"DATA:12345";
        let tampered = injector.tamper_content(original).unwrap();
        assert_eq!(tampered, b"FAKE:12345");
    }

    #[test]
    fn test_drop_outgoing() {
        let mut injector = FaultInjector::new();
        assert!(!injector.should_drop_outgoing());

        injector.inject(FaultType::DropAllOutgoing);
        assert!(injector.should_drop_outgoing());

        injector.clear();
        assert!(!injector.should_drop_outgoing());
    }

    #[test]
    fn test_fork_content() {
        let mut injector = FaultInjector::new();
        injector.inject(FaultType::ForkLog {
            branch_a_content: "MSG_A".to_string(),
            branch_b_content: "MSG_X".to_string(),
            branch_a_targets: vec![3],
            branch_b_targets: vec![5],
        });

        let original = b"ORIGINAL";
        assert_eq!(injector.get_forked_content(original, 3), b"MSG_A");
        assert_eq!(injector.get_forked_content(original, 5), b"MSG_X");
        assert_eq!(injector.get_forked_content(original, 7), original.to_vec());
    }

    #[test]
    fn test_reluctant_forwarder() {
        let mut injector = FaultInjector::new();
        injector.inject(FaultType::ReluctantForwarder);

        // Should drop before challenge
        assert!(injector.should_drop_outgoing());

        // After challenge, should forward
        injector.mark_challenged();
        assert!(!injector.should_drop_outgoing());
    }
}
