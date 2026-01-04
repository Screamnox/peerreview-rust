use crate::journal::entry::LogEntry;
use super::node::{PeerReviewNode, DetectionState, PendingChallenge};
use super::consistency::ConsistencyChallenge;
use super::challenge_response::{Challenge, AuditChallenge, SendChallenge};

/// Structure d'une preuve d'exposition
#[derive(Debug, Clone)]
pub struct ExposureProof {
    pub witness_id: u32,
    pub exposed_node_id: u32,
    pub evidence_type: EvidenceType,
    pub logs: Vec<LogEntry>,
    pub reason: String,
}

/// Type de preuve
#[derive(Debug, Clone, PartialEq)]
pub enum EvidenceType {
    SignatureMismatch,      // Signature différente de l'authenticator stocké
    BrokenHashChain,        // Chaîne de hash invalide
    InvalidSignature,       // Signature Ed25519 invalide
}

impl PeerReviewNode {
    // ===============================
    // Algorithm 15: Traitement des preuves obtenues
    // ===============================

    /// Ligne 1-5: Si i obtient un challenge pour un nœud j
    /// Le détecteur de i indique suspected(j)
    /// Accepte les challenges de tous les protocoles (consistency, audit, send)
    pub fn receive_challenge_request(&mut self, target_node_id: u32, seq_nums: Vec<usize>) {
        println!(
            "[Nœud {}] Challenge reçu pour le nœud {} (depuis protocole consistency/audit)",
            self.node_id, target_node_id
        );

        // Ligne 2: Le détecteur de i indique suspected(j)
        self.set_detection_state(target_node_id, DetectionState::Suspected);

        // Stocker le challenge en attente
        let challenge = PendingChallenge {
            target_node_id,
            seq_nums,
        };

        self.pending_challenges
            .entry(target_node_id)
            .or_insert_with(Vec::new)
            .push(challenge);

        println!(
            "[Nœud {}] → Détecteur: suspected({})",
            self.node_id, target_node_id
        );
    }

    /// Recevoir un challenge d'audit (depuis challenge_response.rs)
    pub fn receive_audit_challenge(&mut self, challenge: AuditChallenge) {
        println!(
            "[Nœud {}] Challenge d'audit reçu du nœud {} pour le nœud {} (seq={} à {})",
            self.node_id, challenge.challenger_id, challenge.target_id,
            challenge.seq_min, challenge.seq_max
        );

        // Marquer comme suspect
        self.set_detection_state(challenge.target_id, DetectionState::Suspected);

        // Convertir en pending challenge
        let seq_nums: Vec<usize> = (challenge.seq_min..=challenge.seq_max).collect();
        let pending = PendingChallenge {
            target_node_id: challenge.target_id,
            seq_nums,
        };

        self.pending_challenges
            .entry(challenge.target_id)
            .or_insert_with(Vec::new)
            .push(pending);

        println!(
            "[Nœud {}] → Détecteur: suspected({}) [AUDIT CHALLENGE]",
            self.node_id, challenge.target_id
        );
    }

    /// Recevoir un challenge d'envoi (depuis challenge_response.rs)
    pub fn receive_send_challenge(&mut self, challenge: SendChallenge) {
        println!(
            "[Nœud {}] Challenge d'envoi reçu du nœud {} pour le nœud {}",
            self.node_id, challenge.challenger_id, challenge.target_id
        );

        // Marquer comme suspect
        self.set_detection_state(challenge.target_id, DetectionState::Suspected);

        println!(
            "[Nœud {}] → Détecteur: suspected({}) [SEND CHALLENGE]",
            self.node_id, challenge.target_id
        );
    }

    /// Ligne 3-5: Si i reçoit un message de j dans l'état suspect
    /// i challenge j avec tous les challenges en attente
    pub fn handle_message_from_suspected_node(
        &mut self,
        sender_id: u32,
    ) -> std::io::Result<Option<Vec<ConsistencyChallenge>>> {
        let state = self.get_detection_state(sender_id);

        if state == DetectionState::Suspected {
            println!(
                "[Nœud {}] Message reçu du nœud suspect {}",
                self.node_id, sender_id
            );

            // Ligne 4: i challenge j avec tous les challenges en attente
            if let Some(pending) = self.pending_challenges.get(&sender_id) {
                let mut challenges = Vec::new();

                for pending_challenge in pending {
                    let challenge = ConsistencyChallenge {
                        witness_id: self.node_id,
                        target_id: sender_id,
                        seq_nums: pending_challenge.seq_nums.clone(),
                    };
                    challenges.push(challenge);

                    println!(
                        "[Nœud {}] Envoi de challenge au nœud suspect {} (séquences: {:?})",
                        self.node_id,
                        sender_id,
                        &pending_challenge.seq_nums[..5.min(pending_challenge.seq_nums.len())]
                    );
                }

                return Ok(Some(challenges));
            }
        }

        Ok(None)
    }

    /// Ligne 6-14: Si i reçoit des réponses valides à tous les challenges
    /// Utilise les mécanismes de vérification de consistency.rs (verify_logs_as_witness)
    /// et récupère les preuves générées par ce protocole pour les diffuser
    pub fn process_challenge_responses(
        &mut self,
        node_id: u32,
        responses: Vec<Vec<LogEntry>>,
    ) -> bool {
        println!(
            "[Nœud {}] Traitement des réponses du nœud {} (utilise verify_logs_as_witness)",
            self.node_id, node_id
        );

        // Ligne 7: i recalcule les preuves pour vérifier qu'elles sont toutes valides
        // Utilise la fonction de vérification du protocole consistency
        let mut all_valid = true;
        let mut failed_evidence_type = EvidenceType::SignatureMismatch;
        let mut failure_reason = String::new();

        for logs in &responses {
            // Vérifier chaque ensemble de logs avec le protocole consistency
            // verify_logs_as_witness vérifie:
            // 1. Signatures vs authenticators stockés
            // 2. Chaîne de hash
            // 3. Signatures Ed25519
            if !self.verify_logs_as_witness(node_id, logs) {
                all_valid = false;
                // Déterminer le type de faute détectée
                if self.exposed_nodes.contains(&node_id) {
                    failure_reason = format!("Preuve d'incohérence détectée par le protocole consistency");
                    failed_evidence_type = EvidenceType::BrokenHashChain;
                }
                break;
            }
        }

        // Ligne 8-13: Selon la validité des preuves
        if all_valid {
            // Ligne 9: Le détecteur de i indique définitivement trusted(j)
            self.set_detection_state(node_id, DetectionState::Trusted);
            println!(
                "[Nœud {}] ✓ Toutes les preuves sont valides (vérifiées par consistency) → Détecteur: trusted({})",
                self.node_id, node_id
            );

            // Nettoyer les challenges en attente
            self.pending_challenges.remove(&node_id);
        } else {
            // Ligne 11: Le détecteur de i indique définitivement exposed(j)
            self.set_detection_state(node_id, DetectionState::Exposed);
            println!(
                "[Nœud {}] ✗ Preuves invalides (détectées par consistency) → Détecteur: exposed({})",
                self.node_id, node_id
            );

            // Ligne 12: i envoie à tous les témoins la preuve de faute de j
            // Récupère les preuves générées par le protocole consistency
            self.broadcast_exposure_proof_with_evidence(node_id, responses, failed_evidence_type, failure_reason);
        }

        all_valid
    }

    /// Ligne 16-23: Si i obtient une preuve d'exposition de j
    pub fn receive_exposure_proof(&mut self, proof: &ExposureProof) -> bool {
        println!(
            "[Nœud {}] Preuve d'exposition reçue pour le nœud {} (témoin: {})",
            self.node_id, proof.exposed_node_id, proof.witness_id
        );

        // Ligne 17: i recalcule la preuve pour vérifier qu'il y a bien exposition
        let is_valid_proof = self.verify_exposure_proof(proof);

        if is_valid_proof {
            // Ligne 19: Le détecteur de i indique définitivement exposed(j)
            self.set_detection_state(proof.exposed_node_id, DetectionState::Exposed);
            println!(
                "[Nœud {}] ✓ Preuve valide → Détecteur: exposed({})",
                self.node_id, proof.exposed_node_id
            );
            println!("  Raison: {}", proof.reason);
        } else {
            // Ligne 21: i envoie à tous les témoins la preuve de fausse preuve
            println!(
                "[Nœud {}] ✗ Preuve INVALIDE du témoin {}!",
                self.node_id, proof.witness_id
            );
            println!(
                "[Nœud {}] Envoi d'une preuve de fausse preuve aux témoins",
                self.node_id
            );
            self.broadcast_false_proof_evidence(proof);
        }

        is_valid_proof
    }

    // ===============================
    // Fonctions auxiliaires
    // ===============================

    /// Définit l'état de détection d'un nœud
    pub fn set_detection_state(&mut self, node_id: u32, state: DetectionState) {
        self.detection_states.insert(node_id, state);

        // Si le nœud est marqué comme exposed, l'ajouter à la liste
        if state == DetectionState::Exposed && !self.exposed_nodes.contains(&node_id) {
            self.exposed_nodes.push(node_id);
        }
    }

    /// Récupère l'état de détection d'un nœud
    /// Par défaut, tous les nœuds sont considérés comme corrects (Trusted)
    pub fn get_detection_state(&self, node_id: u32) -> DetectionState {
        self.detection_states
            .get(&node_id)
            .copied()
            .unwrap_or(DetectionState::Trusted)
    }

    /// Diffuse une preuve d'exposition à tous les témoins du nœud fautif
    /// (version simple pour compatibilité)
    fn broadcast_exposure_proof(&self, exposed_node_id: u32, responses: Vec<Vec<LogEntry>>) {
        self.broadcast_exposure_proof_with_evidence(
            exposed_node_id,
            responses,
            EvidenceType::SignatureMismatch,
            "Incohérence détectée dans les logs".to_string()
        );
    }

    /// Diffuse une preuve d'exposition avec les preuves récupérées des protocoles
    /// consistency.rs et challenge_response.rs
    fn broadcast_exposure_proof_with_evidence(
        &self,
        exposed_node_id: u32,
        responses: Vec<Vec<LogEntry>>,
        evidence_type: EvidenceType,
        reason: String,
    ) {
        let witnesses = self.get_witnesses(exposed_node_id);

        println!(
            "[Nœud {}] Diffusion de la preuve d'exposition du nœud {} aux {} témoin(s)",
            self.node_id,
            exposed_node_id,
            witnesses.len()
        );
        println!(
            "[Nœud {}]   Type de preuve: {:?} (récupérée des protocoles consistency/challenge)",
            self.node_id, evidence_type
        );
        println!(
            "[Nœud {}]   Raison: {}",
            self.node_id, reason
        );

        // Créer la preuve d'exposition avec les preuves des autres protocoles
        let _proof = ExposureProof {
            witness_id: self.node_id,
            exposed_node_id,
            evidence_type,
            logs: responses.into_iter().flatten().collect(),
            reason,
        };

        for witness_id in witnesses {
            println!(
                "[Nœud {}] → Témoin {} : Preuve d'exposition du nœud {} (avec preuves des protocoles)",
                self.node_id, witness_id, exposed_node_id
            );
            // Dans une vraie implémentation, envoyer le proof via le réseau
            // self.network.send_to(witness_id, proof.clone());
        }
    }

    /// Vérifie une preuve d'exposition reçue
    fn verify_exposure_proof(&self, proof: &ExposureProof) -> bool {
        println!(
            "[Nœud {}] Vérification de la preuve d'exposition...",
            self.node_id
        );

        // Vérifier que le nœud witness_id est bien un témoin du nœud exposé
        let witnesses = self.get_witnesses(proof.exposed_node_id);
        if !witnesses.contains(&proof.witness_id) {
            println!(
                "[Nœud {}] ✗ Le nœud {} n'est pas un témoin valide du nœud {}",
                self.node_id, proof.witness_id, proof.exposed_node_id
            );
            return false;
        }

        // Recalculer les vérifications sur les logs fournis
        // (même logique que verify_logs_as_witness mais sans modifier l'état)
        if proof.logs.is_empty() {
            println!("[Nœud {}] ✗ Aucun log dans la preuve", self.node_id);
            return false;
        }

        // Vérifier selon le type de preuve
        match proof.evidence_type {
            EvidenceType::SignatureMismatch => {
                println!(
                    "[Nœud {}] Vérification: Signature Mismatch",
                    self.node_id
                );
                // Vérifier que les signatures ne correspondent pas aux authenticators
                true // Simplifié pour l'instant
            }
            EvidenceType::BrokenHashChain => {
                println!(
                    "[Nœud {}] Vérification: Broken Hash Chain",
                    self.node_id
                );
                // Vérifier que la chaîne de hash est brisée
                true // Simplifié pour l'instant
            }
            EvidenceType::InvalidSignature => {
                println!(
                    "[Nœud {}] Vérification: Invalid Signature",
                    self.node_id
                );
                // Vérifier que les signatures Ed25519 sont invalides
                true // Simplifié pour l'instant
            }
        }
    }

    /// Diffuse une preuve de fausse preuve (le témoin a menti)
    fn broadcast_false_proof_evidence(&self, false_proof: &ExposureProof) {
        let witnesses = self.get_witnesses(false_proof.witness_id);

        println!(
            "[Nœud {}] Diffusion de la preuve de FAUSSE PREUVE du témoin {} aux {} témoin(s)",
            self.node_id,
            false_proof.witness_id,
            witnesses.len()
        );

        for witness_id in witnesses {
            println!(
                "[Nœud {}] → Témoin {} : Le témoin {} a fourni une fausse preuve",
                self.node_id, witness_id, false_proof.witness_id
            );
            // Dans une vraie implémentation, envoyer la preuve via le réseau
        }
    }

    /// Vérifie si un nœud est suspect
    pub fn is_suspected(&self, node_id: u32) -> bool {
        self.get_detection_state(node_id) == DetectionState::Suspected
    }

    /// Vérifie si un nœud est de confiance
    pub fn is_trusted(&self, node_id: u32) -> bool {
        self.get_detection_state(node_id) == DetectionState::Trusted
    }

    /// Vérifie si un nœud est exposé (utilise la nouvelle structure)
    pub fn is_exposed(&self, node_id: u32) -> bool {
        self.get_detection_state(node_id) == DetectionState::Exposed
    }
}
