use std::net::SocketAddr;
use base64::Engine;

use ed25519_dalek::VerifyingKey;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PeersConfig {
    pub peers: Vec<PeerConfigEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PeerConfigEntry {
    pub id: u32,
    pub address: String,
    pub public_key_b64: Option<String>,
    pub witnesses: Option<Vec<u32>>,
}

impl PeerConfigEntry {
    pub fn socket_addr(&self) -> std::io::Result<SocketAddr> {
        self.address.parse().map_err(|e: std::net::AddrParseError| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
        })
    }

    /// Optionnel : construit un verifying key si public_key_b64 est fourni.
    pub fn verifying_key(&self) -> std::io::Result<Option<VerifyingKey>> {
        let Some(b64) = &self.public_key_b64 else { return Ok(None); };

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;

        if bytes.len() != 32 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("public_key_b64 must decode to 32 bytes, got {}", bytes.len()),
            ));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes[..32]);

        Ok(Some(VerifyingKey::from_bytes(&arr).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
        })?))
    }
}
