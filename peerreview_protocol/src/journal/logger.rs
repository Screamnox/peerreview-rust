use crate::journal::entry::{now_ms, LogEntry};
use crate::types::NodeId;
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey, SigningKey};
use sha2::{Digest, Sha256};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct Logger {
    node_id: NodeId,
    pub file: File,
    seq: u64,
    prev_hash32: [u8; 32],
    signing_key: SigningKey,
    path: PathBuf,
}

impl Logger {
    pub fn open(
        node_id: NodeId,
        log_dir: impl AsRef<Path>,
        signing_key: SigningKey,
    ) -> io::Result<Self> {
        create_dir_all(&log_dir)?;
        let path = log_dir.as_ref().join(format!("node{}_app.log", node_id));

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        let (seq, prev_hash32) = Self::recover_tail(&path).unwrap_or((0, [0u8; 32]));

        Ok(Self {
            node_id,
            file,
            seq,
            prev_hash32,
            signing_key,
            path,
        })
    }

    fn compute_hash32(prev: [u8; 32], seq: u64, peer: u32, kind: &str, payload: &str) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(&prev);
        h.update(seq.to_be_bytes());
        h.update(peer.to_be_bytes());
        h.update(kind.as_bytes());
        h.update(payload.as_bytes());
        let out = h.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&out);
        arr
    }

    fn recover_tail(path: &Path) -> io::Result<(u64, [u8; 32])> {
        let f = File::open(path)?;
        let mut last_seq = 0u64;
        let mut last_hash = [0u8; 32];

        for line in BufReader::new(f).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let e = LogEntry::from_json_line(&line)?;
            last_seq = e.seq;
            last_hash = e.hash_bytes_32()?;
        }
        Ok((last_seq, last_hash))
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn prev_hash32(&self) -> [u8; 32] {
        self.prev_hash32
    }

    pub fn read_log_window(&self, from_seq: u64, to_seq: u64) -> io::Result<Vec<String>> {
        let f = File::open(&self.path)?;
        let r = BufReader::new(f);

        let mut out = Vec::new();
        for line in r.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let e = LogEntry::from_json_line(&line)?;
            if e.seq >= from_seq && e.seq <= to_seq {
                out.push(line);
            }
        }
        Ok(out)
    }

    /// Generic event logger with arbitrary payload (parseable by audit tools).
    pub fn log_event(&mut self, kind: &str, peer: Option<u32>, payload: String) -> io::Result<()> {
        let peer = peer.unwrap_or(0);

        self.seq += 1;
        let entry_hash32 = Self::compute_hash32(self.prev_hash32, self.seq, peer, kind, &payload);
        let sig: Signature = self.signing_key.sign(&entry_hash32);

        let entry = LogEntry::new(
            self.seq,
            peer,
            kind,
            entry_hash32,
            self.prev_hash32,
            sig.to_bytes(),
            payload,
        );

        writeln!(self.file, "{}", entry.to_json_line()?)?;
        self.file.flush()?;
        self.prev_hash32 = entry.hash_bytes_32()?;
        Ok(())
    }

    /// Backward compat helper (kept).
    pub fn log_app(
        &mut self,
        kind: &str,
        peer: Option<u32>,
        msg_id: String,
        hash32: [u8; 32],
        ts_ms: u64,
    ) -> io::Result<()> {
        let payload = format!("ts={ts_ms} msg_id={msg_id} hash32={}", hex::encode(hash32));
        self.log_event(kind, peer, payload)
    }

    pub fn log_pr_in(&mut self, line: &str) -> io::Result<()> {
        let payload = format!("ts={} {line}", now_ms());
        self.log_event("PR_IN", None, payload)
    }

    pub fn log_pr_out(&mut self, line: &str) -> io::Result<()> {
        let payload = format!("ts={} {line}", now_ms());
        self.log_event("PR_OUT", None, payload)
    }

    pub fn verify_log_file(path: impl AsRef<Path>, vk: &VerifyingKey, strict: bool) -> io::Result<()> {
        let f = File::open(path)?;
        let mut prev: [u8; 32] = [0u8; 32];
        let mut last_seq: u64 = 0;

        for (i, line) in BufReader::new(f).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let entry = LogEntry::from_json_line(&line).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("bad json at line {}: {e}", i + 1),
                )
            })?;

            let prev_arr = entry.prev_hash_bytes_32()?;
            if strict && prev_arr != prev {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("hash chain broken at line {}: prev_hash mismatch", i + 1),
                ));
            }

            let hash_arr = entry.hash_bytes_32()?;
            let sig64 = entry.sig_bytes_64()?;
            let sig = Signature::from_bytes(&sig64);
            vk.verify(&hash_arr, &sig).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("signature invalid at line {}: {e}", i + 1),
                )
            })?;

            if strict && entry.seq <= last_seq {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("seq not increasing at line {}", i + 1),
                ));
            }

            last_seq = entry.seq;
            prev = hash_arr;
        }

        Ok(())
    }

    pub fn log_path_for(node_id: NodeId, log_dir: impl AsRef<Path>) -> PathBuf {
        log_dir.as_ref().join(format!("node{}_app.log", node_id))
    }
}
