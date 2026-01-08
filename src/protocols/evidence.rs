use crate::journal::entry::LogEntry;
use crate::types::{PeerStatus, PendingChallenge, EvidenceType};
use crate::types::node::Node as PeerReviewNode;
use crate::types::messages::{Challenge, ChallengeKind, ChallengeResponse, Proof, Authenticator, MsgLogEntry, MsgContent, MsgType};
use super::consistency::ConsistencyChallenge;
use super::challenge_response::{AuditChallenge, SendChallenge};

type DetectionState = PeerStatus;

impl PeerReviewNode {
    /// Algorithm 15: Ligne 1-5
    pub fn receive_challenge_request(&mut self, target_node_id: u32, seq_nums: Vec<usize>) {
        println!(
            "[Nœud {}] Challenge reçu pour le nœud {} (depuis protocole consistency/audit)",
            self.node_id, target_node_id
        );

        self.set_detection_state(target_node_id, DetectionState::Suspected);

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

    pub fn receive_audit_challenge(&mut self, challenge: Challenge) {
        let (min_seq, max_seq) = match &challenge.kind {
            ChallengeKind::Audit { min_auth, max_auth } => (min_auth.seq, max_auth.seq),
            _ => return,
        };

        println!(
            "[Nœud {}] Challenge d'audit reçu du nœud {} pour le nœud {} (seq={} à {})",
            self.node_id, challenge.challenger, challenge.target,
            min_seq, max_seq
        );

        self.set_detection_state(challenge.target, DetectionState::Suspected);

        let seq_nums: Vec<usize> = (min_seq..=max_seq).collect();
        let pending = PendingChallenge {
            target_node_id: challenge.target,
            seq_nums,
        };

        self.pending_challenges
            .entry(challenge.target)
            .or_insert_with(Vec::new)
            .push(pending);

        println!(
            "[Nœud {}] → Détecteur: suspected({}) [AUDIT CHALLENGE]",
            self.node_id, challenge.target
        );
    }

    pub fn receive_challenge(&mut self, challenge: Challenge) {
        println!(
            "[Nœud {}] Challenge reçu du nœud {} pour le nœud {}",
            self.node_id, challenge.challenger, challenge.target
        );

        self.set_detection_state(challenge.target, DetectionState::Suspected);

        match &challenge.kind {
            ChallengeKind::Audit { min_auth, max_auth } => {
                let seq_nums: Vec<usize> = (min_auth.seq..=max_auth.seq).collect();
                let pending = PendingChallenge {
                    target_node_id: challenge.target,
                    seq_nums,
                };
                self.pending_challenges
                    .entry(challenge.target)
                    .or_insert_with(Vec::new)
                    .push(pending);
                println!(
                    "[Nœud {}] → Détecteur: suspected({}) [AUDIT CHALLENGE]",
                    self.node_id, challenge.target
                );
            }
            ChallengeKind::Send { .. } => {
                println!(
                    "[Nœud {}] → Détecteur: suspected({}) [SEND CHALLENGE]",
                    self.node_id, challenge.target
                );
            }
        }
    }

    pub fn receive_send_challenge(&mut self, challenge: Challenge) {
        println!(
            "[Nœud {}] Challenge d'envoi reçu du nœud {} pour le nœud {}",
            self.node_id, challenge.challenger, challenge.target
        );

        self.set_detection_state(challenge.target, DetectionState::Suspected);

        println!(
            "[Nœud {}] → Détecteur: suspected({}) [SEND CHALLENGE]",
            self.node_id, challenge.target
        );
    }

    /// Algorithm 15: Ligne 3-5
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

            if let Some(pending) = self.pending_challenges.get(&sender_id) {
                let mut challenges = Vec::new();

                for pending_challenge in pending {
                    let min_seq = *pending_challenge.seq_nums.iter().min().unwrap_or(&0);
                    let max_seq = *pending_challenge.seq_nums.iter().max().unwrap_or(&0);

                    let challenge = ConsistencyChallenge {
                        challenger: self.node_id,
                        target: sender_id,
                        kind: ChallengeKind::Audit {
                            min_auth: Authenticator { seq: min_seq, hash: [0u8; 32], sig: [0u8; 64] },
                            max_auth: Authenticator { seq: max_seq, hash: [0u8; 32], sig: [0u8; 64] },
                        },
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

    /// Algorithm 15: Ligne 6-14
    pub fn process_challenge_responses(
        &mut self,
        node_id: u32,
        responses: Vec<Vec<LogEntry>>,
    ) -> bool {
        println!(
            "[Nœud {}] Traitement des réponses du nœud {} (utilise verify_logs_as_witness)",
            self.node_id, node_id
        );

        let mut all_valid = true;
        let mut failed_evidence_type = EvidenceType::SignatureMismatch;
        let mut failure_reason = String::new();

        for logs in &responses {
            if !self.verify_logs_as_witness(node_id, logs) {
                all_valid = false;
                if self.exposed_nodes.contains(&node_id) {
                    failure_reason = format!("Preuve d'incohérence détectée par le protocole consistency");
                    failed_evidence_type = EvidenceType::BrokenHashChain;
                }
                break;
            }
        }

        if all_valid {
            self.set_detection_state(node_id, DetectionState::Trusted);
            println!(
                "[Nœud {}] ✓ Toutes les preuves sont valides (vérifiées par consistency) → Détecteur: trusted({})",
                self.node_id, node_id
            );

            self.pending_challenges.remove(&node_id);
        } else {
            self.set_detection_state(node_id, DetectionState::Exposed);
            println!(
                "[Nœud {}] ✗ Preuves invalides (détectées par consistency) → Détecteur: exposed({})",
                self.node_id, node_id
            );

            self.broadcast_exposure_proof(node_id, responses);
        }

        all_valid
    }

    /// Algorithm 15: Ligne 16-23
    pub fn receive_proof(&mut self, proof: &Proof) -> bool {
        println!(
            "[Nœud {}] Preuve d'exposition reçue pour le nœud {} (accusateur: {})",
            self.node_id, proof.guilty_node, proof.accuser_node
        );

        let is_valid_proof = self.verify_proof(proof);

        if is_valid_proof {
            self.set_detection_state(proof.guilty_node, DetectionState::Exposed);
            println!(
                "[Nœud {}] ✓ Preuve valide → Détecteur: exposed({})",
                self.node_id, proof.guilty_node
            );
        } else {
            println!(
                "[Nœud {}] ✗ Preuve INVALIDE de l'accusateur {}!",
                self.node_id, proof.accuser_node
            );
            println!(
                "[Nœud {}] Envoi d'une preuve de fausse preuve aux témoins",
                self.node_id
            );
            self.broadcast_false_proof_evidence(proof);
        }

        is_valid_proof
    }

    pub fn set_detection_state(&mut self, node_id: u32, state: DetectionState) {
        self.detection_states.insert(node_id, state);

        if state == DetectionState::Exposed && !self.exposed_nodes.contains(&node_id) {
            self.exposed_nodes.push(node_id);
        }
    }

    pub fn get_detection_state(&self, node_id: u32) -> DetectionState {
        self.detection_states
            .get(&node_id)
            .copied()
            .unwrap_or(DetectionState::Trusted)
    }

    fn broadcast_exposure_proof(&self, exposed_node_id: u32, responses: Vec<Vec<LogEntry>>) {
        let witnesses = self.get_witnesses(exposed_node_id);

        println!(
            "[Nœud {}] Diffusion de la preuve d'exposition du nœud {} aux {} témoin(s)",
            self.node_id,
            exposed_node_id,
            witnesses.len()
        );

        let authenticator = if !responses.is_empty() && !responses[0].is_empty() {
            Authenticator {
                seq: responses[0][0].s_k,
                hash: responses[0][0].hash,
                sig: responses[0][0].sig,
            }
        } else {
            Authenticator {
                seq: 0,
                hash: [0u8; 32],
                sig: [0u8; 64],
            }
        };

        let log_suffix: Vec<MsgLogEntry> = responses
            .iter()
            .flatten()
            .map(|log| MsgLogEntry {
                seq: log.s_k,
                msg_type: MsgType::Send,
                dest: log.corr,
                hash: log.hash,
                sig: log.sig,
                content: MsgContent::SendContent {
                    dest: log.corr,
                    message: log.msg.clone(),
                },
            })
            .collect();

        let _proof = Proof {
            guilty_node: exposed_node_id,
            accuser_node: self.node_id,
            evidence_type: EvidenceType::BrokenHashChain,
            authenticator,
            log_suffix,
            reason: "Incohérence détectée dans les logs".to_string(),
        };

        for witness_id in witnesses {
            println!(
                "[Nœud {}] → Témoin {} : Preuve d'exposition du nœud {}",
                self.node_id, witness_id, exposed_node_id
            );
        }
    }

    fn verify_proof(&self, proof: &Proof) -> bool {
        println!(
            "[Nœud {}] Vérification de la preuve d'exposition...",
            self.node_id
        );

        let witnesses = self.get_witnesses(proof.guilty_node);
        if !witnesses.contains(&proof.accuser_node) {
            println!(
                "[Nœud {}] ✗ Le nœud {} n'est pas un témoin valide du nœud {}",
                self.node_id, proof.accuser_node, proof.guilty_node
            );
            return false;
        }

        if proof.log_suffix.is_empty() {
            println!("[Nœud {}] ✗ Aucun log dans la preuve", self.node_id);
            return false;
        }

        println!(
            "[Nœud {}] ✓ Preuve structurellement valide (authenticator seq={}, {} entrées de log)",
            self.node_id, proof.authenticator.seq, proof.log_suffix.len()
        );
        true
    }

    fn broadcast_false_proof_evidence(&self, false_proof: &Proof) {
        let witnesses = self.get_witnesses(false_proof.accuser_node);

        println!(
            "[Nœud {}] Diffusion de la preuve de FAUSSE PREUVE de l'accusateur {} aux {} témoin(s)",
            self.node_id,
            false_proof.accuser_node,
            witnesses.len()
        );

        for witness_id in witnesses {
            println!(
                "[Nœud {}] → Témoin {} : L'accusateur {} a fourni une fausse preuve",
                self.node_id, witness_id, false_proof.accuser_node
            );
        }
    }

    pub fn is_suspected(&self, node_id: u32) -> bool {
        self.get_detection_state(node_id) == DetectionState::Suspected
    }

    pub fn is_trusted(&self, node_id: u32) -> bool {
        self.get_detection_state(node_id) == DetectionState::Trusted
    }

    pub fn is_exposed(&self, node_id: u32) -> bool {
        self.get_detection_state(node_id) == DetectionState::Exposed
    }
}
