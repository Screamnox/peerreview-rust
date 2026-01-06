use std::{fs, io, path::Path};

use base64::Engine;
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use super::node::NodeId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    pub peers: Vec<PeerConfigEntry>,
}

/// Entrée de config PR (TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfigEntry {
    pub id: NodeId,
    pub address: String,

    /// Optionnel : base64(VerifyingKey bytes)
    pub public_key_b64: Option<String>,

    /// IDs des witnesses pour ce nœud "observé"
    #[serde(default)]
    pub witnesses: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub id: NodeId,
    pub address: String,
    pub public_key_b64: Option<String>,
}

impl PeerConfig {
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let s = fs::read_to_string(path)?;
        toml::from_str(&s).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
    }
}

impl PeerConfigEntry {
    /// Décodage optionnel de la clé publique (si présente)
    pub fn verifying_key(&self) -> io::Result<Option<VerifyingKey>> {
        let Some(b64) = &self.public_key_b64 else {
            return Ok(None);
        };

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;

        let vk =
            VerifyingKey::from_bytes(bytes.as_slice().try_into().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "bad verifying key len")
            })?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;

        Ok(Some(vk))
    }
}
