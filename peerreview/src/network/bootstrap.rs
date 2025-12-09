use std::sync::Arc;
use std::sync::Mutex;

use ed25519_dalek::PublicKey;

use crate::network::tcp::NetworkLayer;
use crate::types::config::PeersConfig;
use crate::types::node::{Node, PeerInfo, PeerStatus};

pub fn bootstrap_from_config(
    cfg_path: &str,
    net: &NetworkLayer,
    mut node: Node,
) -> anyhow::Result<Node> {
    let cfg = PeersConfig::load(cfg_path)?;

    for peer in cfg.other_peers() {
        // TODO: chargement réel de la clé publique depuis peer.public_key_file
        let dummy_bytes = [0u8; 32];
        let public_key = PublicKey::from_bytes(&dummy_bytes)?;

        println!("[PR] connecting to peer {} at {}", peer.id, peer.addr);
        let stream = net.connect(&peer.addr)?;
        stream.set_nodelay(true)?;

        let info = PeerInfo {
            id: peer.id,
            public_key,
            socket: Arc::new(Mutex::new(stream)),
            status: PeerStatus::Trusted,
        };
        node.add_peer(info);
    }

    Ok(node)
}
