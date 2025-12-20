use crate::journal::entry::LogEntry;
use std::collections::{HashMap, HashSet};

use super::node::{PeerReviewMessage, PeerReviewNode};

/// Challenge d'audit : contient (α_j_min, α_j_max)
/// Les signatures des entrées minimale et maximale du segment demandé
#[derive(Debug, Clone)]
pub struct AuditChallenge {
    pub challenger_id: u32,           // ID du nœud i qui émet le défi
    pub target_id: u32,               // ID du nœud j défié
    pub sig_min: [u8; 64],            // α_j_min : signature de l'entrée minimale
    pub sig_max: [u8; 64],            // α_j_max : signature de l'entrée maximale
    pub seq_min: usize,               // s_min : numéro séquentiel minimal
    pub seq_max: usize,               // s_max : numéro séquentiel maximal
}

/// Challenge d'envoi : contient (m, α_i_k)
/// Le message non acquitté et sa signature
#[derive(Debug, Clone)]
pub struct SendChallenge {
    pub challenger_id: u32,           // ID du nœud i qui émet le défi
    pub target_id: u32,               // ID du nœud j défié
    pub message: PeerReviewMessage,   // m : message non acquitté
    pub signature: [u8; 64],          // α_i_k : signature du message
}

/// Type de challenge
#[derive(Debug, Clone)]
pub enum Challenge {
    Audit(AuditChallenge),
    Send(SendChallenge),
}

/// Réponse à un challenge d'audit : (e_min, ..., e_max, h_min-1)
#[derive(Debug, Clone)]
pub struct AuditChallengeResponse {
    pub responder_id: u32,            // ID du nœud j qui répond
    pub log_segment: Vec<LogEntry>,   // [e_min, ..., e_max] : segment de log
    pub hash_before_min: [u8; 32],    // h_min-1 : hash avant le segment
}

/// Réponse à un challenge d'envoi : acquittement (h_l-1, s_l, α_j_l)
#[derive(Debug, Clone)]
pub struct SendChallengeResponse {
    pub responder_id: u32,            // ID du nœud j qui répond
    pub hash_prev: [u8; 32],          // h_l-1 : hash précédent l'acquittement
    pub seq_num: usize,               // s_l : numéro séquentiel de l'acquittement
    pub signature: [u8; 64],          // α_j_l : signature de l'acquittement
    pub acknowledgment: PeerReviewMessage, // Message d'acquittement complet
}

/// Type de réponse
#[derive(Debug, Clone)]
pub enum ChallengeResponse {
    Audit(AuditChallengeResponse),
    Send(SendChallengeResponse),
}

/// État de suspicion d'un nœud
#[derive(Debug, Clone, PartialEq)]
pub enum SuspicionState {
    Trusted,      // Nœud de confiance (état par défaut)
    Suspected,    // Nœud suspecté (ne répond pas aux défis)
}

/// Gestionnaire des challenges et des suspicions
pub struct ChallengeManager {
    pub node_id: u32,
    /// État de suspicion de chaque nœud : node_id -> SuspicionState
    pub suspicion_state: HashMap<u32, SuspicionState>,
    /// Témoins pour chaque nœud : node_id -> Set<witness_id>
    /// W(j) = ensemble des témoins du nœud j
    pub witnesses: HashMap<u32, HashSet<u32>>,
    /// Challenges en attente de réponse : target_id -> Challenge
    pub pending_challenges: HashMap<u32, Challenge>,
}

impl ChallengeManager {
    /// Crée un nouveau gestionnaire de challenges
    pub fn new(node_id: u32) -> Self {
        ChallengeManager {
            node_id,
            suspicion_state: HashMap::new(),
            witnesses: HashMap::new(),
            pending_challenges: HashMap::new(),
        }
    }

    /// Enregistre les témoins W(j) pour un nœud j
    pub fn register_witnesses(&mut self, node_id: u32, witness_ids: HashSet<u32>) {
        println!(
            "[ChallengeManager {}] Enregistrement de {} témoins pour le nœud {}",
            self.node_id,
            witness_ids.len(),
            node_id
        );
        self.witnesses.insert(node_id, witness_ids);
    }

    /// Obtient les témoins W(j) d'un nœud j
    pub fn get_witnesses(&self, node_id: u32) -> Option<&HashSet<u32>> {
        self.witnesses.get(&node_id)
    }

    /// Indique l'état suspected(j) pour le nœud j
    pub fn mark_as_suspected(&mut self, node_id: u32) {
        println!(
            "[ChallengeManager {}] État suspected({}) indiqué",
            self.node_id, node_id
        );
        self.suspicion_state
            .insert(node_id, SuspicionState::Suspected);
    }

    /// Lève la suspicion sur un nœud (réponse valide reçue)
    pub fn clear_suspicion(&mut self, node_id: u32) {
        println!(
            "[ChallengeManager {}] Suspicion levée pour le nœud {}",
            self.node_id, node_id
        );
        self.suspicion_state
            .insert(node_id, SuspicionState::Trusted);
    }

    /// Vérifie l'état de suspicion d'un nœud
    pub fn is_suspected(&self, node_id: u32) -> bool {
        matches!(
            self.suspicion_state.get(&node_id),
            Some(SuspicionState::Suspected)
        )
    }

    /// Enregistre un challenge en attente
    pub fn register_pending_challenge(&mut self, target_id: u32, challenge: Challenge) {
        println!(
            "[ChallengeManager {}] Challenge enregistré pour le nœud {}",
            self.node_id, target_id
        );
        self.pending_challenges.insert(target_id, challenge);
    }

    /// Retire un challenge après réponse
    pub fn remove_pending_challenge(&mut self, target_id: u32) -> Option<Challenge> {
        self.pending_challenges.remove(&target_id)
    }
}

impl PeerReviewNode {
    /// Algorithm 11: Déclenchement du challenge de i sur j
    /// 1. i indique l'état suspected(j)
    /// 2. i crée un challenge (audit ou envoi) pour j
    /// 3. envoie le challenge aux W(j)
    pub fn trigger_challenge(
        &mut self,
        challenge_manager: &mut ChallengeManager,
        target_id: u32,
        challenge: Challenge,
    ) -> std::io::Result<()> {
        println!(
            "[Nœud {}] === Algorithm 11: Déclenchement du challenge sur le nœud {} ===",
            self.node_id, target_id
        );

        // Étape 1: i indique l'état suspected(j)
        challenge_manager.mark_as_suspected(target_id);

        // Étape 2: i crée un challenge (audit ou envoi) pour j
        // (le challenge est déjà créé et passé en paramètre)
        let challenge_type = match &challenge {
            Challenge::Audit(c) => format!(
                "AUDIT (seq {} -> {})",
                c.seq_min, c.seq_max
            ),
            Challenge::Send(c) => format!("SEND (seq {})", c.message.seq_num),
        };
        println!(
            "[Nœud {}] Challenge créé: {}",
            self.node_id, challenge_type
        );

        // Enregistrer le challenge en attente
        challenge_manager.register_pending_challenge(target_id, challenge.clone());

        // Étape 3: envoie le challenge aux W(j)
        let witnesses = challenge_manager.get_witnesses(target_id);
        match witnesses {
            Some(witness_set) => {
                println!(
                    "[Nœud {}] Envoi du challenge aux {} témoins de {}",
                    self.node_id,
                    witness_set.len(),
                    target_id
                );
                for witness_id in witness_set {
                    println!(
                        "[Nœud {}] → Challenge envoyé au témoin {}",
                        self.node_id, witness_id
                    );
                    // TODO: Implémenter l'envoi réseau réel du challenge au témoin
                    // Pour l'instant, on log juste l'action
                }
            }
            None => {
                println!(
                    "[Nœud {}] Attention: Aucun témoin enregistré pour le nœud {}",
                    self.node_id, target_id
                );
            }
        }

        // Logger le challenge dans notre journal
        let challenge_msg = format!("CHALLENGE_{}: {}", challenge_type, target_id);
        self.logger.log_send(target_id, &challenge_msg)?;

        // Mettre à jour prev_hash
        let logs = self.logger.get_log(1)?;
        if !logs.is_empty() {
            self.prev_hash = logs[0].hash;
        }

        println!(
            "[Nœud {}] Challenge déclenché avec succès pour le nœud {}",
            self.node_id, target_id
        );

        Ok(())
    }

    /// Algorithm 12: Traitement du challenge par j
    /// Si j reçoit un challenge d'audit avec (α_j_min, α_j_max):
    ///   - j extrait [e_min, ..., e_max]
    ///   - j envoie (e_min, ..., e_max, h_min-1)
    pub fn process_audit_challenge(
        &mut self,
        challenge: &AuditChallenge,
    ) -> std::io::Result<AuditChallengeResponse> {
        println!(
            "[Nœud {}] === Algorithm 12: Traitement du challenge d'audit ===",
            self.node_id
        );
        println!(
            "[Nœud {}] Challenge d'audit reçu de {} (seq {} -> {})",
            self.node_id, challenge.challenger_id, challenge.seq_min, challenge.seq_max
        );

        // Étape 1: j extrait [e_min, ..., e_max]
        let count = challenge.seq_max - challenge.seq_min + 1;
        let all_logs = self.logger.get_log(count)?;

        // Filtrer pour obtenir le segment exact [e_min, ..., e_max]
        let log_segment: Vec<LogEntry> = all_logs
            .into_iter()
            .filter(|entry| entry.s_k >= challenge.seq_min && entry.s_k <= challenge.seq_max)
            .collect();

        if log_segment.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Segment de log [{}, {}] introuvable",
                    challenge.seq_min, challenge.seq_max
                ),
            ));
        }

        println!(
            "[Nœud {}] Segment extrait: {} entrées",
            self.node_id,
            log_segment.len()
        );

        // Vérifier que les signatures correspondent
        let first_entry = &log_segment[0];
        let last_entry = &log_segment[log_segment.len() - 1];

        if first_entry.sig != challenge.sig_min {
            // APPEL AU PROTOCOLE EVIDENCE
            println!(
                "[Challenge_Response] Signature e_min ne correspond pas à α_j_min"
            );
            println!("[Challenge_Response] → Appel du protocole Evidence\n");
            self.report_signature_mismatch_to_evidence(
                challenge.target_id,
                "Signature e_min ne correspond pas à α_j_min dans challenge d'audit"
            );
            
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "La signature de e_min ne correspond pas à α_j_min",
            ));
        }

        if last_entry.sig != challenge.sig_max {
            // APPEL AU PROTOCOLE EVIDENCE
            println!(
                "[Challenge_Response] Signature e_max ne correspond pas à α_j_max"
            );
            println!("[Challenge_Response] → Appel du protocole Evidence\n");
            self.report_signature_mismatch_to_evidence(
                challenge.target_id,
                "Signature e_max ne correspond pas à α_j_max dans challenge d'audit"
            );
            
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "La signature de e_max ne correspond pas à α_j_max",
            ));
        }

        // Étape 2: j envoie (e_min, ..., e_max, h_min-1)
        // Récupérer h_min-1 : le hash de l'entrée précédant e_min
        let hash_before_min = if challenge.seq_min > 0 {
            // Chercher l'entrée précédente
            let prev_logs = self.logger.get_log(challenge.seq_min)?;
            let prev_entry = prev_logs
                .iter()
                .find(|entry| entry.s_k == challenge.seq_min - 1);

            match prev_entry {
                Some(entry) => entry.hash,
                None => {
                    // Si l'entrée précédente n'existe pas, utiliser HASH_INIT
                    const HASH_INIT: [u8; 32] = [
                        0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8, 0x5F, 0xA2, 0x39, 0x6C,
                        0x00, 0x4D, 0x8B, 0x17, 0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE,
                        0x29, 0x74, 0x66, 0x01, 0xB8, 0x42, 0xDA, 0x10,
                    ];
                    HASH_INIT
                }
            }
        } else {
            // Si c'est la première entrée (seq_min = 0), utiliser HASH_INIT
            const HASH_INIT: [u8; 32] = [
                0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8, 0x5F, 0xA2, 0x39, 0x6C, 0x00,
                0x4D, 0x8B, 0x17, 0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE, 0x29, 0x74,
                0x66, 0x01, 0xB8, 0x42, 0xDA, 0x10,
            ];
            HASH_INIT
        };

        println!(
            "[Nœud {}] Réponse préparée: {} entrées + h_min-1 = {}",
            self.node_id,
            log_segment.len(),
            hex::encode(hash_before_min)
        );

        // Logger la réponse
        let response_msg = format!(
            "RESPONSE_AUDIT: {} entrées [{}, {}]",
            log_segment.len(),
            challenge.seq_min,
            challenge.seq_max
        );
        self.logger
            .log_send(challenge.challenger_id, &response_msg)?;

        // Mettre à jour prev_hash
        let logs = self.logger.get_log(1)?;
        if !logs.is_empty() {
            self.prev_hash = logs[0].hash;
        }

        Ok(AuditChallengeResponse {
            responder_id: self.node_id,
            log_segment,
            hash_before_min,
        })
    }

    /// Algorithm 12: Traitement du challenge par j
    /// Si j reçoit un challenge d'envoi avec (m, α_i_k):
    ///   - Si j n'a pas encore reçu m:
    ///     * i accepte m (cf. Réception message)
    ///     * i génère et envoie l'acquittement (h_l-1, s_l, α_j_l)
    ///   - Sinon:
    ///     * j retrouve l'acquittement envoyé à i
    ///     * j renvoie l'acquittement à ses témoins W(j)
    pub fn process_send_challenge(
        &mut self,
        challenge: &SendChallenge,
    ) -> std::io::Result<SendChallengeResponse> {
        println!(
            "[Nœud {}] === Algorithm 12: Traitement du challenge d'envoi ===",
            self.node_id
        );
        println!(
            "[Nœud {}] Challenge d'envoi reçu de {} (msg seq={})",
            self.node_id, challenge.challenger_id, challenge.message.seq_num
        );

        // Vérifier si j a déjà reçu le message m
        let recent_logs = self.logger.get_log(100)?; // Chercher dans les 100 dernières entrées

        // Chercher une entrée RECV pour ce message
        for entry in &recent_logs {
            if entry.corr == challenge.challenger_id
                && entry.s_k_corr == challenge.message.seq_num
            {
                println!(
                    "[Nœud {}] Message déjà reçu (seq={}, s_k_corr={})",
                    self.node_id, entry.s_k, entry.s_k_corr
                );

                // j retrouve l'acquittement envoyé à i
                // L'acquittement est une entrée SEND juste après la RECV
                if let Some(ack_entry) = recent_logs.iter().find(|e| {
                    e.s_k == entry.s_k + 1
                        && e.corr == challenge.challenger_id
                        && e.log_type == crate::journal::entry::LogType::Send
                }) {
                    println!(
                        "[Nœud {}] Acquittement retrouvé (s_l={})",
                        self.node_id, ack_entry.s_k
                    );

                    // j renvoie l'acquittement à ses témoins W(j)
                    // (implémentation réseau à faire)

                    let ack_msg = PeerReviewMessage {
                        msg_type: super::node::MessageType::Send,
                        seq_num: ack_entry.s_k,
                        prev_hash: entry.hash, // h_l-1 : hash de l'entrée RECV
                        signature: ack_entry.sig,
                        dest: challenge.challenger_id,
                        payload: String::new(),
                    };

                    return Ok(SendChallengeResponse {
                        responder_id: self.node_id,
                        hash_prev: entry.hash,      // h_l-1
                        seq_num: ack_entry.s_k,     // s_l
                        signature: ack_entry.sig,   // α_j_l
                        acknowledgment: ack_msg,
                    });
                }
            }
        }

        // j n'a pas encore reçu m
        println!(
            "[Nœud {}] Message non encore reçu, acceptation du message",
            self.node_id
        );

        // i accepte m (cf. Réception message)
        let ack_msg_opt = self.receive_message(&challenge.message, challenge.challenger_id)?;

        match ack_msg_opt {
            Some(ack_msg) => {
                // Récupérer les informations de l'acquittement depuis le log
                let logs = self.logger.get_log(2)?; // SEND (ack) et RECV

                if logs.len() < 2 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Journal incomplet après réception du message",
                    ));
                }

                let ack_entry = &logs[0]; // Dernière entrée = SEND (acquittement)
                let recv_entry = &logs[1]; // Avant-dernière = RECV

                println!(
                    "[Nœud {}] Acquittement généré: h_l-1={}, s_l={}, α_j_l={}",
                    self.node_id,
                    hex::encode(recv_entry.hash),
                    ack_entry.s_k,
                    hex::encode(ack_entry.sig)
                );

                // i génère et envoie l'acquittement (h_l-1, s_l, α_j_l)
                Ok(SendChallengeResponse {
                    responder_id: self.node_id,
                    hash_prev: recv_entry.hash,   // h_l-1
                    seq_num: ack_entry.s_k,       // s_l
                    signature: ack_entry.sig,     // α_j_l
                    acknowledgment: ack_msg,
                })
            }
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Impossible de générer un acquittement pour ce message",
            )),
        }
    }

    /// Crée un challenge d'audit avec les signatures min et max
    pub fn create_audit_challenge_cr(
        &self,
        target_id: u32,
        seq_min: usize,
        seq_max: usize,
        sig_min: [u8; 64],
        sig_max: [u8; 64],
    ) -> Result<AuditChallenge, String> {
        if seq_min >= seq_max {
            return Err(format!(
                "Intervalle invalide: seq_min={} doit être < seq_max={}",
                seq_min, seq_max
            ));
        }

        Ok(AuditChallenge {
            challenger_id: self.node_id,
            target_id,
            sig_min,
            sig_max,
            seq_min,
            seq_max,
        })
    }

    /// Crée un challenge d'envoi avec un message non acquitté
    pub fn create_send_challenge_cr(
        &self,
        target_id: u32,
        message: PeerReviewMessage,
        signature: [u8; 64],
    ) -> SendChallenge {
        SendChallenge {
            challenger_id: self.node_id,
            target_id,
            message,
            signature,
        }
    }

    /// Signale une incompatibilité de signature au protocole Evidence
    fn report_signature_mismatch_to_evidence(&mut self, faulty_node_id: u32, reason: &str) {
        use super::evidence::EvidenceType;
        use super::node::DetectionState;
        
        // Marquer le nœud comme exposé
        self.set_detection_state(faulty_node_id, DetectionState::Exposed);
        
        // Récupérer les logs comme preuve
        let logs = self.logger.get_log(10).unwrap_or_default();
        
        // Créer une preuve d'exposition
        use super::evidence::ExposureProof;
        let _proof = ExposureProof {
            witness_id: self.node_id,
            exposed_node_id: faulty_node_id,
            evidence_type: EvidenceType::SignatureMismatch,
            logs,
            reason: reason.to_string(),
        };
        
        // Propager via Evidence
        let witnesses = self.get_witnesses(faulty_node_id);
        
        println!(
            "[Challenge_Response → Evidence] Diffusion de la preuve aux {} témoin(s) du nœud {}",
            witnesses.len(),
            faulty_node_id
        );

        for witness_id in witnesses {
            println!(
                "[Challenge_Response → Evidence] → Témoin {} : Preuve de signature différente",
                witness_id
            );
            // Dans une vraie implémentation, envoyer via le réseau
            // self.network.send_evidence_proof(witness_id, proof.clone());
        }
    }
}
