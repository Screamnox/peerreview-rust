use crate::journal::LogType;
use std::time::Duration;

use super::node::{PeerReviewNode, PeerReviewMessage, MessageType};

impl PeerReviewNode {
    // TODO: Les fonctions de hachage et signature seront implémentées dans le Logger
    // En attendant, on accepte tous les messages comme valides

    /// Algorithm 1: Envoi d'un message m de i vers j
    /// 1. Crée une entrée SEND (ck = {j, m})
    /// 2. Calcule hk = H(hk-1 || sk || SEND || H(ck))
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
    /// TODO: La vérification complète sera implémentée quand le Logger calculera les hash
    pub fn verify_send_message(
        &self,
        msg: &PeerReviewMessage,
        sender_id: u32,
        _sender_public_key: &[u8; 32],
    ) -> bool {
        // Vérifier seulement le type de message pour l'instant
        if msg.msg_type != MessageType::SEND {
            println!("[Nœud {}] Erreur: Type de message incorrect (attendu SEND)", self.node_id);
            return false;
        }

        // TODO: Vérifier hash et signature quand le Logger les implémentera
        println!("[Nœud {}] Message SEND accepté du nœud {} (vérification simplifiée)", self.node_id, sender_id);
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
        
        // Mettre à jour prev_hash avec le hash de l'acquittement
        self.prev_hash = log_entry_ack.hash;

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

        println!("[Nœud {}] Acquittement créé pour le nœud {} (seq={})", 
                 self.node_id, sender_id, log_entry_ack.s_k);

        Ok(Some(ack_msg))
    }

    /// Algorithm 4: Vérification d'un message type RECV (acquittement) de j par i
    /// TODO: La vérification complète sera implémentée quand le Logger calculera les hash
    pub fn verify_recv_message(
        &mut self,
        ack_msg: &PeerReviewMessage,
        receiver_id: u32,
        _receiver_public_key: &[u8; 32],
        _original_seq_num: usize,
        _original_message: &str,
    ) -> bool {
        // Vérifier seulement le type de message pour l'instant
        if ack_msg.msg_type != MessageType::SEND {
            println!("[Nœud {}] Erreur: Type de message incorrect (attendu SEND pour acquittement)", 
                     self.node_id);
            return false;
        }

        // TODO: Vérifier hash et signature quand le Logger les implémentera
        println!("[Nœud {}] Acquittement accepté du nœud {} (vérification simplifiée)", self.node_id, receiver_id);
        true
    }

    /// Crée un challenge d'audit pour signaler un problème
    fn create_audit_challenge(&mut self, target_node: u32, reason: &str) -> std::io::Result<()> {
        let challenge_msg = format!("CHALLENGE_AUDIT: {}", reason);
        self.logger.log(LogType::Send, target_node, &challenge_msg)?;
        
        // Récupérer la dernière entrée pour mettre à jour prev_hash
        let logs = self.logger.get_log(1)?;
        self.prev_hash = logs[0].hash;
        
        println!("[Nœud {}] Challenge d'audit créé pour le nœud {} : {}", 
                 self.node_id, target_node, reason);
        Ok(())
    }

    /// Envoie un message et attend un acquittement avec timeout
    /// Simule l'attente d'un acquittement - à implémenter avec le réseau réel
    pub fn send_with_acknowledgment(
        &mut self,
        receiver_id: u32,
        receiver_public_key: &[u8; 32],
        message: &str,
        _timeout: Duration,
        ack_received: Option<PeerReviewMessage>,
    ) -> std::io::Result<bool> {
        // Envoyer le message
        let _send_msg = self.send_message(receiver_id, message)?;

        // Simuler l'attente du timeout
        println!("[Nœud {}] Timeout - aucun acquittement reçu", self.node_id);

        // Vérifier si un acquittement a été reçu
        if let Some(ack) = ack_received {
            let is_valid = self.verify_recv_message(&ack, receiver_id, receiver_public_key, 0, message);
            if is_valid {
                return Ok(true);
            }
        }

        // Pas d'acquittement ou invalide - créer un challenge d'envoi
        let challenge_msg = "CHALLENGE_SEND: Timeout - pas d'acquittement";
        self.logger.log(LogType::Send, receiver_id, challenge_msg)?;
        
        // Récupérer la dernière entrée pour mettre à jour prev_hash
        let logs = self.logger.get_log(1)?;
        self.prev_hash = logs[0].hash;
        
        println!("[Nœud {}] Challenge d'envoi créé pour le nœud {} : Timeout - pas d'acquittement", 
                 self.node_id, receiver_id);

        Ok(false)
    }
}
