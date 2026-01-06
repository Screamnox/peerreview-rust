use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::journal::logger::Logger;
use crate::network::layer::NetworkLayer;
use crate::types::{AppEvent, NodeId, PeerReviewMsg};

pub struct PeerReviewRuntime {
    app_tx: mpsc::Sender<AppEvent>,
    _handles: Vec<JoinHandle<()>>,
}

impl PeerReviewRuntime {
    pub async fn start(
        my_id: NodeId,
        pr_listen: SocketAddr,
        peers: Vec<(NodeId, SocketAddr)>,
        logger: Arc<Mutex<Logger>>,
    ) -> std::io::Result<Self> {
        let net = NetworkLayer::new();

        // Canal APP -> PR
        let (app_tx, mut app_rx) = mpsc::channel::<AppEvent>(1024);

        // Task 1: listener PR (réseau -> PR)
        let net_in = net.clone();
        let logger_in = logger.clone();
        let handle_listener = tokio::spawn(async move {
            let cb: Arc<dyn Fn(SocketAddr, PeerReviewMsg) + Send + Sync> = Arc::new(move |from, msg| {
                if let Ok(mut lg) = logger_in.lock() {
                    let _ = lg.log_pr_in(&format!("from_addr={} msg={:?}", from, msg));
                }
            });

            let _ = net_in.listen(pr_listen, cb).await;
        });

        // Task 2: worker APP -> PR (journalisation + ping démo PR)
        let net_out = net.clone();
        let logger_app = logger.clone();
        let handle_app = tokio::spawn(async move {
            let mut counter: u64 = 0;

            while let Some(ev) = app_rx.recv().await {
                let (kind_str, peer, msg_id, hash32, ts_ms) = match ev {
                    AppEvent::Send { to, msg_id, hash32, ts_ms } => ("SEND", Some(to), msg_id, hash32, ts_ms),
                    AppEvent::Recv { from, msg_id, hash32, ts_ms } => ("RECV", Some(from), msg_id, hash32, ts_ms),
                    AppEvent::Deliver { msg_id, hash32, ts_ms } => ("DELIVER", None, msg_id, hash32, ts_ms),
                };

                if let Ok(mut lg) = logger_app.lock() {
                    let _ = lg.log_app(kind_str, peer, msg_id.clone(), hash32, ts_ms);
                }

                // ping PR très léger (preuve que le thread PR “agit”)
                counter += 1;
                if counter % 50 == 0 {
                    for (pid, paddr) in &peers {
                        let ping = PeerReviewMsg::Ping { from: my_id, counter };
                        let _ = net_out.send(*paddr, ping).await;

                        if let Ok(mut lg) = logger_app.lock() {
                            let _ = lg.log_pr_out(&format!("PING to={} addr={} counter={}", pid, paddr, counter));
                        }
                    }
                }
            }
        });

        // Task 3: scheduler placeholder (audit complet branché plus tard)
        let logger_sched = logger.clone();
        let handle_sched = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                if let Ok(mut lg) = logger_sched.lock() {
                    let _ = lg.log_pr_out("AUDIT_TICK");
                }
            }
        });

        Ok(Self {
            app_tx,
            _handles: vec![handle_listener, handle_app, handle_sched],
        })
    }

    pub fn app_sender(&self) -> mpsc::Sender<AppEvent> {
        self.app_tx.clone()
    }
}
