use crate::types::config::{PeerConfig, PeerInfo};

pub fn peers_from_cluster(nodes: &[PeerInfo]) -> PeerConfig {
    PeerConfig {
        peers: nodes
            .iter()
            .map(|p| crate::types::config::PeerConfigEntry {
                id: p.id,
                addr: p.addr.clone(),
            })
            .collect(),
    }
}
