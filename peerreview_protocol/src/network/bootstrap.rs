use std::collections::HashMap;

use crate::types::{PeerConfigEntry, PeerInfo, PeerStatus};

pub fn build_peer_map(cfg: &[PeerConfigEntry]) -> std::io::Result<HashMap<u32, PeerInfo>> {
    let mut out = HashMap::new();

    for p in cfg {
        let addr = p.socket_addr()?;
        let witnesses = p.witnesses.clone().unwrap_or_default();

        out.insert(
            p.id,
            PeerInfo {
                id: p.id,
                address: addr,
                public_key_b64: p.public_key_b64.clone(),
                status: PeerStatus::Trusted,
                witnesses,
            },
        );
    }

    Ok(out)
}
