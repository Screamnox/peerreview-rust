
use ed25519_dalek::Verifier;

use crate::{journal::{entry::LogType, logger}, protocols, types::{
    Challenge, PeerReviewMsg, PeerStatus, Proof, messages::{
        Authenticator, ChallengeRequest, EvidenceRequest, EvidenceResponse, EvidenceType
    }, node::{Node, NodeId}
}};

impl Node {
    /// Receive challenge signal from consistency/audit protocols (algo 15)
    /// This is called when a node learns about a challenge (e.g., from witnesses)
    pub fn receive_challenge_signal(&mut self, challenge: Challenge) {
        self.set_peer_status(challenge.target, PeerStatus::Suspected);
        self.add_challenge(challenge);
    }

    /// If a node receives a message from a suspected node,
    /// challenge them with all pending challenges
    pub fn handle_message_from_suspected_node(
        &mut self,
        sender: NodeId
    ) -> std::io::Result<()> {
        if !self.is_suspected(sender) {
            return Ok(());  // TODO Fix
        }

        let challenges = self.get_challenges(sender)
            .unwrap_or_default();   // TODO

        for challenge in challenges {
            self.send(sender, PeerReviewMsg::ChallengeRequest(
                ChallengeRequest { challenge },
            ))?
        }

        Ok(())
    }

    // TODO maybe remove
    /// Periodically request evidence from witnesses about nodes
    pub fn periodic_evidence_collection(&mut self) -> std::io::Result<()> {
        // For each node we communicate with (directly or indirectly)
        for (peer_id, _) in self.peers.clone() {
            self.request_evidence_from_witnesses(peer_id)?;
        }
        Ok(())
    }

    /// Request evidence from witnesses about a target node
    pub fn request_evidence_from_witnesses(
        &mut self,
        target: NodeId,
    ) -> std::io::Result<()> {
        let witnesses = self.get_witnesses(target);
        
        if witnesses.is_empty() {
            return Ok(());
        }

        let msg = PeerReviewMsg::EvidenceRequest(
            EvidenceRequest { target }
        );

        for witness_id in witnesses {
            self.send(witness_id, msg.clone())?;
        }

        Ok(())
    }

    pub fn handle_evidence_request(
        &self,
        requester: NodeId,
        request: &EvidenceRequest,
    ) -> std::io::Result<()> {
        let target = request.target;

        let challenges = self.get_challenges(target)
            .unwrap_or_default();   // TODO

        let proofs = self.get_proofs(target)
            .unwrap_or_default();  // TODO

        let response = EvidenceResponse {
            target,
            challenges,
            proofs,
        };

        let msg = PeerReviewMsg::EvidenceResponse(response);
        self.send(requester, msg)
    }

    /// Process evidence response
    pub fn handle_evidence_response(
        &mut self,
        sender: NodeId,
        response: &EvidenceResponse,
    ) -> std::io::Result<()> {
        for challenge in &response.challenges {
            self.receive_challenge_signal(challenge.clone());
        }

        for proof in &response.proofs {
            self.recv_exposure_proof(sender, proof);
        }

        // Algorithm 15: If we receive responses to all challenges and they're valid,
        // mark as trusted. If any proof shows exposure, mark as exposed.
        // This happens automatically in recv_exposure_proof

        Ok(())
    }

    pub fn recv_exposure_proof(&mut self, sender: NodeId, proof: &Proof) -> bool {
        // Note: there no counter proof if invalid, just discarded
        if !self.verify_exposure_proof(proof) {
            return false;
        }

        self.set_peer_status(proof.faulty_node, PeerStatus::Exposed);
        self.add_proof(&proof.faulty_node, proof.clone());
        
        // If there's a challenge key, we can remove related challenges
        if let Some(key) = proof.challenge_key {
            self.remove_challenge(proof.faulty_node, key);
        }

        true
    }

    fn verify_exposure_proof(&self, proof: &Proof) -> bool {
        // Check that accuser is a witness of the faulty node
        // let witnesses = self.get_witnesses(proof.faulty_node);
        // if !witnesses.contains(&proof.accuser_node) {
        //     return false;
        // }

        let faulty_pk = match self.get_peer_public_key(proof.faulty_node) {
            Some(pk) => pk,
            None => {
                return false;
            }
        };

        if !proof.authenticator.verify(faulty_pk) {
            return false;
        }

        match &proof.kind {
            Some(EvidenceType::BrokenHashChain) => {
                self.verify_broken_hash_chain_proof(proof, faulty_pk)
            }
            Some(EvidenceType::InvalidSignature) => {
                self.verify_invalid_signature_proof(proof, faulty_pk)
            }
            Some(EvidenceType::SignatureMismatch) => {
                self.verify_signature_mismatch_proof(proof, faulty_pk)
            }
            Some(EvidenceType::InvalidAckResponse) => {
                self.verify_invalid_ack_proof(proof, faulty_pk)
            }
            Some(EvidenceType::MissingLogEntries) => {
                self.verify_missing_entries_proof(proof, faulty_pk)
            }
            None => {
                // Generic proof without specific type
                // Just authenticator verified
                true
            }
        }
    }

    fn verify_broken_hash_chain_proof(
        &self, 
        proof: &Proof, 
        _faulty_pk: &ed25519_dalek::PublicKey
    ) -> bool {
        let log_suffix = match &proof.log_suffix {
            Some(logs) => logs,
            None => {
                return false;
            }
        };

        if log_suffix.is_empty() {
            return false;
        }

        // Verify that the hash chain is actually broken
        // This means recalculating hashes and finding a mismatch
        for i in 1..log_suffix.len() {
            let prev_entry = &log_suffix[i - 1];
            let curr_entry = &log_suffix[i];

            // Calculate commitment hash for current entry
            let commitment_hash = match curr_entry.log_type {
                LogType::Send => {
                    protocols::commitment::calculate_send_content_hash(
                        curr_entry.corr,
                        &curr_entry.msg,
                    )
                }
                LogType::Recv => {
                    protocols::commitment::calculate_recv_content_hash(
                        curr_entry.corr,
                        curr_entry.s_k_corr,
                        &curr_entry.msg,
                    )
                }
            };

            // Calculate expected hash
            let expected_hash = protocols::commitment::calculate_hash(
                prev_entry.hash,
                curr_entry.s_k,
                curr_entry.log_type,
                commitment_hash,
            );

            // If hash doesn't match, we found the break - proof is valid
            if expected_hash != curr_entry.hash {
                return true;
            }
        }

        false
    }

    /// Verify invalid signature proof
    fn verify_invalid_signature_proof(
        &self,
        proof: &Proof,
        faulty_pk: &ed25519_dalek::PublicKey
    ) -> bool {
        // The authenticator in the proof should have an INVALID signature
        // We already verified it's signed correctly above (for the authenticator itself)
        // Now we need to check if the log entries have invalid signatures
        
        let log_suffix = match &proof.log_suffix {
            Some(logs) => logs,
            None => {
                return false;
            }
        };

        // Check if any entry has an invalid signature
        let mut prev_hash = logger::HASH_INIT;
        for entry in log_suffix {
            let commitment_hash = match entry.log_type {
                LogType::Send => {
                    protocols::commitment::calculate_send_content_hash(
                        entry.corr,
                        &entry.msg,
                    )
                }
                LogType::Recv => {
                    protocols::commitment::calculate_recv_content_hash(
                        entry.corr,
                        entry.s_k_corr,
                        &entry.msg,
                    )
                }
            };

            let hash = protocols::commitment::calculate_hash(
                prev_hash,
                entry.s_k,
                entry.log_type,
                commitment_hash,
            );

            prev_hash = hash;

            let mut signed_data = [0u8; 40];
            signed_data[..8].copy_from_slice(&entry.s_k.to_be_bytes());
            signed_data[8..].copy_from_slice(&hash);

            let sig = match ed25519_dalek::Signature::from_bytes(&entry.sig) {
                Ok(s) => s,
                Err(_) => {
                    return true;  // Invalid signature format proves the claim
                }
            };

            if faulty_pk.verify(&signed_data, &sig).is_err() {
                return true;  // Invalid signature proves the claim
            }
        }

        false
    }

    /// Verify signature mismatch proof
    fn verify_signature_mismatch_proof(
        &self,
        proof: &Proof,
        faulty_pk: &ed25519_dalek::PublicKey
    ) -> bool {
        // Similar to invalid signature, but specifically for mismatched signatures
        // The authenticator signature doesn't match the expected value
        let expected_signed_data = proof.authenticator.signed_data();
        
        let sig = match ed25519_dalek::Signature::from_bytes(&proof.authenticator.sig) {
            Ok(s) => s,
            Err(_) => return true,  // Invalid format is a mismatch
        };

        // If signature is valid for the data, then there's no mismatch
        if faulty_pk.verify(&expected_signed_data, &sig).is_ok() {
            return false;
        }

        true
    }

    /// Verify invalid ACK response proof
    fn verify_invalid_ack_proof(
        &self,
        proof: &Proof,
        _faulty_pk: &ed25519_dalek::PublicKey
    ) -> bool {
        // For ACK proofs, we need the original send message and the ACK
        // This is harder to verify without more context
        // For now, accept if authenticator is valid (already verified above)
        
        if proof.log_suffix.is_some() {
            true
        } else {
            false
        }
    }

    /// Verify missing log entries proof
    fn verify_missing_entries_proof(
        &self,
        proof: &Proof,
        _faulty_pk: &ed25519_dalek::PublicKey
    ) -> bool {
        // Verify that there's a gap in the log
        let log_suffix = match &proof.log_suffix {
            Some(logs) => logs,
            None => {
                return false;
            }
        };

        if log_suffix.len() < 2 {
            return false;
        }

        // Check for sequence number gaps
        for i in 1..log_suffix.len() {
            let prev_seq = log_suffix[i - 1].s_k;
            let curr_seq = log_suffix[i].s_k;

            if curr_seq != prev_seq + 1 {
                return true;
            }
        }

        false
    }

    /// Propagate exposure proof to witnesses
    pub fn propagate_exposure_proof(&self, proof: &Proof) -> std::io::Result<()> {
        let msg = PeerReviewMsg::ProofBroadcast(
            crate::types::messages::ProofBroadcast {
                proof: proof.clone(),
            }
        );

        let witnesses = self.get_witnesses(proof.faulty_node);
        for witness_id in witnesses {
            self.send(witness_id, msg.clone())?;
        }

        Ok(())
    }

    fn broadcast_exposure_proof(&self, proof: &Proof) {
        let witnesses = self.get_witnesses(proof.faulty_node);

        for witness_id in witnesses {
            let msg = PeerReviewMsg::ProofBroadcast(
                crate::types::messages::ProofBroadcast {
                    proof: proof.clone(),
                }
            );
            let _ = self.send(witness_id, msg);
        }
    }

    pub fn report_invalid_signature_to_evidence(
        &mut self,
        faulty_node: NodeId,
        auth: Authenticator,
        reason: &str
    ) {
        self.set_peer_status(faulty_node, PeerStatus::Exposed);
        
        // TODO Why 5?
        let logs = self.logger
            .get_log(self.logger.s_k.saturating_sub(5), self.logger.s_k)
            .unwrap_or_default();

        let proof = Proof {
            accuser_node: self.id,
            faulty_node,
            authenticator: auth,
            kind: Some(EvidenceType::InvalidSignature),
            log_suffix: Some(logs),
            reason: Some(reason.to_string()),
            challenge_key: None,
        };

        self.broadcast_exposure_proof(&proof);
        self.add_proof(&faulty_node, proof);
    }

    pub fn mark_as_exposed(
        &mut self,
        faulty_node: NodeId,
        auth: Authenticator,
    ) {
        self.set_peer_status(faulty_node, PeerStatus::Exposed);
        
        // TODO Why 10?
        let logs = self.logger
            .get_log(self.logger.s_k.saturating_sub(10), self.logger.s_k)
            .unwrap_or_default();
        
        let proof = Proof {
            accuser_node: self.id,
            faulty_node,
            authenticator: auth,
            log_suffix: Some(logs),
            kind: None,
            reason: None,
            challenge_key: None,
        };
        
        self.broadcast_exposure_proof(&proof);
        self.add_proof(&faulty_node, proof);
    }

    /*
     * Detector API
     */
    // TODO Make it safer than just unwrap
    pub fn is_suspected(&self, node: NodeId) -> bool {
        self.get_peer_status(node).unwrap() == PeerStatus::Suspected
    }

    // Note: By default all nodes are 'Trusted'
    pub fn is_trusted(&self, node: NodeId) -> bool {
        self.get_peer_status(node).unwrap() == PeerStatus::Trusted
    }

    pub fn is_exposed(&self, node: NodeId) -> bool {
        self.get_peer_status(node).unwrap() == PeerStatus::Exposed
    }
}