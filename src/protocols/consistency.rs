use ed25519_dalek::Verifier;
use sha2::{Digest, Sha256};

use crate::journal::entry::{LogEntry, LogType};
use crate::types::messages::Authenticator;
use crate::types::{
    PeerReviewMsg,
    messages::{AuditRequest, AuditResponse},
    node::{Node, NodeId},
};

impl Node {
    /// Witness - Send Challenge
    /// Note: Called when authenticator threshold is reached
    pub fn send_consistency_challenge(&mut self, target: NodeId) -> std::io::Result<()> {
        let auths = match self.stored_authenticators.get(&target) {
            Some(a) => a,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Empty authenticator store",
                ));
            }
        };

        let min_seq = auths.iter().map(|a| a.seq).min().unwrap();
        let max_seq = auths.iter().map(|a| a.seq).max().unwrap();

        let msg = PeerReviewMsg::ConsistencyRequest(AuditRequest { min_seq, max_seq });

        self.send(target, msg)
    }

    /// Target - Recv challenge
    pub fn recv_consistency_challenge(
        &mut self,
        sender: NodeId,
        pr_msg: &PeerReviewMsg,
    ) -> std::io::Result<()> {
        let req = match pr_msg {
            PeerReviewMsg::ConsistencyRequest(r) => r,
            _ => return Ok(()), // TODO improve error
        };

        // TODO
        // If sender is not one of my witness do not answer

        let logs = self.logger.get_log(req.min_seq, req.max_seq)?;
        // TODO Maybe filter before not after

        let response = PeerReviewMsg::ConsistencyResponse(AuditResponse {
            entries: logs,
            prev_hash: self.logger.get_current_hash(), // it is last_hash not prev_hash
        });

        // TODO: Is it necessary to log it? In the previous code it was
        // self.logger.log_send(challenge.witness_id, &response_msg, &mut self.keypair)?;

        self.send(sender, response)
    }

    /// Witness - Verify response
    pub fn verify_consistency_response(&mut self, target: NodeId, pr_msg: &PeerReviewMsg) -> bool {
        let response = match pr_msg {
            PeerReviewMsg::ConsistencyResponse(r) => r,
            _ => return false,
        };

        let auths = match self.stored_authenticators.get(&target) {
            Some(a) => a,
            None => return false,
        };

        let public_key = match self.get_peer_public_key(target) {
            Some(pk) => pk.clone(),
            None => return false,
        };

        for entry in &response.entries {
            let auth = match auths.iter().find(|a| a.seq == entry.s_k) {
                Some(a) => a,
                None => {
                    self.mark_as_exposed(
                        target,
                        Authenticator {
                            seq: 0,
                            hash: [0u8; 32],
                            sig: [0u8; 64],
                        },
                    ); // TODO check is it possible
                    return false;
                }
            };

            if auth.sig != entry.sig {
                self.mark_as_exposed(target, auth.clone());
                return false;
            }

            if !verify_log_hash(entry, &response.entries) {
                self.mark_as_exposed(target, auth.clone());
                return false;
            }

            if !verify_entry_signature(entry, &public_key) {
                self.mark_as_exposed(target, auth.clone());
                return false;
            }
        }

        self.clear_authenticators(target);

        true
    }
}

/* ==================
 * HELPER FUNCTIONS
 * ==================
 */

fn verify_log_hash(entry: &LogEntry, logs: &[LogEntry]) -> bool {
    let prev_hash = if entry.s_k == 1 {
        // TODO: implement LogEntry::hash_init()
        [
            0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8, 0x5F, 0xA2, 0x39, 0x6C, 0x00, 0x4D,
            0x8B, 0x17, 0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE, 0x29, 0x74, 0x66, 0x01,
            0xB8, 0x42, 0xDA, 0x10,
        ]
    } else {
        match logs.iter().find(|e| e.s_k == entry.s_k - 1) {
            Some(e) => e.hash,
            None => return false,
        }
    };

    let mut hasher = Sha256::new();
    hasher.update(entry.corr.to_be_bytes());

    if entry.log_type == LogType::Recv {
        hasher.update(entry.s_k_corr.to_be_bytes());
    }

    hasher.update(entry.msg.as_bytes());
    let c_k = hasher.finalize();

    let mut hasher = Sha256::new();
    hasher.update(prev_hash);
    hasher.update(entry.s_k.to_be_bytes());
    hasher.update((entry.log_type as u8).to_be_bytes());
    hasher.update(c_k);

    let computed: [u8; 32] = hasher.finalize().into();
    computed == entry.hash
}

fn verify_entry_signature(entry: &LogEntry, public_key: &ed25519_dalek::PublicKey) -> bool {
    let mut signed = [0u8; 40];
    signed[..8].copy_from_slice(&entry.s_k.to_be_bytes());
    signed[8..].copy_from_slice(&entry.hash);

    let sig = match ed25519_dalek::Signature::from_bytes(&entry.sig) {
        Ok(s) => s,
        Err(_) => return false,
    };

    public_key.verify(&signed, &sig).is_ok()
}
