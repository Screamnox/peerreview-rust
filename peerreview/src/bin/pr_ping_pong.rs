use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// On utilise la lib du crate `peerreview` (grâce à src/lib.rs)
use peerreview::{NetworkLayer, PeerReviewMsg};

fn main() -> std::io::Result<()> {
    // Usage : cargo run -p peerreview --bin pr_ping_pong -- 127.0.0.1:9001
    let args: Vec<String> = std::env::args().collect();
    let bind = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:9001".to_string());

    println!("[PR ping_pong] binding on {}", bind);

    let net = NetworkLayer::new(&bind)?;

    // Petit buffer pour vérifier qu'on a bien reçu un message
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recv_clone = received.clone();

    // Écoute PR : chaque message reçu est loggé
    net.listen(move |addr: SocketAddr, msg: PeerReviewMsg| {
        println!("[PR listener] from {}: {:?}", addr, msg);
        let mut g = recv_clone.lock().unwrap();
        g.push(format!("{:?}", msg));
    })?;

    // On s'auto-connecte après un petit délai pour envoyer un Ping
    {
        let net_clone = net.clone();
        let bind_clone = bind.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            println!("[PR ping_pong] connecting to {}", bind_clone);
            let mut stream = match net_clone.connect(&bind_clone) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[PR ping_pong] connect error: {}", e);
                    return;
                }
            };

            let msg = PeerReviewMsg::Ping {
                from: 1,
                payload: "hello peerreview".to_string(),
            };

            if let Err(e) = net_clone.send_message(&mut stream, &msg) {
                eprintln!("[PR ping_pong] send error: {}", e);
            } else {
                println!("[PR ping_pong] Ping envoyé");
            }
        });
    }

    // Boucle simple : on attend de voir au moins un message reçu
    loop {
        thread::sleep(Duration::from_secs(1));
        let g = received.lock().unwrap();
        if !g.is_empty() {
            println!("[PR ping_pong] messages reçus: {:?}", *g);
            break;
        } else {
            println!("[PR ping_pong] en attente de messages...");
        }
    }

    Ok(())
}
