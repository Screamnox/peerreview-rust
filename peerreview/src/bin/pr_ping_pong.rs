use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ed25519_dalek::Keypair;
use rand::rngs::OsRng;

use peerreview::network::tcp::NetworkLayer;
use peerreview::types::messages::PeerReviewMsg;
use peerreview::types::node::Node;

fn main() -> anyhow::Result<()> {
    // Exemple : on passe l'addr PR en argument
    // ex: cargo run --bin pr_ping_pong -- 127.0.0.1:9001
    let args: Vec<String> = std::env::args().collect();
    let bind = args.get(1).expect("usage: pr_ping_pong <bind-addr>");

    let net = NetworkLayer::new(bind)?;
    let mut csprng = OsRng;
    let keypair = Keypair::generate(&mut csprng);
    let mut node = Node::new(1, keypair); // id=1 pour l'exemple

    // écoute PR
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let recv_clone = received.clone();

    net.listen(move |addr: SocketAddr, msg: PeerReviewMsg| {
        println!("[PR] received from {}: {:?}", addr, msg);
        let mut g = recv_clone.lock().unwrap();
        g.push(format!("{:?}", msg));
    })?;

    // Boucle triviale : s'auto-ping (pour tester)
    thread::spawn({
        let net = net.clone();
        move || {
            thread::sleep(Duration::from_millis(500));
            let mut s = net.connect(bind).unwrap();
            let msg = PeerReviewMsg::Ping {
                from: 1,
                payload: "hello peerreview".to_string(),
            };
            net.send_message(&mut s, &msg).unwrap();
        }
    });

    loop {
        thread::sleep(Duration::from_secs(2));
        let g = received.lock().unwrap();
        if !g.is_empty() {
            println!("[PR] log local: {:?}", *g);
            break;
        }
    }

    Ok(())
}
