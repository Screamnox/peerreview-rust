use peerreview_rust::journal::Logger;
use peerreview_rust::network::{Bootstrap, NetworkLayer};
use peerreview_rust::types::config::{Config, PeersConfig};
use peerreview_rust::types::messages::PeerReviewMsg;
use peerreview_rust::types::node::{Node};

use ed25519_dalek::Keypair;
use rand::rngs::OsRng;
use std::sync::{Arc, Mutex};
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {    
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <node_config.toml>", args[0]);
        std::process::exit(1);
    }

    let config_file = &args[1];
    let config = Config::from_file(config_file)?;
    let peers_config = PeersConfig::from_file(&config.network.peers_file)?;

    println!("[Main] Noeud {} démarrage", config.node.id);

    // Keypair
    let mut rng = OsRng;
    let keypair = Keypair::generate(&mut rng);

    // Logger
    let logger = Logger::new(
        &config.node.log_file,
        config.node.log_max_lines,
        config.node.log_min_line_size,
    )?;

    // Node
    let node = Arc::new(Mutex::new(Node::new(
        config.node.id,
        keypair,
        logger,
        config.witnesses.list.clone(),
    )));

    // Bootstrap
    let connections = Bootstrap::connect_to_peers(
        config.node.id,
        &config.network.listen_address, 
        &peers_config,
        config.network.connection_timeout_secs
    )?;

    // Add peers to the node
    let peer_infos = Bootstrap::get_peer_info(config.node.id, &peers_config)?;
    {
        let mut node = node.lock().unwrap();

        for (peer_id, public_key, witnesses) in peer_infos {
            node.add_peer(peer_id, public_key, witnesses);
        }
    }

    // Network layer
    let network = NetworkLayer::new(connections)?;

    // Test send message
    let test_msg = PeerReviewMsg::Send {
        seq: 1,
        prev_hash: [0u8; 32],
        sig: [0u8; 64],
        dest: config.node.id,
        msg: format!("Hello from node {}", config.node.id),
    };

    let peer_ids = network.get_peer_ids();
    for peer_id in &peer_ids {
        println!("[Node {}] Envoi test → {}", config.node.id, peer_id);
        network.send(*peer_id, &test_msg)?;
    }

    let mut count = 0;
    while count < peer_ids.len() {
        let (from_peer, msg) = network.recv()?;
        
        println!("[Node {}] Received from {} -> {:?}",
            config.node.id, from_peer, msg);
        
        count += 1;
    }

    // Wait 15s before shutdown
    std::thread::sleep(std::time::Duration::from_secs(15));

    network.shutdown()?;

    Ok(())
}
