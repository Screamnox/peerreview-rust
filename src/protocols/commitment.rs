use crate::journal::entry::LogType;
use std::time::Duration;

use super::node::{MessageType, PeerReviewMessage, PeerReviewNode};

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
        // Étape 1: Sauvegarder hk-1 (hash de l'entrée précédente) avant de logger
        let prev_hash_for_msg = self.prev_hash;

        // Étape 2: Logger l'entrée SEND - le Logger calcule automatiquement:
        //   - hk = H(hk-1 || sk || SEND || H(ck))
        //   - αk = signature de (sk || hk)
        self.logger.log_send(receiver_id, message)?;

        // Étape 3: Récupérer les informations de l'entrée qu'on vient de créer
        // On récupère directement depuis le Logger au lieu d'utiliser get_log()
        let logs = self.logger.get_log(1)?;
        let log_entry = &logs[0];
        let current_seq = log_entry.s_k; // sk (numéro de séquence de cette entrée)
        let current_hash = log_entry.hash; // hk (hash de cette entrée)
        let current_sig = log_entry.sig; // αk (signature de cette entrée)
        println!("signature: {}", hex::encode(current_sig));
        // Étape 4: Mettre à jour prev_hash pour la prochaine entrée
        // Le hk actuel devient le hk-1 pour la prochaine entrée
        self.prev_hash = current_hash;

        // Étape 5: Créer le message SEND à envoyer sur le réseau = {sk, hk-1, αk, m}
        let send_msg = PeerReviewMessage {
            msg_type: MessageType::Send,
            seq_num: current_seq,         // sk
            prev_hash: prev_hash_for_msg, // hk-1 (hash de l'entrée PRÉCÉDENTE)
            signature: current_sig,       // αk (signature de l'entrée ACTUELLE)
            dest: receiver_id,
            payload: message.to_string(),
        };

        println!(
            "[Nœud {}] Message SEND créé pour le nœud {} (seq={}, prev_hash={:02x}{:02x}...)",
            self.node_id, receiver_id, current_seq, prev_hash_for_msg[0], prev_hash_for_msg[1]
        );

        Ok(send_msg)
    }

    /// Algorithm 3: Vérification d'un message type SEND de i par j
    /// Vérifie le hash et la signature d'un message SEND
    pub fn verify_send_message(
        &self,
        msg: &PeerReviewMessage,
        sender_id: u32,
        sender_public_key: &ed25519_dalek::PublicKey,
    ) -> bool {
        use ed25519_dalek::Verifier;
        use sha2::{Digest, Sha256};

        // Vérifier le type de message
        if msg.msg_type != MessageType::Send {
            println!(
                "[Nœud {}] Erreur: Type de message incorrect (attendu SEND)",
                self.node_id
            );
            return false;
        }

        // Étape 2: Récupérer hk-1, sk, αk, m
        let prev_hash = msg.prev_hash; // hk-1
        let seq_num = msg.seq_num; // sk
        let signature = msg.signature; // αk
        let message = &msg.payload; // m
        println!(
            "[Nœud {}] Vérification du message SEND (seq={}, prev_hash={:02x}{:02x}...)",
            self.node_id, seq_num, prev_hash[0], prev_hash[1]
        );

        // Étape 3: Calculer ĥk = H(hk-1 || sk || SEND || H(ck))
        // où ck = {j, m}

        // D'abord calculer H(ck) où ck = {receiver_id, message}
        let mut hasher = Sha256::new();
        hasher.update(msg.dest.to_be_bytes()); // j (destinataire du message)
        hasher.update(message.as_bytes()); // m
        let c_k = hasher.finalize();
        println!(
            "[Nœud {}] Calcul de H(ck) pour ck = {{ {}, {} }}",
            self.node_id, msg.dest, message
        );

        // Ensuite calculer ĥk = H(hk-1 || sk || SEND || H(ck))
        hasher = Sha256::new();
        hasher.update(prev_hash); // hk-1
        hasher.update(seq_num.to_be_bytes()); // sk
        hasher.update((LogType::Send as u8).to_be_bytes()); // SEND
        hasher.update(c_k); // H(ck)
        let h_k_computed: [u8; 32] = hasher.finalize().into();
        println!(
            "[Nœud {}] Calcul de ĥk = H(hk-1 || sk || SEND || H(ck)). ĥk = {}",
            self.node_id,
            hex::encode(h_k_computed)
        );

        // Étape 4: Vérifier la signature pour obtenir hk = σ̄i(αk, p(i))
        // La signature est sur (sk || hk)
        let mut signed_data = [0u8; 40];
        signed_data[..8].copy_from_slice(&seq_num.to_be_bytes());
        signed_data[8..].copy_from_slice(&h_k_computed);
        println!(
            "[Nœud {}] Vérification de la signature pour (sk={}, ĥk={})",
            self.node_id,
            seq_num,
            hex::encode(h_k_computed)
        );

        let signature_obj = match ed25519_dalek::Signature::from_bytes(&signature) {
            Ok(sig) => sig,
            Err(_) => {
                println!(
                    "[Nœud {}] Erreur: Format de signature invalide",
                    self.node_id
                );
                return false;
            }
        };

        // Étape 5: Vérifier hk == ĥk en vérifiant la signature
        // verify() retourne Ok si et seulement si:
        //   - La signature αk a été créée avec la clé privée correspondant à sender_public_key
        //   - Les données signées (sk || hk) correspondent exactement à signed_data (sk || ĥk)
        // Donc: verify() réussit ⟺ hk == ĥk
        if sender_public_key
            .verify(&signed_data, &signature_obj)
            .is_err()
        {
            println!(
                "[Nœud {}] ✗ Signature invalide: ĥk = {} ne correspond pas au hk signé par le nœud {}",
                self.node_id,
                hex::encode(h_k_computed),
                sender_id
            );
            return false;
        }

        // La signature est valide, donc hk == ĥk est prouvé mathématiquement
        println!(
            "[Nœud {}] ✓ Signature valide: hk == ĥk = {}",
            self.node_id,
            hex::encode(h_k_computed)
        );

        println!(
            "[Nœud {}] Message SEND vérifié avec succès du nœud {} (seq={})",
            self.node_id, sender_id, seq_num
        );
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
    ) -> std::io::Result<Option<PeerReviewMessage>> {
        // Récupérer la clé publique de l'envoyeur
        let sender_public_key = match self.peer_public_keys.get(&sender_id) {
            Some(key) => key,
            None => {
                println!(
                    "[Nœud {}] Erreur: Clé publique du nœud {} non enregistrée",
                    self.node_id, sender_id
                );
                return Ok(None);
            }
        };

        // Étape 2: Vérifier la validité du message
        let is_valid = self.verify_send_message(msg, sender_id, sender_public_key);

        if !is_valid {
            // Étape 9: Envoyer un challenge d'audit pour les témoins de i, W(i)
            self.create_audit_challenge(sender_id, "Message SEND invalide")?;
            return Ok(None);
        }

        // Étape 3: Message valide
        // Étape 4: Créer une entrée de log RECV (cl = {i, sk, m})
        self.logger
            .log_recv(sender_id, msg.seq_num, msg.signature, &msg.payload)?;

        // Récupérer l'entrée RECV
        let logs_recv = self.logger.get_log(1)?;
        let log_entry_recv = &logs_recv[0];
        println!(
            "[Nœud {}] Message RECV loggé (seq={})",
            self.node_id, log_entry_recv.s_k
        );
        let recv_hash = log_entry_recv.hash;

        // Étape 5: Créer une entrée de log SEND pour l'acquittement (cl+1 = {i})
        self.logger.log_send(sender_id, "")?;

        // Récupérer l'entrée SEND (acquittement)
        let logs_ack = self.logger.get_log(1)?;
        let log_entry_ack = &logs_ack[0];

        // Mettre à jour prev_hash avec le hash de l'acquittement
        self.prev_hash = log_entry_ack.hash;

        // Étape 7: Créer le message SEND (acquittement) = {sl+1, hl, αl+1}
        let ack_msg = PeerReviewMessage {
            msg_type: MessageType::Send,
            seq_num: log_entry_ack.s_k,
            prev_hash: recv_hash,         // hl (hash de l'entrée RECV)
            signature: log_entry_ack.sig, // αl+1 depuis le journal
            dest: sender_id,
            payload: String::new(), // Acquittement vide
        };

        println!(
            "[Nœud {}] Acquittement créé pour le nœud {} (seq={})",
            self.node_id, sender_id, log_entry_ack.s_k
        );

        Ok(Some(ack_msg))
    }

    /// Algorithm 4: Vérification d'un message type RECV (acquittement) de j par i
    /// Vérifie le hash et la signature d'un acquittement (message SEND)
    /// L'acquittement prouve que j a bien reçu notre message en incluant le hash de son entrée RECV
    pub fn verify_recv_message(
        &mut self,
        ack_msg: &PeerReviewMessage,
        receiver_id: u32,
        receiver_public_key: &ed25519_dalek::PublicKey,
        _original_seq_num: usize,
        _original_message: &str,
    ) -> bool {
        use ed25519_dalek::Verifier;
        use sha2::{Digest, Sha256};

        // Vérifier le type de message
        if ack_msg.msg_type != MessageType::Send {
            println!(
                "[Nœud {}] Erreur: Type de message incorrect (attendu SEND pour acquittement)",
                self.node_id
            );
            return false;
        }

        // L'acquittement est un message SEND qui contient:
        // - seq_num: le numéro de séquence de l'acquittement SEND  (sl+1)
        // - prev_hash: le hash de l'entrée RECV (hl)
        // - signature: la signature de l'acquittement SEND (αl+1)

        let ack_seq_num = ack_msg.seq_num; // sl+1
        let recv_hash = ack_msg.prev_hash; // hl (hash de l'entrée RECV)
        let ack_signature = ack_msg.signature; // αl+1

        // Étape 1: Vérifier la signature de l'acquittement SEND
        // L'acquittement SEND est signé sur (sl+1 || hl+1)
        // où hl+1 = H(hl || sl+1 || SEND || H(cl+1))
        // et cl+1 = {i (=nous), ""} (message vide pour l'acquittement)

        // Calculer H(cl+1) où cl+1 = {self.node_id, ""}
        let mut hasher = Sha256::new();
        hasher.update(self.node_id.to_be_bytes()); // destinataire de l'acquittement (nous)
        hasher.update(b""); // message vide
        let c_l_plus_1 = hasher.finalize();

        // Calculer ĥl+1 = H(hl || sl+1 || SEND || H(cl+1))
        hasher = Sha256::new();
        hasher.update(recv_hash); // hl (prev_hash de l'acquittement)
        hasher.update(ack_seq_num.to_be_bytes()); // sl+1
        hasher.update((LogType::Send as u8).to_be_bytes()); // SEND
        hasher.update(c_l_plus_1); // H(cl+1)
        let h_l_plus_1_computed: [u8; 32] = hasher.finalize().into();

        // Vérifier la signature de l'acquittement
        let mut signed_data = [0u8; 40];
        signed_data[..8].copy_from_slice(&ack_seq_num.to_be_bytes());
        signed_data[8..].copy_from_slice(&h_l_plus_1_computed);

        let signature_obj = match ed25519_dalek::Signature::from_bytes(&ack_signature) {
            Ok(sig) => sig,
            Err(_) => {
                println!(
                    "[Nœud {}] Erreur: Format de signature invalide",
                    self.node_id
                );
                return false;
            }
        };

        if receiver_public_key
            .verify(&signed_data, &signature_obj)
            .is_err()
        {
            println!(
                "[Nœud {}] Erreur: Signature invalide de l'acquittement du nœud {}",
                self.node_id, receiver_id
            );
            return false;
        }

        // Note: Dans une implémentation complète, on devrait aussi vérifier que recv_hash (hl)
        // correspond bien à une entrée RECV contenant notre message original.
        // Cela nécessiterait soit de recevoir l'entrée RECV complète, soit d'avoir accès
        // au prev_hash de l'entrée RECV (hl-1) pour recalculer hl.
        // Pour l'instant, on fait confiance à la signature de l'acquittement.

        println!(
            "[Nœud {}] Acquittement vérifié avec succès du nœud {} (seq={})",
            self.node_id, receiver_id, ack_seq_num
        );
        true
    }

    /// Crée un challenge d'audit pour signaler un problème
    fn create_audit_challenge(&mut self, target_node: u32, reason: &str) -> std::io::Result<()> {
        let challenge_msg = format!("CHALLENGE_AUDIT: {}", reason);
        self.logger.log_send(target_node, &challenge_msg)?;

        // Récupérer la dernière entrée pour mettre à jour prev_hash
        let logs = self.logger.get_log(1)?;
        self.prev_hash = logs[0].hash;

        println!(
            "[Nœud {}] Challenge d'audit créé pour le nœud {} : {}",
            self.node_id, target_node, reason
        );
        Ok(())
    }

    /// Envoie un message et attend un acquittement avec timeout
    /// Simule l'attente d'un acquittement - à implémenter avec le réseau réel
    #[allow(dead_code)]
    pub fn send_with_acknowledgment(
        &mut self,
        receiver_id: u32,
        message: &str,
        _timeout: Duration,
        ack_received: Option<PeerReviewMessage>,
    ) -> std::io::Result<bool> {
        // Envoyer le message
        let send_msg = self.send_message(receiver_id, message)?;
        let original_seq = send_msg.seq_num;

        // Simuler l'attente du timeout
        println!("[Nœud {}] Timeout - aucun acquittement reçu", self.node_id);

        // Récupérer la clé publique du récepteur
        let receiver_public_key = match self.peer_public_keys.get(&receiver_id).cloned() {
            Some(key) => key,
            None => {
                println!(
                    "[Nœud {}] Erreur: Clé publique du nœud {} non enregistrée",
                    self.node_id, receiver_id
                );
                return Ok(false);
            }
        };

        // Vérifier si un acquittement a été reçu
        if let Some(ack) = ack_received {
            let is_valid = self.verify_recv_message(
                &ack,
                receiver_id,
                &receiver_public_key,
                original_seq,
                message,
            );
            if is_valid {
                return Ok(true);
            }
        }

        // Pas d'acquittement ou invalide - créer un challenge d'envoi
        let challenge_msg = "CHALLENGE_SEND: Timeout - pas d'acquittement";
        self.logger.log_send(receiver_id, challenge_msg)?;

        // Récupérer la dernière entrée pour mettre à jour prev_hash
        let logs = self.logger.get_log(1)?;
        self.prev_hash = logs[0].hash;

        println!(
            "[Nœud {}] Challenge d'envoi créé pour le nœud {} : Timeout - pas d'acquittement",
            self.node_id, receiver_id
        );

        Ok(false)
    }
}
