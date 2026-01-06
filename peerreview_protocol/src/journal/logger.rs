use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
use ed25519_dalek::Signer;
use sha2::{Digest, Sha256};

use crate::journal::entry::LogEntry;
use crate::types::NodeId;

pub struct Logger {
    node_id: NodeId,
    file: File,
    verifying_key: VerifyingKey,
    signing_key: SigningKey,
    seq: u64,
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

        let verifying_key = signing_key.verifying_key();

        Ok(Self {
            node_id,
            file,
            verifying_key,
            signing_key,
            seq: 0,
        })
    }

    fn append_json_line(&mut self, entry: &LogEntry) -> std::io::Result<()> {
        let line = entry.to_json_line()?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
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

        let mut h = Sha256::new();
        h.update(kind.as_bytes());
        if let Some(p) = peer {
            h.update(p.to_be_bytes());
        }
        h.update(msg_id.as_bytes());
        h.update(ts_ms.to_be_bytes());
        h.update(hash32);

        let digest = h.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest[..]);

        let sig: Signature = self.signing_key.sign(&hash);

        let entry = LogEntry::new(
            self.seq,
            peer.unwrap_or(0),
            kind,
            hash,
            sig.to_bytes(),
            payload,
        );

        self.append_json_line(&entry)
    }

    pub fn log_pr_in(&mut self, line: &str) -> std::io::Result<()> {
        self.seq += 1;

        let mut h = Sha256::new();
        h.update(b"PR_IN");
        h.update(line.as_bytes());
        let digest = h.finalize();

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest[..]);

        let sig: Signature = self.signing_key.sign(&hash);

        let entry = LogEntry::new(self.seq, 0, "PR_IN", hash, sig.to_bytes(), line.to_string());
        self.append_json_line(&entry)
    }

    pub fn log_pr_out(&mut self, line: &str) -> std::io::Result<()> {
        self.seq += 1;

        let mut h = Sha256::new();
        h.update(b"PR_OUT");
        h.update(line.as_bytes());
        let digest = h.finalize();

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest[..]);

        let sig: Signature = self.signing_key.sign(&hash);

        let entry = LogEntry::new(self.seq, 0, "PR_OUT", hash, sig.to_bytes(), line.to_string());
        self.append_json_line(&entry)
    }

    pub fn verify_log_file(
        path: impl AsRef<Path>,
        verifying_key: &VerifyingKey,
    ) -> std::io::Result<()> {
        let f = File::open(path)?;
        let r = BufReader::new(f);

        for line in r.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let entry = LogEntry::from_json_line(&line)?;

            let sig64 = entry.sig_bytes_64()?;
            let sig = Signature::from_bytes(&sig64);

            verifying_key
                .verify(entry.hash_bytes(), &sig)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        }
        Ok(())
    }
}
