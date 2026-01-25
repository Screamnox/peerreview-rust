use std::io;

use crate::{
    journal::entry::LogEntry,
    types::{
        PeerReviewMsg,
        messages::{AuditRequest, AuditResponse},
        node::{Node, NodeId},
    },
};

/// Snapshot
#[derive(Clone)]
pub struct Snapshot {
    // État applicatif minimal nécessaire pour rejouer la machine à états
}

impl Snapshot {
    pub fn new() -> Self {
        Snapshot {}
    }

    pub fn run() {
        unimplemented!();
    }
}

/// Audit protocol
impl Node {
    /// Witness - Send Audit Request
    pub fn send_audit_request(&mut self, target: NodeId) -> io::Result<()> {
        let auths = match self.stored_authenticators.get(&target) {
            Some(a) => a,
            None => return Ok(()), // TODO
        };

        let recent_seq = match auths.last() {
            Some(a) => a.seq,
            None => return Ok(()), // TODO
        };

        let last_audit_seq = match self.get_peer_last_audit_seq(target) {
            Some(seq) => seq,
            None => return Ok(()),
        };
        // TODO: update in recv the last_audit_seq

        let msg = PeerReviewMsg::AuditRequest(AuditRequest {
            min_seq: last_audit_seq,
            max_seq: recent_seq,
        });

        self.send(target, msg)
    }

    /// Target - Recv audit request
    pub fn recv_audit_request(&mut self, sender: NodeId, pr_msg: &PeerReviewMsg) -> io::Result<()> {
        let req = match pr_msg {
            PeerReviewMsg::AuditRequest(r) => r,
            _ => return Ok(()),
        };

        let logs = self.logger.get_log(req.min_seq, req.max_seq)?;

        let resp = PeerReviewMsg::AuditResponse(AuditResponse {
            entries: logs,
            prev_hash: self.logger.get_current_hash(),
        });

        self.send(sender, resp)
    }

    /// Witness - recv audit response
    pub fn recv_audit_response(
        &mut self,
        target: NodeId,
        pr_msg: &PeerReviewMsg,
    ) -> io::Result<()> {
        let resp = match pr_msg {
            PeerReviewMsg::AuditResponse(r) => r,
            _ => return Ok(()),
        };

        self.replay_and_verify(target, &resp.entries)
    }

    /// Replay and verify
    fn replay_and_verify(&mut self, target: NodeId, logs: &[LogEntry]) -> io::Result<()> {
        let snapshot = self
            .snapshots
            .get(&target)
            .and_then(|v| v.last())
            .cloned()
            .unwrap_or_else(Snapshot::new);

        let replayed_logs = self.replay_from_snapshot(snapshot, logs)?;

        if replayed_logs.len() != logs.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Replay length mismatch",
            ));
        }

        for (a, b) in logs.iter().zip(replayed_logs.iter()) {
            if a != b {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Replay divergence detected",
                ));
            }
        }

        self.create_snapshot(target);
        Ok(())
    }

    /* Helper functions */

    fn replay_from_snapshot(
        &self,
        _snapshot: Snapshot,
        logs: &[LogEntry],
    ) -> io::Result<Vec<LogEntry>> {
        // Dépend entièrement de l’application
        Ok(logs.to_vec())
    }

    fn create_snapshot(&mut self, target: NodeId) {
        let snap = Snapshot::new();

        self.snapshots.entry(target).or_default().push(snap);
    }
}
