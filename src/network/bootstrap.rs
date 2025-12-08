use ed25519_dalek::PublicKey;
use std::net::TcpStream;
use std::time::Duration;

use super::layer::NetworkLayer;
use crate::types::config::PeersConfig;
use crate::types::node::NodeId;

pub struct Bootstrap;

impl Bootstrap {
    /// Connecte le nœud à tous les pairs depuis peers.toml
    ///
    /// Stratégie: ID > node_id initie la connexion (évite les races)
    pub fn connect_to_peers(
        node_id: NodeId,
        network: &mut NetworkLayer,
        peers_config: &PeersConfig,
        timeout_secs: u64,
    ) -> Result<Vec<(NodeId, PublicKey, Vec<NodeId>)>, String> {
        unimplemented!();
    }

    /// Établit une connexion avec handshake
    fn connect_with_handshake(
        our_id: NodeId,
        peer_id: NodeId,
        address: &str,
        timeout: Duration,
    ) -> std::io::Result<TcpStream> {
        unimplemented!();
    }

    /// Accepte une connexion avec handshake
    fn accept_connection_with_handshake(
        our_id: NodeId,
        expected_peer_id: NodeId,
        listener: &std::net::TcpListener,
        timeout: Duration,
    ) -> std::io::Result<TcpStream> {
        unimplemented!();
    }

    /// Décode une clé publique base64
    fn decode_public_key(base64_str: &str) -> Result<PublicKey, String> {
        unimplemented!();
    }
}
