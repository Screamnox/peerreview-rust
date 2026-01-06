use std::{collections::HashMap, io};

use crate::types::{NodeId, PeerConfig};
use crate::types::config::PeerInfo;

pub fn build_peer_map(cfg: &PeerConfig) -> io::Result<HashMap<NodeId, PeerInfo>> {
    let mut m = HashMap::new();
    for p in cfg.peers.iter() {
        m.insert(
            p.id,
            PeerInfo {
                id: p.id,
                address: p.address.clone(),
                public_key_b64: p.public_key_b64.clone(),
            },
        );
    }
    Ok(m)
}
