use ed25519_dalek::{SigningKey, VerifyingKey};

pub type NodeId = u32;

/// Clés PR du nœud (Signing + Verifying)
#[derive(Clone)]
pub struct PrKeys {
    pub signing: SigningKey,
    pub verifying: VerifyingKey,
}

impl PrKeys {
    /// Génération simple pour démo/tests (pas d’import rand direct ici).
    /// On s’appuie sur le fait que SigningKey::from_bytes existe :
    /// on dérive 32 bytes depuis un seed déterministe.
    pub fn generate_for_demo(seed: u64) -> Self {
        // seed -> 32 bytes deterministes
        let mut sk = [0u8; 32];
        sk[..8].copy_from_slice(&seed.to_le_bytes());
        for i in 8..32 {
            sk[i] = (sk[i - 8]).wrapping_add(i as u8);
        }

        let signing = SigningKey::from_bytes(&sk);
        let verifying = signing.verifying_key();
        Self { signing, verifying }
    }
}
