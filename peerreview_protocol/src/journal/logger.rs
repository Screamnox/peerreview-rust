use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey, SigningKey};
use sha2::{Digest, Sha256};

use crate::journal::entry::LogEntry;
use crate::types::NodeId;

pub struct Logger {
    node_id: NodeId,
    file: File,
    signing_key: SigningKey,
    seq: u64,

    /// hash(chain) of previous entry
    prev_hash32: [u8; 32],
}

impl Logger {
    pub fn open(
        node_id: NodeId,
        log_path: impl AsRef<Path>,
        signing_key: SigningKey,
    ) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(log_path)?;

        Ok(Self {
            node_id,
            file,
            signing_key,
            seq: 0,
            prev_hash32: [0u8; 32],
        })
    }

    fn append_json_line(&mut self, entry: &LogEntry) -> std::io::Result<()> {
        let line = entry.to_json_line()?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
    }

    fn sign_hash(&self, hash32: &[u8; 32]) -> [u8; 64] {
        let sig: Signature = self.signing_key.sign(hash32);
        sig.to_bytes()
    }

    pub fn log_app(
        &mut self,
        kind: &str,
        peer: Option<NodeId>,
        msg_id: String,
        hash32: [u8; 32],
        ts_ms: u64,
    ) -> std::io::Result<()> {
        self.seq += 1;

        let payload = format!(
            "msg_id={} peer={:?} ts_ms={} hash={}",
            msg_id,
            peer,
            ts_ms,
            hex::encode(hash32)
        );

        // stable digest for this log line (not the same as msg hash32)
        let mut h = Sha256::new();
        h.update(kind.as_bytes());
        if let Some(p) = peer {
            h.update(p.to_be_bytes());
        }
        h.update(msg_id.as_bytes());
        h.update(ts_ms.to_be_bytes());
        h.update(hash32);

        let digest = h.finalize();
        let mut line_hash = [0u8; 32];
        line_hash.copy_from_slice(&digest[..]);

        let sig64 = self.sign_hash(&line_hash);

        let entry = LogEntry::new(
            self.seq,
            peer.unwrap_or(0),
            kind,
            line_hash,
            self.prev_hash32,
            sig64,
            payload,
        );

        self.append_json_line(&entry)?;
        self.prev_hash32 = line_hash;
        Ok(())
    }

    pub fn log_pr_in(&mut self, line: &str) -> std::io::Result<()> {
        self.seq += 1;

        let mut h = Sha256::new();
        h.update(b"PR_IN");
        h.update(line.as_bytes());
        let digest = h.finalize();

        let mut line_hash = [0u8; 32];
        line_hash.copy_from_slice(&digest[..]);

        let sig64 = self.sign_hash(&line_hash);

        let entry = LogEntry::new(
            self.seq,
            0,
            "PR_IN",
            line_hash,
            self.prev_hash32,
            sig64,
            line.to_string(),
        );

        self.append_json_line(&entry)?;
        self.prev_hash32 = line_hash;
        Ok(())
    }

    pub fn log_pr_out(&mut self, line: &str) -> std::io::Result<()> {
        self.seq += 1;

        let mut h = Sha256::new();
        h.update(b"PR_OUT");
        h.update(line.as_bytes());
        let digest = h.finalize();

        let mut line_hash = [0u8; 32];
        line_hash.copy_from_slice(&digest[..]);

        let sig64 = self.sign_hash(&line_hash);

        let entry = LogEntry::new(
            self.seq,
            0,
            "PR_OUT",
            line_hash,
            self.prev_hash32,
            sig64,
            line.to_string(),
        );

        self.append_json_line(&entry)?;
        self.prev_hash32 = line_hash;
        Ok(())
    }

    /// Verifies:
    /// - JSON parse
    /// - hash len 32, sig len 64
    /// - signature validity (sig over stored hash bytes)
    /// - seq monotonic (+1)
    /// - optional strict chain: prev_hash must match previous entry hash
    pub fn verify_log_file(
        path: impl AsRef<Path>,
        verifying_key: &VerifyingKey,
        strict_chain: bool,
    ) -> std::io::Result<()> {
        let f = File::open(path)?;
        let r = BufReader::new(f);

        let mut expected_seq: u64 = 1;
        let mut prev_hash32: [u8; 32] = [0u8; 32];

        for line in r.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let entry = LogEntry::from_json_line(&line)?;

            // seq monotonic
            if entry.seq != expected_seq {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("bad seq: got {}, expected {}", entry.seq, expected_seq),
                ));
            }
            expected_seq += 1;

            // strict chaining
            if strict_chain {
                let got_prev = entry.prev_hash_bytes_32()?;
                if got_prev != prev_hash32 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "prev_hash chain mismatch",
                    ));
                }
            }

            let hash32 = entry.hash_bytes_32()?;
            let sig64 = entry.sig_bytes_64()?;
            let sig = Signature::from_bytes(&sig64);

            verifying_key
                .verify(&hash32, &sig)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

            prev_hash32 = hash32;
        }

        Ok(())
    }
}
