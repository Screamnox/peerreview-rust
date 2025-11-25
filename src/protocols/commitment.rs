use crate::journal::LogType;
use crate::journal::entry::LogEntry;
use sha2::{Sha256, Digest};
use std::time::Duration;

use super::node::{PeerReviewNode, PeerReviewMessage, MessageType};

impl PeerReviewNode {
    /// Fonction de hachage pour calculer H(commitment) - utilisée pour la vérification
    /// commitment = {dest, seq_num?, message}
    fn hash_commitment(dest: u32, seq_num: Option<usize>, message: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(dest.to_le_bytes());
        if let Some(seq) = seq_num {
            hasher.update(seq.to_le_bytes());
        }
        hasher.update(message.as_bytes());
        hasher.finalize().into()
    }

    /// Fonction de hachage H(h_prev || s || type || H(commitment)) - utilisée pour la vérification
    fn compute_entry_hash(
        prev_hash: &[u8; 32],
        seq_num: usize,
        log_type: &LogType,
        commitment_hash: &[u8; 32],
    ) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(prev_hash);
        hasher.update(seq_num.to_le_bytes());
        match log_type {
            LogType::Send => hasher.update(b"SEND"),
            LogType::Recv => hasher.update(b"RECV"),
        }
        hasher.update(commitment_hash);
        hasher.finalize().into()
    }

    /// Fonction de signature MAC σ(hash, private_key) - utilisée pour la vérification
    fn compute_signature(hash: &[u8; 32], private_key: &[u8; 32]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(private_key);
        hasher.update(hash);
        hasher.finalize().into()
    }

    /// Algorithm 1: Envoi d'un message m de i vers j
    /// 1. Créer une entrée de log (SEND, ck = {j, m})
    /// 2. Récupérer hk-1, sk, αk depuis le journal
    /// 3. Envoyer le message SEND = {sk, hk-1, αk, m}
    pub fn send_message(
        &mut self,
        receiver_id: u32,
        message: &str,
    ) -> std::io::Result<PeerReviewMessage> {
        // Sauvegarder le prev_hash avant de logger (c'est hk-1)
        let prev_hash_for_msg = self.prev_hash;
        
        // Logger l'entrée - le Logger crée le hash et la signature
        self.logger.log(LogType::Send, receiver_id, message)?;
        
        // Récupérer la dernière entrée via get_log(1)
        let logs = self.logger.get_log(1)?;
        let log_entry = &logs[0];
        
        // Mettre à jour prev_hash avec le hash du journal
        self.prev_hash = log_entry.hash;
        
        // Étape 2-3: Créer le message SEND à envoyer = {sk, hk-1, αk, m}
        let send_msg = PeerReviewMessage {
            msg_type: MessageType::SEND,
            seq_num: log_entry.s_k,
            prev_hash: prev_hash_for_msg,  // hk-1 (hash précédent)
            signature: log_entry.sig,       // αk depuis le journal
            dest: receiver_id,
            payload: message.to_string(),
        };
        
        println!("[Nœud {}] Message SEND créé pour le nœud {} (seq={})", 
                 self.node_id, receiver_id, log_entry.s_k);
        
        Ok(send_msg)
    }

    /// Algorithm 3: Vérification d'un message type SEND de i par j
    /// Vérifie que le hash et la signature du message sont cohérents
    /// Le commitment vérifie seulement la cohérence, ne crée PAS de hash
    pub fn verify_send_message(
        &self,
        msg: &PeerReviewMessage,
        sender_id: u32,
        sender_public_key: &[u8; 32],
    ) -> bool {
        // Étape 1: Vérifier le type de message
        if msg.msg_type != MessageType::SEND {
            println!("[Nœud {}] Erreur: Type de message incorrect (attendu SEND)", self.node_id);
            return false;
        }

        // Étape 2: Reconstruire le commitment ck = {j (dest), m}
        // Pour SEND, dest dans le message est le receveur (nous), donc ck = {self.node_id, message}
        let commitment_hash = Self::hash_commitment(msg.dest, None, &msg.payload);

        // Étape 3: Recalculer ĥk = H(hk-1 || sk || SEND || H(ck))
        let computed_hash = Self::compute_entry_hash(
            &msg.prev_hash,
            msg.seq_num,
            &LogType::Send,
            &commitment_hash,
        );

        // Étape 4-5: Calculer h̄k = σ(ĥk, p(i)) et vérifier
        let expected_signature = Self::compute_signature(&computed_hash, sender_public_key);

        if msg.signature != expected_signature {
            println!("[Nœud {}] Erreur: Signature invalide du nœud {}", self.node_id, sender_id);
            return false;
        }

        println!("[Nœud {}] Message SEND valide du nœud {}", self.node_id, sender_id);
        true
    }

    /// Algorithm 2: Réception d'un message m de i par j
    /// 1. Vérifie la validité du message (Algorithm 3)
    /// 2. Crée une entrée RECV (cl = {i, sk, m})
    /// 3. Crée une entrée SEND pour l'acquittement (cl+1 = {i})
    /// 4. Envoie l'acquittement
    pub fn receive_message(
        &mut self,
        msg: &PeerReviewMessage,
        sender_id: u32,
        sender_public_key: &[u8; 32],
    ) -> std::io::Result<Option<PeerReviewMessage>> {
        // Étape 2: Vérifier la validité du message
        let is_valid = self.verify_send_message(msg, sender_id, sender_public_key);

        if !is_valid {
            // Étape 9: Envoyer un challenge d'audit pour les témoins de i, W(i)
            self.create_audit_challenge(sender_id, "Message SEND invalide")?;
            return Ok(None);
        }

        // Étape 3: Message valide
        // Étape 4: Créer une entrée de log RECV (cl = {i, sk, m})
        self.logger.log(LogType::Recv, sender_id, &msg.payload)?;
        
        // Étape 5: Créer une entrée de log SEND pour l'acquittement (cl+1 = {i})
        self.logger.log(LogType::Send, sender_id, "")?;
        
        // Récupérer les 2 dernières entrées via get_log(2)
        let logs = self.logger.get_log(2)?;
        let log_entry_recv = &logs[0];  // Avant-dernière entrée (RECV)
        let log_entry_ack = &logs[1];   // Dernière entrée (SEND - acquittement)
        
        println!("[Nœud {}] Message RECV loggé (seq={})", self.node_id, log_entry_recv.s_k);

        // Étape 7: Créer le message SEND (acquittement) = {sl+1, hl, αl+1}
        let ack_msg = PeerReviewMessage {
            msg_type: MessageType::SEND,
            seq_num: log_entry_ack.s_k,
            prev_hash: log_entry_recv.hash,  // hl (hash de l'entrée RECV)
            signature: log_entry_ack.sig,    // αl+1 depuis le journal
            dest: sender_id,
            payload: String::new(), // Acquittement vide
        };

        // Mettre à jour prev_hash avec le hash de la dernière entrée (SEND)
        self.prev_hash = log_entry_ack.hash;

        println!("[Nœud {}] Acquittement créé pour le nœud {} (seq={})", 
                 self.node_id, sender_id, log_entry_ack.s_k);

        Ok(Some(ack_msg))
    }

    /// Algorithm 4: Vérification d'un message type RECV (acquittement) de j par i
    /// Vérifie la validité de l'acquittement
    pub fn verify_recv_message(
        &mut self,
        ack_msg: &PeerReviewMessage,
        receiver_id: u32,
        receiver_public_key: &[u8; 32],
        _original_seq_num: usize,
        _original_message: &str,
    ) -> bool {
        // Étape 1-2: Vérifier le type de message
        if ack_msg.msg_type != MessageType::SEND {
            println!("[Nœud {}] Erreur: Type de message incorrect (attendu SEND pour acquittement)", 
                     self.node_id);
            return false;
        }

        // L'acquittement est un SEND avec payload vide
        // Étape 3-4: Construire le commitment pour l'acquittement cl+1 = {i}
        let commitment_ack_hash = Self::hash_commitment(self.node_id, None, "");

        // Calculer ĥl+1 = H(hl || sl+1 || SEND || H(cl+1))
        let computed_hash = Self::compute_entry_hash(
            &ack_msg.prev_hash,
            ack_msg.seq_num,
            &LogType::Send,
            &commitment_ack_hash,
        );

        // Étape 5-6: Calculer h̄l = σ(ĥl+1, p(j)) et vérifier
        let expected_signature = Self::compute_signature(&computed_hash, receiver_public_key);

        if ack_msg.signature != expected_signature {
            println!("[Nœud {}] Erreur: Signature d'acquittement invalide", self.node_id);
            // Étape 8: Créer un challenge d'audit aux témoins de j, W(j)
            if let Err(e) = self.create_audit_challenge(receiver_id, "Acquittement invalide") {
                eprintln!("[Nœud {}] Erreur lors de la création du challenge: {}", self.node_id, e);
            }
            return false;
        }

        // Étape 9-10: Message valide
        println!("[Nœud {}] Acquittement valide du nœud {}", self.node_id, receiver_id);
        true
    }

    /// Fonction complète pour envoyer un message avec gestion de l'acquittement
    /// Combine Algorithm 1 et Algorithm 4
    pub fn send_with_acknowledgment(
        &mut self,
        receiver_id: u32,
        receiver_public_key: &[u8; 32],
        message: &str,
        _timeout: Duration,
        ack_response: Option<PeerReviewMessage>,
    ) -> std::io::Result<bool> {
        // Algorithm 1: Envoyer le message
        let send_msg = self.send_message(receiver_id, message)?;
        
        // Attendre l'acquittement (simulation avec ack_response)
        match ack_response {
            Some(ack) => {
                // Algorithm 4: Vérifier l'acquittement
                let is_valid = self.verify_recv_message(
                    &ack,
                    receiver_id,
                    receiver_public_key,
                    send_msg.seq_num,
                    message,
                );
                
                if is_valid {
                    println!("[Nœud {}] Message envoyé et acquittement reçu avec succès", self.node_id);
                    Ok(true)
                } else {
                    println!("[Nœud {}] Acquittement invalide", self.node_id);
                    Ok(false)
                }
            }
            None => {
                // Pas d'acquittement reçu dans le timeout
                println!("[Nœud {}] Timeout - aucun acquittement reçu", self.node_id);
                self.create_send_challenge(receiver_id, "Timeout - pas d'acquittement")?;
                Ok(false)
            }
        }
    }

    /// Crée un challenge d'envoi pour les témoins W(j)
    pub fn create_send_challenge(&mut self, receiver_id: u32, reason: &str) -> std::io::Result<()> {
        let challenge_msg = format!("CHALLENGE_SEND: {}", reason);
        self.logger.log(LogType::Send, receiver_id, &challenge_msg)?;
        println!("[Nœud {}] Challenge d'envoi créé pour le nœud {} : {}", 
                 self.node_id, receiver_id, reason);
        Ok(())
    }

    /// Crée un challenge d'audit pour les témoins W(i)
    pub fn create_audit_challenge(&mut self, sender_id: u32, reason: &str) -> std::io::Result<()> {
        let challenge_msg = format!("CHALLENGE_AUDIT: {}", reason);
        self.logger.log(LogType::Send, sender_id, &challenge_msg)?;
        println!("[Nœud {}] Challenge d'audit créé pour le nœud {} : {}", 
                 self.node_id, sender_id, reason);
        Ok(())
    }
}
