use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufRead, BufReader},
    path::Path,
};

use ed25519_dalek::VerifyingKey;

use crate::journal::entry::LogEntry;
use crate::journal::logger::Logger;
use crate::types::NodeId;

#[derive(Debug, Clone)]
pub struct AuditVerdict {
    pub ok: bool,
    pub reason: String,
}

impl AuditVerdict {
    pub fn ok() -> Self {
        Self { ok: true, reason: "OK".to_string() }
    }
    pub fn fault(reason: impl Into<String>) -> Self {
        Self { ok: false, reason: reason.into() }
    }
}

pub struct AuditNode {
    pub node_id: NodeId,

    /// Pubkeys for each node log we want to verify
    pub pubkeys: HashMap<NodeId, VerifyingKey>,
}

impl AuditNode {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            pubkeys: HashMap::new(),
        }
    }

    pub fn add_pubkey(&mut self, peer: NodeId, vk: VerifyingKey) {
        self.pubkeys.insert(peer, vk);
    }

    pub fn read_log_entries(path: impl AsRef<Path>) -> io::Result<Vec<LogEntry>> {
        let f = File::open(path)?;
        let r = BufReader::new(f);
        let mut out = Vec::new();

        for line in r.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let e = LogEntry::from_json_line(&line)?;
            out.push(e);
        }
        Ok(out)
    }

    /// Verify one log file given the node's verifying key.
    pub fn verify_one_log(
        &self,
        node: NodeId,
        log_path: impl AsRef<Path>,
        strict_chain: bool,
    ) -> io::Result<AuditVerdict> {
        let vk = self.pubkeys.get(&node).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("missing pubkey for node {}", node))
        })?;

        match Logger::verify_log_file(log_path, vk, strict_chain) {
            Ok(()) => Ok(AuditVerdict::ok()),
            Err(e) => Ok(AuditVerdict::fault(e.to_string())),
        }
    }

    /// Compare two logs structurally (same number of entries and same (seq,kind,peer,hash,prev_hash,sig)).
    /// Useful for "node has two journals" scenario (equivocation detection demo).
    pub fn compare_logs_strict(
        &self,
        log_a: impl AsRef<Path>,
        log_b: impl AsRef<Path>,
    ) -> io::Result<AuditVerdict> {
        let a = Self::read_log_entries(log_a)?;
        let b = Self::read_log_entries(log_b)?;

        if a.len() != b.len() {
            return Ok(AuditVerdict::fault(format!(
                "log length mismatch: {} vs {}",
                a.len(),
                b.len()
            )));
        }

        for (i, (ea, eb)) in a.iter().zip(b.iter()).enumerate() {
            if ea != eb {
                return Ok(AuditVerdict::fault(format!(
                    "log divergence at index {} (seq {}): entries differ",
                    i,
                    ea.seq
                )));
            }
        }

        Ok(AuditVerdict::ok())
    }
}
