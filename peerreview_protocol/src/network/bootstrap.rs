use crate::types::config::{PeerConfig, PeerInfo};
use crate::types::NodeId;

pub fn build_peer_config(my_id: NodeId, peers: &[PeerInfo]) -> PeerConfig {
    // On crée un PeerConfig qui contient tous les autres peers (pas soi-même)
    let mut out = Vec::new();
    for p in peers {
        if p.id == my_id {
            continue;
        }
        out.push(PeerInfo {
            id: p.id,
            addr: p.addr,
            public_key_b64: p.public_key_b64.clone(),
        });
    }
    PeerConfig { peers: out }
}
