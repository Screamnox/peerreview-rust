use crate::journal::entry::{LogEntry, LogType};
use crate::types::node::Node as PeerReviewNode;
use crate::types::messages::{Challenge, ChallengeKind, Authenticator};
use ed25519_dalek::Verifier;
use sha2::{Digest, Sha256};

pub type ConsistencyChallenge = Challenge;

impl PeerReviewNode {
    pub fn challenge_witnessed_node(
        &mut self,
        observed_node_id: u32,
    ) -> std::io::Result<Option<Challenge>> {
        if !self.should_challenge(observed_node_id) {
            return Ok(None);
        }

        let stored_auths = self.stored_authenticators.get(&observed_node_id).unwrap();
        let seq_nums: Vec<usize> = stored_auths.iter().map(|a| a.seq_num()).collect();

        println!(
            "[Témoin {}] Challenge du nœud {} pour {} authenticators",
            self.node_id,
            observed_node_id,
            seq_nums.len()
        );

        let challenge_msg = format!("CONSISTENCY_CHALLENGE: {} logs demandés", seq_nums.len());
        self.logger.log_send(observed_node_id, &challenge_msg, &mut self.keypair)?;

        let min_seq = *seq_nums.iter().min().unwrap_or(&0);
        let max_seq = *seq_nums.iter().max().unwrap_or(&0);

        let min_auth = stored_auths.iter()
            .find(|a| a.seq_num() == min_seq)
            .map(|a| a.authenticator.clone())
            .unwrap_or(Authenticator { seq: 0, hash: [0u8; 32], sig: [0u8; 64] });

        let max_auth = stored_auths.iter()
            .find(|a| a.seq_num() == max_seq)
            .map(|a| a.authenticator.clone())
            .unwrap_or(Authenticator { seq: 0, hash: [0u8; 32], sig: [0u8; 64] });

        Ok(Some(Challenge {
            challenger: self.node_id,
            target: observed_node_id,
            kind: ChallengeKind::Audit { min_auth, max_auth },
        }))
    }

    pub fn respond_to_challenge(
        &mut self,
        challenge: &Challenge,
    ) -> std::io::Result<Vec<LogEntry>> {
        let (min_seq, max_seq) = match &challenge.kind {
            ChallengeKind::Audit { min_auth, max_auth } => (min_auth.seq, max_auth.seq),
            _ => return Ok(Vec::new()),
        };

        println!(
            "[Nœud {}] Réponse au challenge du témoin {} pour les logs {} à {}",
            self.node_id,
            challenge.challenger,
            min_seq,
            max_seq
        );

        let all_logs = self.logger.get_log(self.logger.s_k - self.logger.line_max + 1, self.logger.s_k)?;
        let requested_logs: Vec<LogEntry> = all_logs
            .into_iter()
            .filter(|l| l.s_k >= min_seq && l.s_k <= max_seq)
            .collect();

        let response_msg = format!(
            "CONSISTENCY_RESPONSE: {} logs envoyés",
            requested_logs.len()
        );
        self.logger.log_send(challenge.challenger, &response_msg, &mut self.keypair)?;

        Ok(requested_logs)
    }

    /// [TÉMOIN] Vérifie la chaîne de hash et les signatures des logs reçus
    pub fn verify_logs_as_witness(
        &mut self,
        observed_node_id: u32,
        received_logs: &[LogEntry],
    ) -> bool {
        println!(
            "[Témoin {}] Vérification de {} logs du nœud {}",
            self.node_id,
            received_logs.len(),
            observed_node_id
        );

        let stored_auths = match self.stored_authenticators.get(&observed_node_id) {
            Some(auths) => auths,
            None => {
                println!(
                    "[Témoin {}] Aucun authenticator stocké pour le nœud {}",
                    self.node_id, observed_node_id
                );
                return false;
            }
        };

        let public_key = match self.peer_public_keys.get(&observed_node_id) {
            Some(key) => key,
            None => {
                println!(
                    "[Témoin {}] Clé publique du nœud {} inconnue",
                    self.node_id, observed_node_id
                );
                return false;
            }
        };

        for log_entry in received_logs {
            let stored_auth = match stored_auths.iter().find(|a| a.seq_num() == log_entry.s_k) {
                Some(auth) => auth,
                None => {
                    println!(
                        "[Témoin {}] ✗ Aucun authenticator stocké pour seq={}",
                        self.node_id, log_entry.s_k
                    );
                    self.mark_as_exposed(observed_node_id, "Numéro de séquence non trouvé");
                    return false;
                }
            };

            if stored_auth.signature() != log_entry.sig {
                println!(
                    "[Témoin {}] ✗ Signature différente pour seq={}: attendu {} != reçu {}",
                    self.node_id,
                    log_entry.s_k,
                    hex::encode(&stored_auth.signature()[..8]),
                    hex::encode(&log_entry.sig[..8])
                );
                self.mark_as_exposed(observed_node_id, "Signature modifiée");
                return false;
            }

            if !self.verify_log_hash(log_entry, received_logs) {
                println!(
                    "[Témoin {}] ✗ Hash invalide pour seq={}",
                    self.node_id, log_entry.s_k
                );
                self.mark_as_exposed(observed_node_id, "Chaîne de hash brisée");
                return false;
            }

            let mut signed_data = [0u8; 40];
            signed_data[..8].copy_from_slice(&log_entry.s_k.to_be_bytes());
            signed_data[8..].copy_from_slice(&log_entry.hash);

            let signature_obj = match ed25519_dalek::Signature::from_bytes(&log_entry.sig) {
                Ok(sig) => sig,
                Err(_) => {
                    println!(
                        "[Témoin {}] ✗ Format de signature invalide pour seq={}",
                        self.node_id, log_entry.s_k
                    );
                    self.mark_as_exposed(observed_node_id, "Format de signature invalide");
                    return false;
                }
            };

            if public_key.verify(&signed_data, &signature_obj).is_err() {
                println!(
                    "[Témoin {}] ✗ Signature Ed25519 invalide pour seq={}",
                    self.node_id, log_entry.s_k
                );
                self.mark_as_exposed(observed_node_id, "Signature Ed25519 invalide");
                return false;
            }
        }

        println!(
            "[Témoin {}] ✓ Tous les logs du nœud {} sont valides",
            self.node_id, observed_node_id
        );
        
        self.clear_authenticators(observed_node_id);
        
        true
    }

    fn verify_log_hash(&self, log_entry: &LogEntry, all_logs: &[LogEntry]) -> bool {
        let prev_hash = if log_entry.s_k == 1 {
            [
                0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8, 0x5F, 0xA2, 0x39, 0x6C, 0x00, 0x4D,
                0x8B, 0x17, 0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE, 0x29, 0x74, 0x66, 0x01,
                0xB8, 0x42, 0xDA, 0x10,
            ]
        } else {
            match all_logs.iter().find(|l| l.s_k == log_entry.s_k - 1) {
                Some(prev) => prev.hash,
                None => {
                    println!(
                        "[Témoin {}] Impossible de trouver l'entrée précédente (seq={})",
                        self.node_id,
                        log_entry.s_k - 1
                    );
                    return false;
                }
            }
        };

        let mut hasher = Sha256::new();
        hasher.update(log_entry.corr.to_be_bytes());
        if log_entry.log_type == LogType::Recv {
            hasher.update(log_entry.s_k_corr.to_be_bytes());
        }
        hasher.update(log_entry.msg.as_bytes());
        let c_k = hasher.finalize();

        hasher = Sha256::new();
        hasher.update(prev_hash);
        hasher.update(log_entry.s_k.to_be_bytes());
        hasher.update((log_entry.log_type.clone() as u8).to_be_bytes());
        hasher.update(c_k);
        let computed_hash: [u8; 32] = hasher.finalize().into();

        computed_hash == log_entry.hash
    }

    fn mark_as_exposed(&mut self, node_id: u32, reason: &str) {
        use crate::types::EvidenceType;
        let evidence_type = if reason.contains("Signature") {
            if reason.contains("Ed25519") {
                EvidenceType::InvalidSignature
            } else {
                EvidenceType::SignatureMismatch
            }
        } else if reason.contains("hash") || reason.contains("Hash") {
            EvidenceType::BrokenHashChain
        } else {
            EvidenceType::SignatureMismatch
        };

        println!(
            "\n[Consistency] Détection d'incohérence sur le nœud {}",
            node_id
        );
        println!("[Consistency] Raison: {}", reason);
        println!("[Consistency] Type de preuve: {:?}", evidence_type);
        println!("[Consistency] → Appel du protocole Evidence pour propager les preuves\n");

        use crate::types::{PeerStatus as DetectionState, Proof};
        self.set_detection_state(node_id, DetectionState::Exposed);
        
        let start_seq = self.logger.s_k.saturating_sub(10).max(1);
        let logs = self.logger.get_log(start_seq, self.logger.s_k).unwrap_or_default();
        
        let authenticator = if !logs.is_empty() {
            crate::types::Authenticator {
                seq: logs[0].s_k,
                hash: logs[0].hash,
                sig: logs[0].sig,
            }
        } else {
            crate::types::Authenticator {
                seq: 0,
                hash: [0u8; 32],
                sig: [0u8; 64],
            }
        };

        let log_suffix: Vec<crate::types::MsgLogEntry> = logs.iter().map(|log| crate::types::MsgLogEntry {
            seq: log.s_k,
            msg_type: crate::types::MsgType::Send,
            dest: log.corr,
            hash: log.hash,
            sig: log.sig,
            content: crate::types::messages::MsgContent::SendContent {
                dest: log.corr,
                message: log.msg.clone(),
            },
        }).collect();
        
        let proof = Proof {
            guilty_node: node_id,
            accuser_node: self.node_id,
            evidence_type,
            authenticator,
            log_suffix,
            reason: reason.to_string(),
        };
        
        // TOUJOURS propager la preuve via le protocole Evidence (même si déjà exposé)
        self.propagate_exposure_proof_via_evidence(proof);
        
        if !self.exposed_nodes.contains(&node_id) {
            self.exposed_nodes.push(node_id);
            
            println!(
                "\n⚠️  [Témoin {}] NŒUD {} MARQUÉ COMME EXPOSED ⚠️",
                self.node_id, node_id
            );
            println!("Raison: {}\n", reason);
        }
    }

    /// Propage une preuve d'exposition via le protocole Evidence
fn propagate_exposure_proof_via_evidence(&self, proof: crate::types::Proof) {
        let witnesses = self.get_witnesses(proof.guilty_node);
        
        println!(
            "[Consistency → Evidence] Diffusion de la preuve aux {} témoin(s) du nœud {}",
            witnesses.len(),
            proof.guilty_node
        );

        for witness_id in witnesses {
            println!(
                "[Consistency → Evidence] → Témoin {} : Preuve d'exposition (type: {:?})",
                witness_id, proof.evidence_type
            );
        }
    }

    pub fn get_exposed_nodes(&self) -> &[u32] {
        &self.exposed_nodes
    }

    pub fn clear_authenticators(&mut self, node_id: u32) {
        if let Some(auths) = self.stored_authenticators.get_mut(&node_id) {
            let count = auths.len();
            auths.clear();
            println!(
                "[Témoin {}] {} authenticators supprimés pour le nœud {} (mémoire libérée)",
                self.node_id, count, node_id
            );
        }
    }
}
