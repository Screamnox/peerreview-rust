use std::collections::HashMap;
use std::net::SocketAddr;

use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use signature::{Signer, Verifier};

pub type NodeId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerStatus {
    Trusted,
    Suspected,
    Exposed,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub id: NodeId,
    pub address: SocketAddr,
    /// Optionnel : b64 du verifying key (32 bytes). Utile quand le groupe PR branchera l’auth.
    pub public_key_b64: Option<String>,
    pub status: PeerStatus,
    pub witnesses: Vec<NodeId>,
}

pub struct Node {
    pub id: NodeId,
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    pub peers: HashMap<NodeId, PeerInfo>,
}

impl Node {
    /// Création déterministe : super utile pour tests reproductibles (pas besoin de RNG).
    pub fn new(id: NodeId) -> Self {
        let sk_bytes = derive_32_bytes_from_u32(id);
        let signing_key = SigningKey::from_bytes(&sk_bytes);
        let verifying_key = signing_key.verifying_key();

        Self {
            id,
            signing_key,
            verifying_key,
            peers: HashMap::new(),
        }
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.verifying_key
    }

    pub fn sign(&self, data: &[u8]) -> [u8; 64] {
        self.signing_key.sign(data).to_bytes()
    }

    pub fn verify(vk: &VerifyingKey, data: &[u8], sig: &[u8; 64]) -> bool {
        let signature = Signature::from_bytes(sig);
        vk.verify(data, &signature).is_ok()
    }
}

fn derive_32_bytes_from_u32(id: u32) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(id.to_be_bytes());
    let out = h.finalize();
    let mut sk = [0u8; 32];
    sk.copy_from_slice(&out[..32]);
    sk
}
