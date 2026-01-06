use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub seq: u64,
    pub peer: u32,
    pub kind: String,
    pub hash: [u8; 32],
    pub sig: Vec<u8>,      // 64 bytes
    pub payload: String,
}

impl LogEntry {
    pub fn new(
        seq: u64,
        peer: u32,
        kind: impl Into<String>,
        hash: [u8; 32],
        sig: [u8; 64],
        payload: impl Into<String>,
    ) -> Self {
        Self {
            seq,
            peer,
            kind: kind.into(),
            hash,
            sig: sig.to_vec(),
            payload: payload.into(),
        }
    }

    pub fn to_json_line(&self) -> std::io::Result<String> {
        serde_json::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn from_json_line(line: &str) -> std::io::Result<Self> {
        serde_json::from_str(line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn hash_bytes(&self) -> &[u8; 32] {
        &self.hash
    }

    pub fn sig_bytes_64(&self) -> std::io::Result<[u8; 64]> {
        if self.sig.len() != 64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("sig len {} != 64", self.sig.len()),
            ));
        }
        let mut out = [0u8; 64];
        out.copy_from_slice(&self.sig);
        Ok(out)
    }
}
