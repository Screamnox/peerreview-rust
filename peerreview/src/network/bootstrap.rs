use std::io;
use std::sync::{Arc, Mutex};

use crate::network::tcp::NetworkLayer;
use crate::types::config::PeersConfig;
use crate::types::node::{Node, PeerInfo, PeerStatus, PublicKey};

pub fn bootstrap_from_config(
    cfg_path: &str,
    net: &NetworkLayer,
    mut node: Node,
) -> io::Result<Node> {
    let cfg = PeersConfig::load(cfg_path)?;

    for peer in cfg.other_peers() {
        // Stub : clé publique vide (32 octets)
        let public_key: PublicKey = [0u8; 32];

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
