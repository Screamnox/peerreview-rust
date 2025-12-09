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
    // canal d'envoi TCP
    tx_send: mpsc::Sender<(Peer, Vec<u8>)>,
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
    let n = order.len();
    let i = order
        .iter()
        .position(|p| p.id == my_id)
        .expect("current node id not found in order");
    let mut v = Vec::new();
    for j in 1..=k {
        let idx = i * k + j;
        if idx < n {
            v.push(order[idx].clone());
        }
    }
    v
}

/// ----------------- HTTP Handlers -----------------

#[derive(Deserialize)]
struct PublishIn {
    payload: String,
}

async fn stats(State(st): State<Arc<NodeState>>) -> Json<Stats> {
    let mut last = Vec::new();
    {
        let mut q = st.inbox.lock();
        let take = q.len().min(10);
        for _ in 0..take {
            if let Some(s) = q.pop_back() {
                last.push(s);
            }
        }
        // remettre pour garder l'historique
        for s in last.iter().rev() {
            q.push_back(s.clone());
        }
    }
    Json(Stats {
        node_id: st.my_id.clone(),
        known_count: st.known_msgs.len(),
        last_msgs: last,
        trees: st.children_by_tree.len(),
    })
}

async fn publish(
    State(st): State<Arc<NodeState>>,
    Json(input): Json<PublishIn>,
) -> Json<&'static str> {
    // duplique le même message sur tous les arbres
    for tt in 0u8..(st.children_by_tree.len() as u8) {
        let msg = Msg {
            id: uuid::Uuid::new_v4().to_string(),
            from: st.my_id.clone(),
            kind: MsgKind::PublishText,
            payload: input.payload.clone().into_bytes(),
            ts_ms: now_ms(),
            tree_id: tt,
        };

        // marquer connu localement + journal local minimal
        st.known_msgs.insert((tt, msg.id.clone()));
        {
            let mut q = st.inbox.lock();
            q.push_back(format!(
                "(local t{}) {}",
                tt,
                String::from_utf8_lossy(&msg.payload)
            ));
            if q.len() > 200 {
                q.pop_front();
            }
        }

        let bytes = bincode::serialize(&msg).unwrap();
        for p in st.children_by_tree[tt as usize].iter() {
            let _ = st.tx_send.send((p.clone(), bytes.clone())).await;
        }
    }

    Json("ok")
}

#[derive(Deserialize)]
struct PublishBinaryDemoIn {
    file_id: String,
    total_size: usize,
    chunk_size: usize,
}

/// Démo : envoie un flux binaire simulé (octets aléatoires) découpé en chunks,
/// diffusé sur tous les arbres.
async fn publish_binary_demo(
    State(st): State<Arc<NodeState>>,
    Json(input): Json<PublishBinaryDemoIn>,
) -> Json<&'static str> {
    let total_size = input.total_size;
    let chunk_size = input.chunk_size.max(1);
    let num_chunks = (total_size + chunk_size - 1) / chunk_size;

    // Génère un buffer binaire aléatoire (simulation de fichier/vidéo)
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

            // dédoublon local
            st.known_msgs.insert((tt, msg.id.clone()));

            // log minimal dans l'inbox (côté émetteur)
            {
                let mut q = st.inbox.lock();
                q.push_back(format!(
                    "(local BIN t{} {} chunk {}/{} size={})",
                    tt,
                    input.file_id,
                    i + 1,
                    num_chunks,
                    slice.len(),
                ));
                if q.len() > 200 {
                    q.pop_front();
                }
            }

            let bytes = bincode::serialize(&msg).unwrap();
            for p in st.children_by_tree[tt as usize].iter() {
                let _ = st.tx_send.send((p.clone(), bytes.clone())).await;
            }
        }
    }

    Json("ok")
}

/// ----------------- main -----------------

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    eprintln!("CWD = {:?}", std::env::current_dir().unwrap());
    eprintln!("args = {:?}", std::env::args().collect::<Vec<_>>());
    if std::fs::metadata(&args.config).is_err() {
        eprintln!("❌ config introuvable: {}", &args.config);
    }
    if std::fs::metadata(&args.cluster).is_err() {
        eprintln!("❌ cluster introuvable: {}", &args.cluster);
    }

    let node_cfg: NodeCfg = serde_yaml::from_str(&fs::read_to_string(&args.config)?)?;
    let cluster_cfg: ClusterCfg =
        serde_yaml::from_str(&fs::read_to_string(&args.cluster)?)?;

    // Paramètres multi-arbres
    let k = cluster_cfg.fanout.max(1);
    let t = cluster_cfg.num_trees.max(1);

    let orders = make_orders(&cluster_cfg.nodes, t);

    // Enfants par arbre pour CE noeud
    let mut children_by_tree: Vec<Vec<Peer>> = Vec::with_capacity(t);
    for ord in &orders {
        children_by_tree.push(kary_children(ord, &node_cfg.id, k));
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

    // Canal d'envoi
    let (tx, mut rx) = mpsc::channel::<(Peer, Vec<u8>)>(2048);

    // État partagé
    let st = NodeState {
        my_id: node_cfg.id.clone(),
        children_by_tree: Arc::new(children_by_tree),
        known_msgs: Arc::new(DashSet::new()),
        inbox: Arc::new(Mutex::new(VecDeque::new())),
        binary_buffers: Arc::new(Mutex::new(HashMap::new())),
        tx_send: tx.clone(),
    };
    let st = Arc::new(st);

    // Tâche d'envoi TCP (avec mini backoff au boot)
    {
        tokio::spawn(async move {
            while let Some((peer, bytes)) = rx.recv().await {
                match TcpStream::connect(&peer.addr).await {
                    Ok(mut s) => {
                        let _ = s
                            .write_all(&(bytes.len() as u32).to_be_bytes())
                            .await;
                        let _ = s.write_all(&bytes).await;
                    }
                    Err(_e) => {
                        // au démarrage l'autre peut ne pas être prêt
                        tokio::time::sleep(Duration::from_millis(150)).await;
                    }
                }
            }
        });
    }

    // Boucle de réception PR (pour l'instant : on logge juste les octets bruts)
    {
        let my_id = st.my_id.clone();
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
                        println!(
                            "[{}] PR RAW {} bytes from {}",
                            my_id, len, addr
                        );
                        // plus tard : bincode::deserialize::<PRMsg>(&buf)
                    }
                });
            }
        });
    }

    // Heartbeats par arbre (facultatif mais utile pour visualiser)
    {
        let stc = st.clone();
        let period = node_cfg.heartbeat_ms;
        tokio::spawn(async move {
            let mut counter = 0u64;
            // petite pause pour laisser les pairs démarrer
            tokio::time::sleep(Duration::from_millis(600)).await;
            loop {
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
                    let bytes = bincode::serialize(&msg).unwrap();
                    for p in stc.children_by_tree[tt as usize].iter() {
                        let _ = stc.tx_send.send((p.clone(), bytes.clone())).await;
                    }
                }
                tokio::time::sleep(Duration::from_millis(period)).await;
            }
        });
    }

    // HTTP API
    {
        let http_addr = SocketAddr::from(([0, 0, 0, 0], node_cfg.http_api));
        let router = Router::new()
            .route("/stats", get(stats))
            .route("/publish", post(publish))
            .route("/publish_binary_demo", post(publish_binary_demo))
            .with_state(st.clone());
        println!(
            "{} HTTP on http://0.0.0.0:{}/",
            node_cfg.id, node_cfg.http_api
        );
        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(http_addr)
                .await
                .unwrap();
            axum::serve(listener, router).await.unwrap();
        });
    }

    // Réception TCP (socket APP : texte + binaire + heartbeats)
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
                match bincode::deserialize::<Msg>(&buf) {
                    Ok(msg) => {
                        let key = (msg.tree_id, msg.id.clone());
                        // dédoublon (tree_id, msg_id)
                        if !stc.known_msgs.insert(key) {
                            continue;
                        }

                        match &msg.kind {
                            MsgKind::Heartbeat { counter, .. } => {
                                println!(
                                    "[{}] HB t{} #{} from {}",
                                    stc.my_id, msg.tree_id, counter, msg.from
                                );
                                // pas de reforward obligatoire pour les HB
                            }
                            MsgKind::PublishText => {
                                // stocke un aperçu texte local
                                let text =
                                    String::from_utf8_lossy(&msg.payload)
                                        .to_string();
                                {
                                    let mut q = stc.inbox.lock();
                                    q.push_back(format!(
                                        "(t{} from {}) {}",
                                        msg.tree_id, msg.from, text
                                    ));
                                    if q.len() > 200 {
                                        q.pop_front();
                                    }
                                }

                                // forward aux enfants de CET arbre
                                let bytes =
                                    bincode::serialize(&msg).unwrap();
                                for p in stc.children_by_tree
                                    [msg.tree_id as usize]
                                    .iter()
                                {
                                    if p.id != stc.my_id {
                                        let _ = stc.tx_send.send((
                                            p.clone(),
                                            bytes.clone(),
                                        ));
                                    }
                                }
                            }
                            MsgKind::PublishBinaryChunk => {
                                // désérialiser le chunk
                                let chunk: BinaryChunk = match bincode
                                    ::deserialize(&msg.payload)
                                {
                                    Ok(c) => c,
                                    Err(e) => {
                                        eprintln!(
                                            "[{}] invalid BinaryChunk from {}: {e}",
                                            stc.my_id, msg.from
                                        );
                                        return;
                                    }
                                };

                                // log minimal terminal
                                println!(
                                    "[{}] BIN chunk from {} file_id={} {}/{} size={}",
                                    stc.my_id,
                                    msg.from,
                                    chunk.file_id,
                                    chunk.index + 1,
                                    chunk.total_chunks,
                                    chunk.data.len()
                                );

                                // log dans /stats pour voir la diffusion binaire
                                {
                                    let mut q = stc.inbox.lock();
                                    q.push_back(format!(
                                        "(BIN t{} from {} file_id={} chunk {}/{} size={})",
                                        msg.tree_id,
                                        msg.from,
                                        chunk.file_id,
                                        chunk.index + 1,
                                        chunk.total_chunks,
                                        chunk.data.len()
                                    ));
                                    if q.len() > 200 {
                                        q.pop_front();
                                    }
                                }

                                // mise à jour du buffer de reconstitution
                                {
                                    let mut buffers =
                                        stc.binary_buffers.lock();
                                    let entry = buffers
                                        .entry(chunk.file_id.clone())
                                        .or_insert(BinaryFileBuffer {
                                            total_chunks: chunk.total_chunks,
                                            received: HashMap::new(),
                                        });
                                    entry.received.insert(
                                        chunk.index,
                                        chunk.data.clone(),
                                    );

                                    // si on a tout reçu, on reconstitue en mémoire
                                    if entry.received.len() as u32
                                        == entry.total_chunks
                                    {
                                        let mut full = Vec::new();
                                        for i in 0..entry.total_chunks {
                                            if let Some(part) =
                                                entry.received.get(&i)
                                            {
                                                full.extend_from_slice(part);
                                            } else {
                                                eprintln!(
                                                    "[{}] manque chunk {} pour {}",
                                                    stc.my_id,
                                                    i,
                                                    chunk.file_id
                                                );
                                            }
                                        }
                                        println!(
                                            "[{}] Reconstitution complète de {} : {} octets",
                                            stc.my_id,
                                            chunk.file_id,
                                            full.len()
                                        );

                                        // trace aussi dans /stats
                                        let mut q = stc.inbox.lock();
                                        q.push_back(format!(
                                            "(BIN COMPLETE from {} file_id={} total_bytes={})",
                                            msg.from,
                                            chunk.file_id,
                                            full.len()
                                        ));
                                        if q.len() > 200 {
                                            q.pop_front();
                                        }
                                        // Option : buffers.remove(&chunk.file_id);
                                    }
                                }

                                // forward aux enfants de CET arbre
                                let bytes =
                                    bincode::serialize(&msg).unwrap();
                                for p in stc.children_by_tree
                                    [msg.tree_id as usize]
                                    .iter()
                                {
                                    if p.id != stc.my_id {
                                        let _ = stc.tx_send.send((
                                            p.clone(),
                                            bytes.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("[{}] invalid msg: {}", stc.my_id, e),
                }
            }
        });
    }
}
