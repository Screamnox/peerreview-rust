use ed25519_dalek::Verifier;
use sha2::{Digest, Sha256};

use crate::journal::entry::LogType;
use crate::types::messages::{Authenticator, AuthenticatorBroadcast, SendMsg};
use crate::types::{Node, PeerReviewMsg, node::NodeId};

impl Node {
    /// SEND
    pub fn send_message(
        &mut self,
        dest: NodeId,
        msg: &str,
    ) -> std::io::Result<()> {
        let prev_hash = self.logger.get_current_hash();

        let sig =
            self.logger.log_send(dest, msg, &mut self.keypair)?;

        let pr_msg = PeerReviewMsg::Send(SendMsg {
            seq: self.logger.s_k,
            prev_hash, 
            sig,
            dest,
            msg: msg.to_string(),
        });

        self.send(dest, pr_msg)
    }

    /// Verify SEND (algorithm 3)
    fn verify_send_message(
        &mut self,
        sender_id: NodeId,
        pr_msg: &PeerReviewMsg,
    ) -> bool {
        let send_msg = match pr_msg {
            PeerReviewMsg::Send(payload) => payload,
            _ => return false,
        };

        let mut hasher = Sha256::new();
        hasher.update(send_msg.dest.to_be_bytes());
        hasher.update(send_msg.msg.as_bytes());
        let c_k = hasher.finalize();

        let mut hasher = Sha256::new();
        hasher.update(send_msg.prev_hash);
        hasher.update(send_msg.seq.to_be_bytes());
        hasher.update((LogType::Send as u8).to_be_bytes());
        hasher.update(c_k);

        let h_k: [u8; 32] = hasher.finalize().into();

        let mut signed_data = [0u8; 40];
        signed_data[..8].copy_from_slice(&send_msg.seq.to_be_bytes());
        signed_data[8..].copy_from_slice(&h_k);

        let sig = match ed25519_dalek::Signature::from_bytes(&send_msg.sig) {
            Ok(s) => s,
            Err(_) => return false,
        };

        let sender_pk = match self.get_peer_public_key(sender_id) {
            Some(sender_public_key) => sender_public_key,
            None => return false,
        }.clone();
        
        if sender_pk.verify(&signed_data, &sig).is_err() {
            // TODO
            self.report_invalid_signature_to_evidence(
                sender_id,
                Authenticator { seq: send_msg.seq, hash: h_k, sig: send_msg.sig },
                "Invalid SEND signature",
            );
            return false;
        }

        true
    }

    /// RECV
    pub fn recv_message(
        &mut self,
        sender_id: NodeId,
        pr_msg: &PeerReviewMsg,
    ) -> std::io::Result<()> {
        // TODO: Missing verification if no resposes after T times

        let send_msg = match pr_msg {
            PeerReviewMsg::Send(payload) => payload,
            _ => return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid msg type"
            )),
        };

        let sender_auth = Authenticator {
            seq: send_msg.seq,
            hash: calculate_hash(
                send_msg.prev_hash,
                send_msg.seq,
                LogType::Send,
                calculate_send_content_hash(send_msg.dest, &send_msg.msg)
            ),
            sig: send_msg.sig,
        };      

        if !self.verify_send_message(sender_id, pr_msg) {
            let _ = self.create_send_challenge(
                sender_id,
                send_msg.clone(),
                sender_auth.clone()
            );  // TODO

            return Ok(());  // TODO
        }

        self.send_to_witnesses(
            sender_id, 
            PeerReviewMsg::AuthenticatorBroadcast(
                AuthenticatorBroadcast {
                    auth_node: sender_id,
                    auth: sender_auth.clone(),
                }
            )
        )?;

        self.logger.log_recv(
            sender_id, 
            send_msg.seq,
            send_msg.sig,
            &send_msg.msg,
        )?;

        let recv_log =
            &self.logger.get_log(self.logger.s_k, self.logger.s_k)?[0];
        
        let ack_signature =
            self.logger.log_send(sender_id, "", &mut self.keypair)?;

        let ack = PeerReviewMsg::Ack(SendMsg { 
            seq: self.logger.s_k,
            prev_hash: recv_log.hash,
            sig: ack_signature,
            dest: sender_id,
            msg: String::new(),
        });

        self.send(sender_id, ack)
    }

    /// Verify ACK (algorithm 2)
    pub fn verify_ack(
        &mut self,
        receiver_id: NodeId,
        pr_msg: &PeerReviewMsg,
    ) -> bool {
        let ack = match pr_msg {
            PeerReviewMsg::Ack(payload) => payload,
            _ => return false,
        };

        let receiver_pk = match self.get_peer_public_key(receiver_id) {
            Some(receiver_public_key) => receiver_public_key,
            None => return false,
        }.clone();

        // TODO: Maybe fill with ack content
        let mut hasher = Sha256::new();
        hasher.update(self.id.to_be_bytes());  // ack receiver (us)
        hasher.update(b"");     // empty msg
        let c = hasher.finalize();

        let mut hasher = Sha256::new();
        hasher.update(ack.prev_hash);
        hasher.update(ack.seq.to_be_bytes());
        hasher.update((LogType::Send as u8).to_be_bytes());
        hasher.update(c);

        let h: [u8; 32] = hasher.finalize().into();

        let mut signed = [0u8; 40];
        signed[..8].copy_from_slice(&ack.seq.to_be_bytes());
        signed[8..].copy_from_slice(&h);

        let sig = match ed25519_dalek::Signature::from_bytes(&ack.sig) {
            Ok(s) => s,
            Err(_) => return false,
        };

        if receiver_pk.verify(&signed, &sig).is_err() {
            // TODO
            self.report_invalid_signature_to_evidence(
                receiver_id,
                Authenticator { seq: ack.seq, hash: h, sig: ack.sig },
                "Invalid ACK signature",
            );
            return false;
        }

        // Note: Dans une implémentation complète, on devrait aussi vérifier que recv_hash (hl)
        // correspond bien à une entrée RECV contenant notre message original.
        // Cela nécessiterait soit de recevoir l'entrée RECV complète, soit d'avoir accès
        // au prev_hash de l'entrée RECV (hl-1) pour recalculer hl.
        // Pour l'instant, on fait confiance à la signature de l'acquittement.

        true
    }
}

pub fn calculate_hash(
    prev_hash: [u8; 32],
    seq: usize,
    log_type: LogType,
    content_hash: [u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash);
    hasher.update(seq.to_be_bytes());
    hasher.update((log_type as u8).to_be_bytes());
    hasher.update(content_hash);
    
    hasher.finalize().into()
}

pub fn calculate_send_content_hash(dest: NodeId, msg: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(dest.to_be_bytes());
    hasher.update(msg.as_bytes());
    hasher.finalize().into()
}

pub fn calculate_recv_content_hash(
    sender_id: NodeId,
    sender_seq: usize,
    msg: &str,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(sender_id.to_be_bytes());
    hasher.update(sender_seq.to_be_bytes());
    hasher.update(msg.as_bytes());
    hasher.finalize().into()
}