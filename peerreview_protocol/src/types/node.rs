use ed25519_dalek::{SigningKey, VerifyingKey};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub type NodeId = u32;

#[derive(Clone)]
pub struct PrKeys {
    pub signing: SigningKey,
    pub verifying: VerifyingKey,
}

/// Stable demo keys derived from node name.
/// OK for POC, not for production security.
pub fn derive_keys_from_name(name: &str) -> PrKeys {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    let seed = hasher.finish();

    let mut sk = [0u8; 32];
    // 8 bytes of entropy is enough for a deterministic demo; keep the rest 0.
    sk[..8].copy_from_slice(&seed.to_be_bytes());
    let signing = SigningKey::from_bytes(&sk);
    let verifying = signing.verifying_key();
    PrKeys { signing, verifying }
}

/// Convention demo: "node7" -> 7
pub fn node_id_from_name(name: &str) -> NodeId {
    name.trim()
        .strip_prefix("node")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

pub fn signing_key_from_name(name: &str) -> SigningKey {
    derive_keys_from_name(name).signing
}

pub fn verifying_key_from_name(name: &str) -> VerifyingKey {
    derive_keys_from_name(name).verifying
}
