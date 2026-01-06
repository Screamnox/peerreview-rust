// apps/gossip_node/src/main.rs

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};

use common_proto::{BinaryChunk, Msg, MsgKind};

use peerreview_protocol::{
    journal::Logger as PrLogger, network::layer::NetworkLayer as PrNetwork, types::PeerReviewMsg,
};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::{sleep, Duration},
};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// ----------------------------
// Config structures (YAML)
// ----------------------------
#[derive(Debug, Clone, Deserialize)]
struct NodeCfg {
    id: String,
    listen_addr: String,
    heartbeat_ms: u64,
    gossip_ms: u64,
    anti_entropy_ms: u64,
    http_api: u16,
}

#[derive(Debug, Clone, Deserialize)]
struct ClusterCfg {
    nodes: Vec<PeerCfg>,
    fanout: usize,
    num_trees: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct PeerCfg {
    id: String,
    addr: String,
}

// Peer used internally by gossip_node
#[derive(Debug, Clone)]
struct Peer {
    id: String,
    addr: String,
}

// ----------------------------
// HTTP payloads
// ----------------------------
#[derive(Debug, Clone, Deserialize)]
struct PublishReq {
    payload: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PublishBinaryDemoReq {
    file_id: String,
    total_size: usize,
    chunk_size: usize,
}

#[derive(Debug, Clone, Serialize)]
struct StatsResp {
    node_id: String,
    known_count: usize,
    last_msgs: Vec<String>,
    trees: usize,
    pr_rx_count: usize,
    pr_tx_count: usize,
}

// ----------------------------
// Shared node state
// ----------------------------
#[derive(Clone)]
struct AppState {
    node_id: String,
    known_msgs: Arc<Mutex<HashSet<(u32, String)>>>,
    inbox: Arc<Mutex<VecDeque<String>>>,
    children_by_tree: Arc<Vec<Vec<Peer>>>,
    tx: mpsc::Sender<(Peer, Vec<u8>)>,

    // Internal APP<->PR within node
    pr_to_app_tx: mpsc::Sender<String>,
    app_to_pr_tx: mpsc::Sender<String>,

    pr_rx_count: Arc<Mutex<u64>>,
    pr_tx_count: Arc<Mutex<u64>>,
}

// Keep inbox size bounded
fn push_inbox(inbox: &mut VecDeque<String>, msg: String) {
    inbox.push_front(msg);
    if inbox.len() > 64 {
        let keep = 64usize;
        let drain_count = inbox.len().saturating_sub(keep);
        if drain_count > 0 {
            for _ in 0..drain_count {
                inbox.pop_back();
            }
        }
    }
}

// deterministic permutation orders for multi-tree
fn make_orders(v: Vec<Peer>, num_trees: usize) -> Vec<Vec<Peer>> {
    let mut out = Vec::with_capacity(num_trees);
    for t in 0..num_trees {
        let mut vv = v.clone();
        let len = vv.len().max(1);
        let rot = t % len;
        vv.rotate_left(rot);
        if vv.len() >= 2 {
            let last = vv.len() - 1;
            vv.swap(0, last);
        }
        out.push(vv);
    }
    out
}

fn kary_children(order: &[Peer], me: &str, fanout: usize) -> Vec<Peer> {
    let idx = order.iter().position(|p| p.id == me);
    let Some(i) = idx else { return vec![] };
    let mut out = vec![];
    for k in 0..fanout {
        let child_i = i * fanout + 1 + k;
        if child_i < order.len() {
            out.push(order[child_i].clone());
        }
    }
    out
}

// ----------------------------
// HTTP handlers
// ----------------------------
async fn get_stats(State(st): State<AppState>) -> impl IntoResponse {
    let known_count = st.known_msgs.lock().unwrap().len();
    let last_msgs = st
        .inbox
        .lock()
        .unwrap()
        .iter()
        .take(12)
        .cloned()
        .collect::<Vec<_>>();

    let pr_rx = *st.pr_rx_count.lock().unwrap();
    let pr_tx = *st.pr_tx_count.lock().unwrap();

    Json(StatsResp {
        node_id: st.node_id.clone(),
        known_count,
        last_msgs,
        trees: st.children_by_tree.len(),
        pr_rx_count: pr_rx as usize,
        pr_tx_count: pr_tx as usize,
    })
}

async fn post_publish(
    State(st): State<AppState>,
    Json(req): Json<PublishReq>,
) -> impl IntoResponse {
    let payload = req.payload;
    let num_trees = st.children_by_tree.len();

    for t in 0..num_trees {
        let msg = Msg {
            id: format!("{}-{}-{}", st.node_id, now_ms(), t),
            from: st.node_id.clone(),
            tree_id: t as u8,
            kind: MsgKind::PublishText,
            payload: payload.clone().into_bytes(),
            ts_ms: now_ms(),
        };

        let key = (msg.tree_id as u32, msg.id.clone());
        st.known_msgs.lock().unwrap().insert(key);

        {
            let mut inbox = st.inbox.lock().unwrap();
            push_inbox(
                &mut inbox,
                format!("(t{} from {}) {}", t, msg.from, payload),
            );
        }

        let bytes = bincode::serialize(&msg).unwrap(); // APP = bincode v1
        for peer in &st.children_by_tree[t] {
            let _ = st.tx.send((peer.clone(), bytes.clone())).await;
        }
    }

    (StatusCode::OK, "ok")
}

async fn post_publish_binary_demo(
    State(st): State<AppState>,
    Json(req): Json<PublishBinaryDemoReq>,
) -> impl IntoResponse {
    let file_id = req.file_id;
    let total_size = req.total_size;
    let chunk_size = req.chunk_size.max(1);

    let mut buf = vec![0u8; total_size];
    rand::thread_rng().fill_bytes(&mut buf);

    let total_chunks = (total_size + chunk_size - 1) / chunk_size;
    let num_trees = st.children_by_tree.len();

    for i in 0..total_chunks {
        let start = i * chunk_size;
        let end = (start + chunk_size).min(total_size);
        let data = buf[start..end].to_vec();

        let chunk = BinaryChunk {
            file_id: file_id.clone(),
            index: i as u32,
            total_chunks: total_chunks as u32,
            data,
        };

        let payload = bincode::serialize(&chunk).unwrap(); // v1

        for t in 0..num_trees {
            let msg = Msg {
                id: format!("{}-{}-{}-{}", st.node_id, now_ms(), t, i),
                from: st.node_id.clone(),
                tree_id: t as u8,
                kind: MsgKind::PublishBinaryChunk,
                payload: payload.clone(),
                ts_ms: now_ms(),
            };

            let key = (msg.tree_id as u32, msg.id.clone());
            st.known_msgs.lock().unwrap().insert(key);

            {
                let mut inbox = st.inbox.lock().unwrap();
                push_inbox(
                    &mut inbox,
                    format!(
                        "(local BIN t{} {} chunk {}/{} size={})",
                        t,
                        file_id,
                        i + 1,
                        total_chunks,
                        end - start
                    ),
                );
            }

            let bytes = bincode::serialize(&msg).unwrap();
            for peer in &st.children_by_tree[t] {
                let _ = st.tx.send((peer.clone(), bytes.clone())).await;
            }
        }
    }

    (StatusCode::OK, "ok")
}

// ----------------------------
// Main
// ----------------------------
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: gossip_node <node_yaml> <cluster_yaml>");
        std::process::exit(1);
    }

    let node_path = &args[1];
    let cluster_path = &args[2];

    let node_cfg: NodeCfg = serde_yaml::from_str(&std::fs::read_to_string(node_path)?)?;
    let cluster_cfg: ClusterCfg = serde_yaml::from_str(&std::fs::read_to_string(cluster_path)?)?;

    let my_id = node_cfg.id.clone();
    println!("[{}] starting", my_id);

    let peers_all: Vec<Peer> = cluster_cfg
        .nodes
        .iter()
        .map(|p| Peer {
            id: p.id.clone(),
            addr: p.addr.clone(),
        })
        .collect();

    let orders = make_orders(peers_all.clone(), cluster_cfg.num_trees);

    let mut children_by_tree: Vec<Vec<Peer>> = Vec::with_capacity(cluster_cfg.num_trees);
    for t in 0..cluster_cfg.num_trees {
        let children = kary_children(&orders[t], &my_id, cluster_cfg.fanout);
        children_by_tree.push(children);
    }

    // APP outgoing
    let (tx, mut rx) = mpsc::channel::<(Peer, Vec<u8>)>(1024);

    // Internal APP<->PR
    let (pr_to_app_tx, mut pr_to_app_rx) = mpsc::channel::<String>(256);
    let (app_to_pr_tx, mut app_to_pr_rx) = mpsc::channel::<String>(256);

    let st = AppState {
        node_id: my_id.clone(),
        known_msgs: Arc::new(Mutex::new(HashSet::new())),
        inbox: Arc::new(Mutex::new(VecDeque::new())),
        children_by_tree: Arc::new(children_by_tree),
        tx: tx.clone(),
        pr_to_app_tx: pr_to_app_tx.clone(),
        app_to_pr_tx: app_to_pr_tx.clone(),
        pr_rx_count: Arc::new(Mutex::new(0)),
        pr_tx_count: Arc::new(Mutex::new(0)),
    };

    // APP and PR listeners
    let app_listen: SocketAddr = node_cfg.listen_addr.parse()?;
    let mut pr_listen = app_listen;
    pr_listen.set_port(app_listen.port() + 1);

    let app_listener = TcpListener::bind(app_listen).await?;
    println!("[{}] listening APP on {}", my_id, app_listen);

    let pr_listener = TcpListener::bind(pr_listen).await?;
    println!("[{}] listening PR  on {}", my_id, pr_listen);

    // PR logger (demo)
    let log_path = PathBuf::from(format!("peerreview_logs/{}.log", my_id));
    std::fs::create_dir_all("peerreview_logs")?;

    let signing_key = {
        use ed25519_dalek::SigningKey;
        let b = [my_id.as_bytes()[0]; 32];
        SigningKey::from_bytes(&b)
    };

    let pr_id_u32 = my_id.trim_start_matches("node").parse::<u32>().unwrap_or(0);
    let _pr_logger = Arc::new(Mutex::new(PrLogger::open(
        pr_id_u32,
        &log_path,
        signing_key,
    )?));

    // PR network layer exists, can be used later
    let _pr_net = PrNetwork::new();

    // --- PR listen task (PR socket)
    {
        let my_id_for_accept = my_id.clone();
        let pr_to_app_tx = pr_to_app_tx.clone();
        let pr_rx_count = st.pr_rx_count.clone();

        tokio::spawn(async move {
            loop {
                let (mut sock, addr) = match pr_listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[{}] PR accept error: {}", my_id_for_accept, e);
                        break;
                    }
                };

                println!("[{}] PR connection from {}", my_id_for_accept, addr);

                let pr_to_app_tx = pr_to_app_tx.clone();
                let pr_rx_count = pr_rx_count.clone();
                let my_id_conn = my_id_for_accept.clone();

                tokio::spawn(async move {
                    loop {
                        let mut len_buf = [0u8; 4];
                        if sock.read_exact(&mut len_buf).await.is_err() {
                            break;
                        }
                        let len = u32::from_be_bytes(len_buf) as usize;
                        let mut buf = vec![0u8; len];
                        if sock.read_exact(&mut buf).await.is_err() {
                            break;
                        }

                        *pr_rx_count.lock().unwrap() += 1;

                        if let Ok((msg, _used)) = bincode2::decode_from_slice::<PeerReviewMsg, _>(
                            &buf,
                            bincode2::config::standard(),
                        ) {
                            println!("[{}] PR {:?}", my_id_conn, msg);
                            let _ = pr_to_app_tx.send(format!("PR_RX {:?}", msg)).await;
                        } else {
                            println!("[{}] PR RAW {} bytes from {}", my_id_conn, len, addr);
                        }
                    }
                });
            }
        });
    }

    // --- APP send loop task
    tokio::spawn(async move {
        while let Some((peer, bytes)) = rx.recv().await {
            match TcpStream::connect(&peer.addr).await {
                Ok(mut stream) => {
                    let len = bytes.len() as u32;
                    let mut frame = Vec::with_capacity(4 + bytes.len());
                    frame.extend_from_slice(&len.to_be_bytes());
                    frame.extend_from_slice(&bytes);
                    let _ = stream.write_all(&frame).await;
                }
                Err(e) => {
                    eprintln!("send connect {} failed: {}", peer.addr, e);
                }
            }
        }
    });

    // --- Heartbeats per tree
    {
        let stc = st.clone();
        let hb_ms = node_cfg.heartbeat_ms;
        tokio::spawn(async move {
            let mut counter: u64 = 0;
            loop {
                sleep(Duration::from_millis(hb_ms)).await;
                counter += 1;

                for t in 0..stc.children_by_tree.len() {
                    let tt = t as u8;

                    let msg = Msg {
                        id: format!("{}-hb-{}-{}", stc.node_id, tt, counter),
                        from: stc.node_id.clone(),
                        tree_id: tt,
                        kind: MsgKind::Heartbeat {
                            tree_id: tt,
                            counter,
                        },
                        payload: vec![],
                        ts_ms: now_ms(),
                    };

                    let key = (msg.tree_id as u32, msg.id.clone());
                    stc.known_msgs.lock().unwrap().insert(key);

                    let bytes = bincode::serialize(&msg).unwrap();
                    for peer in &stc.children_by_tree[t] {
                        let _ = stc.tx.send((peer.clone(), bytes.clone())).await;
                    }
                }
            }
        });
    }

    // --- Internal PR -> APP
    {
        let stc = st.clone();
        tokio::spawn(async move {
            while let Some(line) = pr_to_app_rx.recv().await {
                let mut inbox = stc.inbox.lock().unwrap();
                push_inbox(&mut inbox, format!("[INTERNAL PR->APP] {}", line));
            }
        });
    }

    // --- Internal APP -> PR (observable trace)
    {
        let my_id2 = my_id.clone();
        let pr_tx_count = st.pr_tx_count.clone();
        tokio::spawn(async move {
            let mut ctr: u64 = 0;
            while let Some(line) = app_to_pr_rx.recv().await {
                ctr += 1;
                *pr_tx_count.lock().unwrap() += 1;
                println!("[{}] [INTERNAL APP->PR] {} {}", my_id2, ctr, line);
            }
        });
    }

    // --- HTTP server
    let http_addr = SocketAddr::from(([0, 0, 0, 0], node_cfg.http_api));
    let app = Router::new()
        .route("/stats", get(get_stats))
        .route("/publish", post(post_publish))
        .route("/publish_binary_demo", post(post_publish_binary_demo))
        .with_state(st.clone());

    {
        let my_id_http = my_id.clone();
        tokio::spawn(async move {
            println!("[{}] HTTP on http://{}", my_id_http, http_addr);
            axum::serve(tokio::net::TcpListener::bind(http_addr).await.unwrap(), app)
                .await
                .unwrap();
        });
    }

    // --- APP receive loop
    loop {
        let (mut sock, _addr) = app_listener.accept().await?;
        let stc = st.clone();

        tokio::spawn(async move {
            loop {
                let mut len_buf = [0u8; 4];
                if sock.read_exact(&mut len_buf).await.is_err() {
                    break;
                }
                let len = u32::from_be_bytes(len_buf) as usize;
                let mut buf = vec![0u8; len];
                if sock.read_exact(&mut buf).await.is_err() {
                    break;
                }

                let msg: Msg = match bincode::deserialize(&buf) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("APP decode error: {}", e);
                        continue;
                    }
                };

                let key = (msg.tree_id as u32, msg.id.clone());
                if !stc.known_msgs.lock().unwrap().insert(key) {
                    continue;
                }

                match &msg.kind {
                    MsgKind::PublishText => {
                        let text = String::from_utf8_lossy(&msg.payload).to_string();
                        {
                            let mut inbox = stc.inbox.lock().unwrap();
                            push_inbox(
                                &mut inbox,
                                format!("(t{} from {}) {}", msg.tree_id, msg.from, text),
                            );
                        }
                        let _ = stc
                            .app_to_pr_tx
                            .send(format!(
                                "TEXT tree={} from={} id={}",
                                msg.tree_id, msg.from, msg.id
                            ))
                            .await;
                    }

                    MsgKind::PublishBinaryChunk => {
                        if let Ok(chunk) = bincode::deserialize::<BinaryChunk>(&msg.payload) {
                            let mut inbox = stc.inbox.lock().unwrap();
                            push_inbox(
                                &mut inbox,
                                format!(
                                    "(BIN t{} from {} file_id={} chunk {}/{} size={})",
                                    msg.tree_id,
                                    msg.from,
                                    chunk.file_id,
                                    chunk.index + 1,
                                    chunk.total_chunks,
                                    chunk.data.len()
                                ),
                            );
                        }

                        let _ = stc
                            .app_to_pr_tx
                            .send(format!(
                                "BIN tree={} from={} id={}",
                                msg.tree_id, msg.from, msg.id
                            ))
                            .await;
                    }

                    MsgKind::Heartbeat { .. } => { /* silent */ }
                }

                // relay
                let t = msg.tree_id as usize;
                if t < stc.children_by_tree.len() {
                    let bytes = bincode::serialize(&msg).unwrap();
                    for peer in &stc.children_by_tree[t] {
                        let _ = stc.tx.send((peer.clone(), bytes.clone())).await;
                    }
                }
            }
        });
    }
}
