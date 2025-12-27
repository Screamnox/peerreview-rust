use anyhow::Result;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use common_proto::{BinaryChunk, Msg, MsgKind, NodeId};
use dashmap::DashSet;
use parking_lot::Mutex;
use rand::{seq::SliceRandom, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

/// ----------------- CLI -----------------
#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    config: String,
    #[arg(long)]
    cluster: String,
}

/// ----------------- Config -----------------
#[derive(Debug, Deserialize, Clone)]
struct NodeCfg {
    id: String,
    listen_addr: String, // ex "0.0.0.0:7001"
    heartbeat_ms: u64,
    gossip_ms: u64,
    anti_entropy_ms: u64,
    http_api: u16, // ex 8081
}

#[derive(Debug, Deserialize, Clone)]
struct Peer {
    id: String,
    addr: String, // ex "node3:7001" ou "127.0.0.1:7001"
}

#[derive(Debug, Deserialize)]
struct ClusterCfg {
    nodes: Vec<Peer>,
    fanout: usize,   // k
    num_trees: usize, // T
}

/// ----------------- État du nœud -----------------

/// Buffer pour reconstituer un flux binaire reçu par chunks.
struct BinaryFileBuffer {
    total_chunks: u32,
    received: HashMap<u32, Vec<u8>>,
}

#[derive(Clone)]
struct NodeState {
    my_id: NodeId,
    // enfants par arbre: children_by_tree[t] = Vec<Peer>
    children_by_tree: Arc<Vec<Vec<Peer>>>,
    // anti-doublon: (tree_id, msg_id)
    known_msgs: Arc<DashSet<(u8, String)>>,
    // inbox texte/binaire pour /stats
    inbox: Arc<Mutex<VecDeque<String>>>,
    // buffers pour flux binaires (file_id -> BinaryFileBuffer)
    binary_buffers: Arc<Mutex<HashMap<String, BinaryFileBuffer>>>,
    // canal d'envoi TCP (APP)
    tx_send: mpsc::Sender<(Peer, Vec<u8>)>,
    // canal interne APP -> PR (tests / future intégration PeerReview)
    tx_to_pr: mpsc::Sender<String>,
}

#[derive(Serialize)]
struct Stats {
    node_id: String,
    known_count: usize,
    last_msgs: Vec<String>,
    trees: usize,
}

/// ----------------- Utilitaires -----------------

fn now_ms() -> u64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as u64
}

// ordre permuté déterministe des nœuds pour chaque arbre (seed fixe)
fn make_orders(nodes: &[Peer], num_trees: usize) -> Vec<Vec<Peer>> {
    let mut res = Vec::with_capacity(num_trees);
    for t in 0..num_trees {
        let mut v = nodes.to_vec();
        let mut rng = ChaCha8Rng::seed_from_u64(0xD15E_A5EEDu64 ^ (t as u64));
        v.as_mut_slice().shuffle(&mut rng);
        res.push(v);
    }
    res
}

// enfants k-aires d'un noeud (id) dans un ordre donné
fn kary_children(order: &[Peer], my_id: &str, k: usize) -> Vec<Peer> {
    let i = order
        .iter()
        .position(|p| p.id == my_id)
        .expect("current node id not found in order");
    let mut v = Vec::new();
    for j in 1..=k {
        let idx = i * k + j;
        if idx < order.len() {
            v.push(order[idx].clone());
        }
    }
    v
}

// push dans inbox (cap 200)
fn inbox_push(st: &Arc<NodeState>, line: String) {
    let mut q = st.inbox.lock();
    q.push_back(line);
    if q.len() > 200 {
        q.pop_front();
    }
}

/// ----------------- HTTP handlers -----------------

#[derive(Deserialize)]
struct PublishInput {
    payload: String,
}

async fn get_stats(State(st): State<Arc<NodeState>>) -> Json<Stats> {
    let known_count = st.known_msgs.len();
    let mut last_msgs: Vec<String> = st.inbox.lock().iter().cloned().collect();
    if last_msgs.len() > 15 {
        last_msgs = last_msgs[last_msgs.len() - 15..].to_vec();
    }
    Json(Stats {
        node_id: st.my_id.clone(),
        known_count,
        last_msgs,
        trees: st.children_by_tree.len(),
    })
}

async fn post_publish(State(st): State<Arc<NodeState>>, Json(input): Json<PublishInput>) -> Json<&'static str> {
    // 1) événement APP -> PR (Test C)
    let _ = st
        .tx_to_pr
        .send(format!("EVENT publish(text='{}')", input.payload))
        .await;

    // 2) diffusion normale APP (multi-arbres)
    for tt in 0u8..(st.children_by_tree.len() as u8) {
        let msg = Msg {
            id: uuid::Uuid::new_v4().to_string(),
            from: st.my_id.clone(),
            kind: MsgKind::PublishText,
            payload: input.payload.as_bytes().to_vec(),
            ts_ms: now_ms(),
            tree_id: tt,
        };

        st.known_msgs.insert((tt, msg.id.clone()));
        inbox_push(
            &st,
            format!("(local t{} from {}) {}", tt, st.my_id, input.payload),
        );

        let bytes = bincode::serialize(&msg).unwrap();
        for peer in &st.children_by_tree[tt as usize] {
            let _ = st.tx_send.send((peer.clone(), bytes.clone())).await;
        }
    }

    Json("ok")
}

#[derive(Deserialize)]
struct PublishBinaryDemoInput {
    file_id: String,
    total_size: usize,
    chunk_size: usize,
}

async fn post_publish_binary_demo(
    State(st): State<Arc<NodeState>>,
    Json(input): Json<PublishBinaryDemoInput>,
) -> Json<&'static str> {
    // événement APP -> PR (Test C)
    let _ = st
        .tx_to_pr
        .send(format!(
            "EVENT publish_binary(file_id={}, total_size={}, chunk_size={})",
            input.file_id, input.total_size, input.chunk_size
        ))
        .await;

    let total_size = input.total_size;
    let chunk_size = input.chunk_size.max(1);
    let num_chunks = (total_size + chunk_size - 1) / chunk_size;

    let mut data = vec![0u8; total_size];
    rand::thread_rng().fill_bytes(&mut data);

    for tt in 0u8..(st.children_by_tree.len() as u8) {
        for i in 0..num_chunks {
            let start = i * chunk_size;
            let end = ((i + 1) * chunk_size).min(total_size);
            let slice = &data[start..end];

            let chunk = BinaryChunk {
                file_id: input.file_id.clone(),
                index: i as u32,
                total_chunks: num_chunks as u32,
                data: slice.to_vec(),
            };

            let payload = bincode::serialize(&chunk).unwrap();
            let msg = Msg {
                id: uuid::Uuid::new_v4().to_string(),
                from: st.my_id.clone(),
                kind: MsgKind::PublishBinaryChunk,
                payload,
                ts_ms: now_ms(),
                tree_id: tt,
            };

            st.known_msgs.insert((tt, msg.id.clone()));

            inbox_push(
                &st,
                format!(
                    "(local BIN t{} {} chunk {}/{} size={})",
                    tt,
                    input.file_id,
                    i + 1,
                    num_chunks,
                    slice.len()
                ),
            );

            let bytes = bincode::serialize(&msg).unwrap();
            for peer in &st.children_by_tree[tt as usize] {
                let _ = st.tx_send.send((peer.clone(), bytes.clone())).await;
            }
        }
    }

    Json("ok")
}

/// ----------------- Main -----------------

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Load configs
    let node_cfg: NodeCfg = serde_yaml::from_str(&fs::read_to_string(&args.config)?)?;
    let cluster_cfg: ClusterCfg = serde_yaml::from_str(&fs::read_to_string(&args.cluster)?)?;

    // Build trees
    let orders = make_orders(&cluster_cfg.nodes, cluster_cfg.num_trees);
    let mut children_by_tree: Vec<Vec<Peer>> = Vec::with_capacity(cluster_cfg.num_trees);
    for t in 0..cluster_cfg.num_trees {
        let order = &orders[t];
        let ch = kary_children(order, &node_cfg.id, cluster_cfg.fanout);
        children_by_tree.push(ch);
    }

    println!(
        "{} children_by_tree = {:?}",
        node_cfg.id,
        children_by_tree
            .iter()
            .enumerate()
            .map(|(ti, ch)| {
                format!(
                    "t{}: {:?}",
                    ti,
                    ch.iter().map(|p| &p.id).collect::<Vec<_>>()
                )
            })
            .collect::<Vec<_>>()
    );

    // --- Listeners TCP ---
    // Socket APP : messages applicatifs (texte + binaire + heartbeats)
    let app_listen: SocketAddr = node_cfg.listen_addr.parse()?;

    // Socket PR : même IP, port_app + 1, pour PeerReview
    let mut pr_listen = app_listen;
    pr_listen.set_port(app_listen.port() + 1);

    let app_listener = TcpListener::bind(app_listen).await?;
    println!("{} listening APP on TCP {}", node_cfg.id, app_listen);

    let pr_listener = TcpListener::bind(pr_listen).await?;
    println!("{} listening PR  on TCP {}", node_cfg.id, pr_listen);

    // Canal d'envoi APP
    let (tx, mut rx) = mpsc::channel::<(Peer, Vec<u8>)>(2048);

    // --- Bus interne APP ↔ PR (dans un nœud) ---
    // Test C: prouve qu'APP et PR (2 tâches) peuvent s'échanger des messages.
    let (tx_to_pr, mut rx_from_app) = mpsc::channel::<String>(256);
    let (tx_to_app, mut rx_from_pr) = mpsc::channel::<String>(256);

    // État partagé
    let st = NodeState {
        my_id: node_cfg.id.clone(),
        children_by_tree: Arc::new(children_by_tree),
        known_msgs: Arc::new(DashSet::new()),
        inbox: Arc::new(Mutex::new(VecDeque::new())),
        binary_buffers: Arc::new(Mutex::new(HashMap::new())),
        tx_send: tx.clone(),
        tx_to_pr: tx_to_pr.clone(),
    };
    let st = Arc::new(st);

    // PR -> APP : tout ce qui arrive de PR (ACK ou events PR) est visible via /stats
    {
        let stc = st.clone();
        tokio::spawn(async move {
            while let Some(line) = rx_from_pr.recv().await {
                inbox_push(&stc, format!("(PR→APP) {}", line));
            }
        });
    }

    // APP -> PR : le thread PR reçoit des événements APP et renvoie un ACK vers APP
    {
        let my_id = st.my_id.clone();
        let tx_to_app = tx_to_app.clone();
        tokio::spawn(async move {
            while let Some(line) = rx_from_app.recv().await {
                println!("[{}] PR got (APP→PR): {}", my_id, line);
                let _ = tx_to_app
                    .send(format!("ACK from PR: received '{}'", line))
                    .await;
            }
        });
    }

    // Tâche d'envoi TCP (APP)
    {
        tokio::spawn(async move {
            while let Some((peer, bytes)) = rx.recv().await {
                match TcpStream::connect(&peer.addr).await {
                    Ok(mut s) => {
                        let _ = s.write_all(&(bytes.len() as u32).to_be_bytes()).await;
                        let _ = s.write_all(&bytes).await;
                    }
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                    }
                }
            }
        });
    }

    // Boucle de réception PR (TCP PR): on log + on envoie un résumé vers APP via tx_to_app
    {
        let my_id = st.my_id.clone();
        let tx_to_app = tx_to_app.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, addr) = match pr_listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[{}] error on PR accept: {}", my_id, e);
                        break;
                    }
                };
                println!("[{}] PR connection from {}", my_id, addr);

                let tx_to_app2 = tx_to_app.clone();
                let my_id2 = my_id.clone();
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

                        println!("[{}] PR RAW {} bytes from {}", my_id2, len, addr);

                        let preview_len = len.min(16);
                        let preview = buf[..preview_len]
                            .iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<Vec<_>>()
                            .join(" ");

                        let _ = tx_to_app2
                            .send(format!(
                                "RAW {} bytes from {} | first={} [{}]",
                                len, addr, preview_len, preview
                            ))
                            .await;
                    }
                });
            }
        });
    }

    // Heartbeats APP
    {
        let stc = st.clone();
        let period = node_cfg.heartbeat_ms;
        tokio::spawn(async move {
            let mut counter = 0u64;
            loop {
                tokio::time::sleep(Duration::from_millis(period)).await;
                counter += 1;

                for tt in 0u8..(stc.children_by_tree.len() as u8) {
                    let msg = Msg {
                        id: uuid::Uuid::new_v4().to_string(),
                        from: stc.my_id.clone(),
                        kind: MsgKind::Heartbeat { counter, tree_id: tt },
                        payload: vec![],
                        ts_ms: now_ms(),
                        tree_id: tt,
                    };

                    stc.known_msgs.insert((tt, msg.id.clone()));

                    let bytes = bincode::serialize(&msg).unwrap();
                    for peer in &stc.children_by_tree[tt as usize] {
                        let _ = stc.tx_send.send((peer.clone(), bytes.clone())).await;
                    }
                }
            }
        });
    }

    // HTTP server
    {
        let stc = st.clone();
        let http_addr: SocketAddr = format!("0.0.0.0:{}", node_cfg.http_api).parse()?;
        let router = Router::new()
            .route("/stats", get(get_stats))
            .route("/publish", post(post_publish))
            .route("/publish_binary_demo", post(post_publish_binary_demo))
            .with_state(stc);

        tokio::spawn(async move {
            let listener = TcpListener::bind(http_addr).await.unwrap();
            println!("HTTP on http://{}", http_addr);
            axum::serve(listener, router).await.unwrap();
        });
    }

    // Boucle de réception TCP APP : diffusion (texte/binaire/heartbeat)
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
                    Err(_) => continue,
                };

                let key = (msg.tree_id, msg.id.clone());
                if !stc.known_msgs.insert(key) {
                    continue;
                }

                match msg.kind {
                    MsgKind::Heartbeat { counter, .. } => {
                        inbox_push(
                            &stc,
                            format!(
                                "(HB t{} #{} from {})",
                                msg.tree_id, counter, msg.from
                            ),
                        );
                    }
                    MsgKind::PublishText => {
                        let text = String::from_utf8_lossy(&msg.payload).to_string();
                        inbox_push(
                            &stc,
                            format!("(t{} from {}) {}", msg.tree_id, msg.from, text),
                        );
                        let bytes = bincode::serialize(&msg).unwrap();
                        for peer in &stc.children_by_tree[msg.tree_id as usize] {
                            let _ = stc.tx_send.send((peer.clone(), bytes.clone())).await;
                        }
                    }
                    MsgKind::PublishBinaryChunk => {
                        let chunk: BinaryChunk = match bincode::deserialize(&msg.payload) {
                            Ok(c) => c,
                            Err(e) => {
                                eprintln!(
                                    "[{}] invalid BinaryChunk from {}: {e}",
                                    stc.my_id, msg.from
                                );
                                continue;
                            }
                        };

                        {
                            let mut buffers = stc.binary_buffers.lock();
                            let entry = buffers.entry(chunk.file_id.clone()).or_insert_with(|| {
                                BinaryFileBuffer {
                                    total_chunks: chunk.total_chunks,
                                    received: HashMap::new(),
                                }
                            });

                            entry.received.insert(chunk.index, chunk.data.clone());

                            inbox_push(
                                &stc,
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

                            if entry.received.len() as u32 == entry.total_chunks {
                                inbox_push(
                                    &stc,
                                    format!(
                                        "(BIN COMPLETE from {} file_id={} total_chunks={})",
                                        msg.from, chunk.file_id, entry.total_chunks
                                    ),
                                );
                            }
                        }

                        let bytes = bincode::serialize(&msg).unwrap();
                        for peer in &stc.children_by_tree[msg.tree_id as usize] {
                            let _ = stc.tx_send.send((peer.clone(), bytes.clone())).await;
                        }
                    }
                }
            }
        });
    }
}
