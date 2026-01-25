use ed25519_dalek::Verifier;

use crate::{
    journal::{
        entry::{LogEntry, LogType},
        logger,
    },
    protocols,
    types::{
        ChallengeKind, PeerReviewMsg, PeerStatus, Proof,
        messages::{
            AuditChallenge, AuditResponse, Authenticator, Challenge, ChallengeAnswer, ChallengeId,
            ChallengeKey, ChallengeRequest, ChallengeResponse, EvidenceType, SendChallenge,
            SendMsg, SendResponse,
        },
        node::{Node, NodeId},
    },
};

impl Node {
    /// Send challenge to witnesses of target node
    pub fn send_challenge(&mut self, target: NodeId, challenge: Challenge) -> std::io::Result<()> {
        self.set_peer_status(target, PeerStatus::Suspected);

        let msg = PeerReviewMsg::ChallengeRequest(ChallengeRequest {
            challenge: challenge.clone(),
        });

        self.add_challenge(challenge);

        self.send_to_witnesses(target, msg)
    }

    /// Witness recv challenge and forwards to target
    pub fn witnesses_recv_challenge(&mut self, challenge: Challenge) -> std::io::Result<()> {
        let msg = PeerReviewMsg::ChallengeRequest(ChallengeRequest {
            challenge: challenge.clone(),
        });

        // TODO check validity of challenge

        self.send(challenge.target, msg)?;

        self.add_challenge(challenge);

        Ok(())
    }

    /// Target recv and processes challenge
    pub fn recv_challenge(&mut self, sender: NodeId, msg: &PeerReviewMsg) -> std::io::Result<()> {
        let challenge = match msg {
            PeerReviewMsg::ChallengeRequest(c) => &c.challenge,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid message type for challenge",
                ));
            }
        };

        let response = match &challenge.kind {
            ChallengeKind::Audit(c) => self.handle_audit_challenge(c)?,
            ChallengeKind::Send(c) => self.handle_send_challenge(sender, challenge.key(), c)?,
        };

        self.send_challenge_response(sender, challenge.key(), response)
    }

    fn handle_audit_challenge(&mut self, c: &AuditChallenge) -> std::io::Result<ChallengeAnswer> {
        let logs = self.logger.get_log(c.min_auth.seq, c.max_auth.seq)?;

        let hash_before_min = if c.min_auth.seq > 0 {
            let prev_entries = self
                .logger
                .get_log(c.min_auth.seq - 1, c.min_auth.seq - 1)?;

            if prev_entries.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Cannot find entry before min",
                ));
            }

            prev_entries[0].hash
        } else {
            logger::HASH_INIT
        };

        Ok(ChallengeAnswer::Audit(AuditResponse {
            entries: logs,
            prev_hash: hash_before_min,
        }))
    }

    fn handle_send_challenge(
        &mut self,
        challenger: NodeId,
        challenge_key: ChallengeKey,
        c: &SendChallenge,
    ) -> std::io::Result<ChallengeAnswer> {
        let recv_entry =
            self.find_recv_entry_for_message(challenger, c.sender_auth.seq, &c.message.msg);

        match recv_entry {
            Some(recv_log) => {
                let ack_entry = self.find_ack_entry_after_recv(recv_log.s_k)?;

                let ack_auth = Authenticator {
                    seq: ack_entry.s_k,
                    hash: ack_entry.hash,
                    sig: ack_entry.sig,
                };

                let ack_answer = ChallengeAnswer::Send(SendResponse {
                    auth: ack_auth.clone(),
                });

                let ack_msg = PeerReviewMsg::ChallengeResponse(ChallengeResponse {
                    challenge_key,
                    answer: ack_answer.clone(),
                });

                // TODO check if there not double sending
                self.send_to_witnesses(self.id, ack_msg)?;

                Ok(ack_answer)
            }
            None => {
                let pr_msg = PeerReviewMsg::Send(c.message.clone());

                // TODO check if there not double sending
                self.recv_message(challenger, &pr_msg)?;

                let ack_entry = self.logger.get_log(self.logger.s_k, self.logger.s_k)?[0].clone();

                Ok(ChallengeAnswer::Send(SendResponse {
                    auth: Authenticator {
                        seq: ack_entry.s_k,
                        hash: ack_entry.hash,
                        sig: ack_entry.sig,
                    },
                }))
            }
        }
    }

    fn send_challenge_response(
        &self,
        target: NodeId,
        challenge_key: ChallengeKey,
        answer: ChallengeAnswer,
    ) -> std::io::Result<()> {
        let cr = ChallengeResponse {
            challenge_key,
            answer,
        };
        let msg = PeerReviewMsg::ChallengeResponse(cr);
        self.send(target, msg)
    }

    pub fn recv_challenge_response(
        &mut self,
        sender: NodeId,
        msg: &PeerReviewMsg,
    ) -> std::io::Result<()> {
        let resp = match msg {
            PeerReviewMsg::ChallengeResponse(r) => r,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid message type",
                ));
            }
        };

        match &resp.answer {
            ChallengeAnswer::Audit(a) => {
                self.process_audit_response(sender, resp.challenge_key, &a)?;
            }
            ChallengeAnswer::Send(a) => {
                self.process_send_response(sender, resp.challenge_key, &a)?;
            }
        }

        Ok(())
    }

    fn process_audit_response(
        &mut self,
        sender: NodeId,
        challenge_key: ChallengeKey,
        r: &AuditResponse,
    ) -> std::io::Result<()> {
        let challenge = self.get_challenge(sender, challenge_key).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Challenge ({}, {}) not found for peer {}",
                    challenge_key.0, challenge_key.1, sender
                ),
            )
        })?;

        let (min_auth, max_auth) = match &challenge.kind {
            ChallengeKind::Audit(ac) => (&ac.min_auth, &ac.max_auth),
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Expected audit challenge",
                ));
            }
        };

        let valid = self.verify_audit_response(r, min_auth, max_auth);

        if valid {
            // TODO create function
            self.remove_challenge(sender, challenge_key);

            // Set trust if no more challenge
            if self.get_challenge_count(sender) == 0 {
                self.set_peer_status(sender, PeerStatus::Trusted);
            }
        } else {
            // TODO create function
            self.set_peer_status(sender, PeerStatus::Exposed);

            let proof = Proof {
                accuser_node: self.id,
                faulty_node: sender,
                authenticator: max_auth.clone(),
                log_suffix: Some(r.entries.clone()),
                kind: Some(EvidenceType::BrokenHashChain),
                reason: Some("Audit response hash chain verification failed".to_string()),
                challenge_key: Some(challenge_key),
            };

            // TODO
            self.propagate_exposure_proof(&proof)?;
        }

        Ok(())
    }

    fn verify_audit_response(
        &self,
        r: &AuditResponse,
        min_auth: &Authenticator,
        max_auth: &Authenticator,
    ) -> bool {
        if r.entries.is_empty() {
            return false;
        }

        if r.entries[0].s_k != min_auth.seq {
            return false;
        }

        if r.entries[r.entries.len() - 1].s_k != max_auth.seq {
            return false;
        }

        let mut prev_hash = r.prev_hash;

        for entry in &r.entries {
            let commitment_hash = match entry.log_type {
                LogType::Send => {
                    protocols::commitment::calculate_send_content_hash(entry.corr, &entry.msg)
                }
                LogType::Recv => protocols::commitment::calculate_recv_content_hash(
                    entry.corr,
                    entry.s_k_corr,
                    &entry.msg,
                ),
            };

            let calculated_hash = protocols::commitment::calculate_hash(
                prev_hash,
                entry.s_k,
                entry.log_type,
                commitment_hash,
            );

            if calculated_hash != entry.hash {
                return false;
            }

            prev_hash = entry.hash;
        }

        if r.entries[0].hash != min_auth.hash {
            return false;
        }

        if r.entries[r.entries.len() - 1].hash != max_auth.hash {
            return false;
        }

        true
    }

    fn process_send_response(
        &mut self,
        sender: NodeId,
        challenge_key: ChallengeKey,
        r: &SendResponse,
    ) -> std::io::Result<()> {
        let challenge = self.get_challenge(sender, challenge_key).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Challenge ({}, {}) not found for peer {}",
                    challenge_key.0, challenge_key.1, sender
                ),
            )
        })?;

        let send_challenge = match &challenge.kind {
            ChallengeKind::Send(sc) => sc,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Expected send challenge",
                ));
            }
        };

        let valid = self.verify_send_challenge_response(sender, r, &send_challenge.message);

        if valid {
            self.remove_challenge(sender, challenge_key);

            // Set trusted if no more challenges remain
            if self.get_challenge_count(sender) == 0 {
                self.set_peer_status(sender, PeerStatus::Trusted);
            }
        } else {
            let proof = Proof {
                accuser_node: self.id,
                faulty_node: sender,
                authenticator: r.auth.clone(),
                log_suffix: None,
                kind: Some(EvidenceType::InvalidAckResponse),
                reason: Some("Send challenge ack verification failed".to_string()),
                challenge_key: Some(challenge_key),
            };

            // TODO
            self.propagate_exposure_proof(&proof)?;
        }

        Ok(())
    }

    fn verify_send_challenge_response(
        &self,
        sender: NodeId,
        r: &SendResponse,
        original_send: &SendMsg,
    ) -> bool {
        let sender_pk = match self.get_peer_public_key(sender) {
            Some(pk) => pk,
            None => return false,
        }
        .clone();

        let mut signed_data = [0u8; 40];
        signed_data[..8].copy_from_slice(&r.auth.seq.to_be_bytes());
        signed_data[8..].copy_from_slice(&r.auth.hash);

        let sig = match ed25519_dalek::Signature::from_bytes(&r.auth.sig) {
            Ok(s) => s,
            Err(_) => return false,
        };

        if sender_pk.verify(&signed_data, &sig).is_err() {
            eprintln!("Invalid signature on ACK authenticator");
            return false;
        }

        // Additional verification: The ACK should correspond to receiving our message
        // We'd need to verify the hash chain, but without the full RECV entry,
        // we trust the signature for now
        // In a complete implementation, request the RECV entry and verify fully

        true
    }

    /// Create and send an audit challenge
    pub fn create_audit_challenge(
        &mut self,
        target: NodeId,
        min_auth: Authenticator,
        max_auth: Authenticator,
    ) -> std::io::Result<ChallengeId> {
        let challenge_id = self.generate_challenge_id();

        let challenge = Challenge {
            id: challenge_id,
            challenger: self.id,
            target,
            kind: ChallengeKind::Audit(crate::types::messages::AuditChallenge {
                min_auth,
                max_auth,
            }),
        };

        self.send_challenge(target, challenge)?;
        Ok(challenge_id)
    }

    /// Create and send a send challenge
    pub fn create_send_challenge(
        &mut self,
        target: NodeId,
        message: crate::types::messages::SendMsg,
        sender_auth: Authenticator,
    ) -> std::io::Result<ChallengeId> {
        let challenge_id = self.generate_challenge_id();

        let challenge = Challenge {
            id: challenge_id,
            challenger: self.id,
            target,
            kind: ChallengeKind::Send(crate::types::messages::SendChallenge {
                message,
                sender_auth,
            }),
        };

        self.send_challenge(target, challenge)?;
        Ok(challenge_id)
    }

    /* Helper functions */
    fn find_recv_entry_for_message(
        &mut self,
        sender: NodeId,
        sender_seq: usize,
        message: &str,
    ) -> Option<LogEntry> {
        let start = self.logger.s_k.saturating_sub(self.logger.line_max);
        let logs = self.logger.get_log(start, self.logger.s_k).ok()?;

        logs.into_iter().find(|e| {
            e.log_type == LogType::Recv
                && e.corr == sender
                && e.s_k_corr == sender_seq
                && e.msg == message
        })
    }

    fn find_ack_entry_after_recv(&mut self, recv_seq: usize) -> std::io::Result<LogEntry> {
        let ack_seq = recv_seq + 1;
        let entries = self.logger.get_log(ack_seq, ack_seq)?;

        if entries.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "ACK entry not found",
            ));
        }

        let ack = &entries[0];
        if ack.log_type != LogType::Send {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Expected SEND entry for ACK",
            ));
        }

        Ok(ack.clone())
    }
}
