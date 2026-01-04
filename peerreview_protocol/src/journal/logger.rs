use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use signature::{Signer, Verifier};

use crate::journal::entry::LogEntry;
use crate::types::{AuditEvent, NodeId};

pub struct Logger {
    node_id: NodeId,
    seq: u64,
    last_hash: [u8; 32],
    file: File,

    signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
}

impl Logger {
    /// Ouvre un journal (append-only) et restaure l’état (seq + last_hash) si le fichier existe déjà.
    pub fn open(
        node_id: NodeId,
        log_path: impl AsRef<Path>,
        signing_key: SigningKey,
    ) -> std::io::Result<Self> {
        let verifying_key = signing_key.verifying_key();

        // Lire l’existant pour restaurer last_hash/seq
        let (seq, last_hash) = restore_state_from_file(log_path.as_ref()).unwrap_or((0, [0u8; 32]));

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;

        Ok(Self {
            node_id,
            seq,
            last_hash,
            file,
            signing_key,
            verifying_key,
        })
    }

    /// Logguer un événement auditable (Monde A -> Monde B).
    pub fn log_event(&mut self, event: AuditEvent) -> std::io::Result<LogEntry> {
        let ts_ms = now_ms();
        let kind = event.kind().to_string();
        let payload = event.to_string();

        // prev_hash = last_hash
        let prev_hash = self.last_hash;

        // hash = SHA256(prev_hash || seq || ts_ms || node_id || peer || kind || payload)
        let peer_opt: Option<NodeId> = match &event {
            AuditEvent::AppSend { to, .. } => Some(*to),
            AuditEvent::AppRecv { from, .. } => Some(*from),
            _ => None,
        };

        let hash = compute_hash(
            prev_hash,
            self.seq,
            ts_ms,
            self.node_id,
            peer_opt,
            &kind,
            &payload,
        );

        let sig: Signature = self.signing_key.sign(&hash);

        let entry = LogEntry {
            seq: self.seq,
            ts_ms,
            node_id: self.node_id,
            peer: peer_opt,
            kind,
            payload,
            prev_hash,
            hash,
            sig: sig.to_bytes().to_vec(),
        };

        // JSON line
        let line = entry.to_json_line()?;
        writeln!(self.file, "{line}")?;
        self.file.flush()?;

        // update state
        self.seq += 1;
        self.last_hash = entry.hash;

        Ok(entry)
    }

    /// Vérifie un fichier de log complet.
    pub fn verify_log(path: impl AsRef<Path>, vk: &VerifyingKey) -> std::io::Result<bool> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut expected_prev = [0u8; 32];

        for (i, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let entry = LogEntry::from_json_line(&line)?;

            // 1) chain prev_hash cohérente
            if entry.prev_hash != expected_prev {
                eprintln!("[verify] bad prev_hash at line {}", i + 1);
                return Ok(false);
            }

            // 2) recompute hash
            let recomputed = compute_hash(
                entry.prev_hash,
                entry.seq,
                entry.ts_ms,
                entry.node_id,
                entry.peer,
                &entry.kind,
                &entry.payload,
            );

            if recomputed != entry.hash {
                eprintln!("[verify] bad hash at line {}", i + 1);
                return Ok(false);
            }

            // 3) signature
            if entry.sig.len() != 64 {
                eprintln!("[verify] bad sig len at line {}", i + 1);
                return Ok(false);
            }
            let mut sig_arr = [0u8; 64];
            sig_arr.copy_from_slice(&entry.sig[..64]);
            let sig = Signature::from_bytes(&sig_arr);

            if vk.verify(&entry.hash, &sig).is_err() {
                eprintln!("[verify] bad signature at line {}", i + 1);
                return Ok(false);
            }

            expected_prev = entry.hash;
        }

        Ok(true)
    }
}

// ----------------- helpers -----------------

fn compute_hash(
    prev_hash: [u8; 32],
    seq: u64,
    ts_ms: u64,
    node_id: NodeId,
    peer: Option<NodeId>,
    kind: &str,
    payload: &str,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(prev_hash);
    h.update(seq.to_be_bytes());
    h.update(ts_ms.to_be_bytes());
    h.update(node_id.to_be_bytes());

    match peer {
        Some(p) => h.update(p.to_be_bytes()),
        None => h.update(0u32.to_be_bytes()),
    }

    h.update(kind.as_bytes());
    h.update(payload.as_bytes());

    let out = h.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&out[..32]);
    hash
}

fn restore_state_from_file(path: &Path) -> Option<(u64, [u8; 32])> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut last: Option<LogEntry> = None;
    for line in reader.lines().flatten() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(e) = LogEntry::from_json_line(&line) {
            last = Some(e);
        }
    }

    let last = last?;
    Some((last.seq + 1, last.hash))
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
