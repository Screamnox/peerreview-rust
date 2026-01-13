use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub seq: u64,
    pub peer: u32,
    pub kind: String,

    /// SHA256 digest bytes
    pub hash: Vec<u8>, // must be len=32

    /// Chaining hash bytes (previous entry hash)
    pub prev_hash: Vec<u8>, // must be len=32

    /// Ed25519 signature bytes over `hash` (NOT over JSON)
    pub sig: Vec<u8>, // must be len=64

    /// Human-readable payload
    pub payload: String,
}

impl LogEntry {
    pub fn new(
        seq: u64,
        peer: u32,
        kind: &str,
        hash32: [u8; 32],
        prev_hash32: [u8; 32],
        sig64: [u8; 64],
        payload: String,
    ) -> Self {
        Self {
            seq,
            peer,
            kind: kind.to_string(),
            hash: hash32.to_vec(),
            prev_hash: prev_hash32.to_vec(),
            sig: sig64.to_vec(),
            payload,
        }
    }

    pub fn to_json_line(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn from_json_line(s: &str) -> io::Result<Self> {
        serde_json::from_str(s)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn hash_bytes_32(&self) -> io::Result<[u8; 32]> {
        if self.hash.len() != 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "hash must be 32 bytes",
            ));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&self.hash);
        Ok(out)
    }

    pub fn prev_hash_bytes_32(&self) -> io::Result<[u8; 32]> {
        if self.prev_hash.len() != 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "prev_hash must be 32 bytes",
            ));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&self.prev_hash);
        Ok(out)
    }

    pub fn sig_bytes_64(&self) -> io::Result<[u8; 64]> {
        if self.sig.len() != 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "sig must be 64 bytes",
            ));
        }
        let mut out = [0u8; 64];
        out.copy_from_slice(&self.sig);
        Ok(out)
    }
}
