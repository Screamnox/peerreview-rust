use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

use super::entry::Hash;
use super::errors::Result;

pub struct Authenticator {
    pub node_id: u32,
    pub seq: u64,
    pub hash: Hash,
    pub sig: Signature,
}

pub enum AuthSource<'a> {
    Signature(Signature),
    SigningKey(&'a SigningKey),
}

impl Authenticator {
    /// Create a new `Authenticator`.
    ///
    /// This constructs an authenticator for `node_id` that signs (or accepts)
    /// the provided `seq` and `hash`. The signature is produced over the
    /// big-endian sequence number followed by the hash.
    ///
    /// # Parameters
    /// - `node_id`: id of the node creating the authenticator.
    /// - `seq`: sequence number included in the signed payload.
    /// - `hash`: 32-byte hash included in the signed payload.
    /// - `source`: either an existing signature (`AuthSource::Signature`) or a
    ///   signing key (`AuthSource::SigningKey`) used to create a signature.
    ///
    /// # Returns
    /// A new `Authenticator` with the provided fields and a signature over
    /// (seq || hash).
    pub fn new(node_id: u32, seq: u64, hash: Hash, source: AuthSource) -> Self {
        let sig = match source {
            AuthSource::Signature(sig) => sig,
            AuthSource::SigningKey(key) => {
                let mut payload = Vec::with_capacity(size_of::<u64>() + size_of::<Hash>());
                payload.extend_from_slice(&seq.to_be_bytes());
                payload.extend_from_slice(&hash);
                key.sign(&payload)
            }
        };

        Self {
            node_id,
            seq,
            hash,
            sig,
        }
    }

    /// Verifies that an authenticator signature is valid
    ///
    /// # Arguments
    /// * `seq` - Sequence number
    /// * `hash` - Hash value
    /// * `sig` - Authenticator signature
    /// * `verifying_key` - Public key of the node that created this authenticator
    ///
    /// # Returns
    /// `Ok(true)` if signature is valid, `Ok(false)` if invalid, or error if signature is malformed
    pub fn verify(
        seq: u64,
        hash: &[u8; 32],
        sig: Signature,
        verifying_key: &VerifyingKey,
    ) -> Result<bool> {
        let mut payload = Vec::with_capacity(size_of::<u64>() + size_of::<[u8; 32]>());
        payload.extend_from_slice(&seq.to_be_bytes());
        payload.extend_from_slice(hash);

        match verifying_key.verify(&payload, &sig) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}
