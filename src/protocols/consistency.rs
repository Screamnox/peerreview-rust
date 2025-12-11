use crate::journal::entry::{LogEntry, LogType};
use ed25519_dalek::Verifier;
use sha2::{Digest, Sha256};

use super::node::PeerReviewNode;

/// Challenge de consistency envoyé par un témoin
#[derive(Debug, Clone)]
pub struct ConsistencyChallenge {
    pub witness_id: u32,
    pub target_id: u32,
    pub seq_nums: Vec<usize>, // Numéros de séquence des authenticators stockés
}

impl PeerReviewNode {
    /// [TÉMOIN] Challenge automatique quand le seuil d'authenticators est atteint
    /// Le témoin demande les logs correspondants aux authenticators stockés
    pub fn challenge_witnessed_node(
        &mut self,
        observed_node_id: u32,
    ) -> std::io::Result<Option<ConsistencyChallenge>> {
        if !self.should_challenge(observed_node_id) {
            return Ok(None);
        }

        let stored_auths = self.stored_authenticators.get(&observed_node_id).unwrap();
        let seq_nums: Vec<usize> = stored_auths.iter().map(|a| a.seq_num).collect();

        println!(
            "[Témoin {}] Challenge du nœud {} pour {} authenticators",
            self.node_id,
            observed_node_id,
            seq_nums.len()
        );

        // Logger le challenge
        let challenge_msg = format!("CONSISTENCY_CHALLENGE: {} logs demandés", seq_nums.len());
        self.logger.log_send(observed_node_id, &challenge_msg)?;

        // Mettre à jour prev_hash
        let logs = self.logger.get_log(1)?;
        if !logs.is_empty() {
            self.prev_hash = logs[0].hash;
        }

        Ok(Some(ConsistencyChallenge {
            witness_id: self.node_id,
            target_id: observed_node_id,
            seq_nums,
        }))
    }

    /// [NŒUD SURVEILLÉ] Répond au challenge d'un témoin avec les logs demandés
    pub fn respond_to_challenge(
        &mut self,
        challenge: &ConsistencyChallenge,
    ) -> std::io::Result<Vec<LogEntry>> {
        println!(
            "[Nœud {}] Réponse au challenge du témoin {} pour {} logs",
            self.node_id,
            challenge.witness_id,
            challenge.seq_nums.len()
        );

        // Récupérer tous les logs demandés
        // Note: get_log(n) récupère les n derniers logs, on devrait implémenter get_logs_by_seq()
        let all_logs = self.logger.get_log(self.logger.s_k)?;
        let mut requested_logs = Vec::new();

        for seq in &challenge.seq_nums {
            if let Some(log) = all_logs.iter().find(|l| l.s_k == *seq) {
                requested_logs.push(log.clone());
            }
        }

        // Logger la réponse
        let response_msg = format!(
            "CONSISTENCY_RESPONSE: {} logs envoyés",
            requested_logs.len()
        );
        self.logger.log_send(challenge.witness_id, &response_msg)?;

        // Mettre à jour prev_hash
        let logs = self.logger.get_log(1)?;
        if !logs.is_empty() {
            self.prev_hash = logs[0].hash;
        }

        Ok(requested_logs)
    }

    /// [TÉMOIN] Vérifie la chaîne de hash et les signatures des logs reçus
    /// Retourne true si tout est correct, false si une incohérence est détectée
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

        // Récupérer les authenticators stockés pour ce nœud
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

        // Récupérer la clé publique du nœud surveillé
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

        // Vérifier chaque log
        for log_entry in received_logs {
            // Trouver l'authenticator correspondant
            let stored_auth = match stored_auths.iter().find(|a| a.seq_num == log_entry.s_k) {
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

            // Vérifier que la signature stockée correspond à celle du log
            if stored_auth.signature != log_entry.sig {
                println!(
                    "[Témoin {}] ✗ Signature différente pour seq={}: attendu {} != reçu {}",
                    self.node_id,
                    log_entry.s_k,
                    hex::encode(&stored_auth.signature[..8]),
                    hex::encode(&log_entry.sig[..8])
                );
                self.mark_as_exposed(observed_node_id, "Signature modifiée");
                return false;
            }

            // Recalculer le hash et vérifier
            if !self.verify_log_hash(log_entry, received_logs) {
                println!(
                    "[Témoin {}] ✗ Hash invalide pour seq={}",
                    self.node_id, log_entry.s_k
                );
                self.mark_as_exposed(observed_node_id, "Chaîne de hash brisée");
                return false;
            }

            // Vérifier la signature Ed25519
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
        
        // Nettoyer les authenticators après vérification réussie
        self.clear_authenticators(observed_node_id);
        
        true
    }

    /// Recalcule et vérifie le hash d'une entrée de log
    fn verify_log_hash(&self, log_entry: &LogEntry, all_logs: &[LogEntry]) -> bool {
        // Trouver le hash précédent (hk-1)
        let prev_hash = if log_entry.s_k == 1 {
            // Première entrée, utiliser HASH_INIT
            [
                0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8, 0x5F, 0xA2, 0x39, 0x6C, 0x00, 0x4D,
                0x8B, 0x17, 0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE, 0x29, 0x74, 0x66, 0x01,
                0xB8, 0x42, 0xDA, 0x10,
            ]
        } else {
            // Trouver l'entrée précédente
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

        // Calculer H(ck) où ck = {corr, [s_k_corr], msg}
        let mut hasher = Sha256::new();
        hasher.update(log_entry.corr.to_be_bytes());
        if log_entry.log_type == LogType::Recv {
            hasher.update(log_entry.s_k_corr.to_be_bytes());
        }
        hasher.update(log_entry.msg.as_bytes());
        let c_k = hasher.finalize();

        // Calculer hk = H(hk-1 || sk || log_type || H(ck))
        hasher = Sha256::new();
        hasher.update(prev_hash);
        hasher.update(log_entry.s_k.to_be_bytes());
        hasher.update((log_entry.log_type.clone() as u8).to_be_bytes());
        hasher.update(c_k);
        let computed_hash: [u8; 32] = hasher.finalize().into();

        computed_hash == log_entry.hash
    }

    /// Marque un nœud comme EXPOSED (fautif)
    fn mark_as_exposed(&mut self, node_id: u32, reason: &str) {
        if !self.exposed_nodes.contains(&node_id) {
            self.exposed_nodes.push(node_id);
            println!(
                "\n⚠️  [Témoin {}] NŒUD {} MARQUÉ COMME EXPOSED ⚠️",
                self.node_id, node_id
            );
            println!("Raison: {}\n", reason);
        }
    }

    /// Vérifie si un nœud est exposé
    pub fn is_exposed(&self, node_id: u32) -> bool {
        self.exposed_nodes.contains(&node_id)
    }

    /// Retourne la liste des nœuds exposés
    pub fn get_exposed_nodes(&self) -> &[u32] {
        &self.exposed_nodes
    }

    /// Supprime les authenticators stockés pour un nœud après vérification réussie
    /// Note: En cas de fraude détectée, les authenticators sont CONSERVÉS comme preuve
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
