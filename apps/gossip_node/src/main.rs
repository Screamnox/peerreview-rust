use std::{
    collections::{HashSet, VecDeque},
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use common_proto::{Msg, MsgKind};
use ed25519_dalek::SigningKey;
use peerreview_protocol::{
    journal::Logger as PrLogger,
    types::config::{ClusterConfig, ClusterNode},
};
use rand::{seq::SliceRandom, RngCore};
use sha2::{Digest, Sha256};
use tokio::{
    net::{TcpListener, TcpStream, UdpSocket},
    sync::Mutex,
    task::JoinHandle,
};

/// ================================
/// CLI
/// ================================
#[derive(Parser, Debug, Clone)]
#[command(author, version, about)]
struct Args {
    /// Nom du noeud (ex: node1, node2...). On matche sur cluster.nodes[*].name en priorité.
    #[arg(long)]
    name: String,

    /// Fichier YAML cluster (docker/cluster.yaml)
    #[arg(long, default_value = "docker/cluster.yaml")]
    cluster: PathBuf,

    /// Répertoire pour logs peerreview
    #[arg(long, default_value = "peerreview_logs")]
    log_dir: PathBuf,

    /// Activer endpoints HTTP
    #[arg(long, default_value_t = true)]
    http: bool,
}

/// ================================
/// Runtime data
/// ================================
#[derive(Clone)]
struct Peer {
    id: u32,
    name: String,
    app_addr: SocketAddr,
    pr_addr: SocketAddr,
}

struct NodeCfg {
    my_id: u32,
    my_name: String,
    app_addr: SocketAddr,
    pr_addr: SocketAddr,
    http_addr: Option<SocketAddr>,
    peers: Vec<Peer>,
    fanout: usize,
    num_trees: u8,
}

#[derive(Default)]
struct AppStats {
    recv_total: u64,
    deliver_total: u64,
    hb_total: u64,
    publish_text_total: u64,
    publish_bin_total: u64,
    unique_msg_ids: usize,
}

struct AppState {
    cfg: NodeCfg,
    udp: Arc<UdpSocket>,
    stats: Mutex<AppStats>,
    seen: Mutex<HashSet<String>>,
    last_events: Mutex<VecDeque<String>>,
    pr_logger: Mutex<PrLogger>,
    hb_counter: AtomicU64,
}

#[derive(serde::Serialize)]
struct StatsOut {
    me: String,
    recv_total: u64,
    deliver_total: u64,
    hb_total: u64,
    publish_text_total: u64,
    publish_bin_total: u64,
    unique_msg_ids: usize,
    last_events: Vec<String>,
}

/// ================================
/// Helpers
/// ================================
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

/// Génère une SigningKey déterministe depuis le nom (sha256(name) -> 32 bytes)
fn signing_key_from_node_name(name: &str) -> SigningKey {
    let mut h = Sha256::new();
    h.update(name.as_bytes());
    let digest = h.finalize();
    let seed: [u8; 32] = digest.as_slice().try_into().expect("sha256 len");
    SigningKey::from_bytes(&seed)
}

/// Hash stable d’un Msg (pour journaliser)
fn hash_msg_32(m: &Msg) -> [u8; 32] {
    let mut h = Sha256::new();

    // id, from : String
    h.update(m.id.as_bytes());
    h.update(b"|");
    h.update(m.from.as_bytes());
    h.update(b"|");

    // kind : bincode stable
    if let Ok(kb) = bincode::serialize(&m.kind) {
        h.update(&kb);
    } else {
        match m.kind {
            MsgKind::Heartbeat { .. } => h.update(b"Heartbeat"),
            MsgKind::PublishText => h.update(b"PublishText"),
            MsgKind::PublishBinaryChunk => h.update(b"PublishBinaryChunk"),
        }
    }

    h.update(b"|");
    h.update(&m.payload);
    h.update(b"|");
    h.update(m.ts_ms.to_be_bytes());
    h.update([m.tree_id]);

    let out = h.finalize();
    out.as_slice().try_into().expect("sha256 len")
}

async fn push_event(state: &Arc<AppState>, s: impl Into<String>) {
    let mut ev = state.last_events.lock().await;
    if ev.len() >= 200 {
        ev.pop_front();
    }
    ev.push_back(s.into());
}

/// ================================
/// Cluster parsing
/// ================================
fn parse_cluster(args: &Args) -> Result<NodeCfg> {
    let cluster =
        ClusterConfig::from_yaml_file(&args.cluster).context("ClusterConfig::from_yaml_file")?;

    // IMPORTANT: chez toi, fanout/num_trees sont des usize (pas Option)
    let fanout = cluster.fanout as usize;
    let num_trees = cluster.num_trees as u8;

    let me: &ClusterNode = cluster
        .nodes
        .iter()
        .find(|n| n.name == args.name)
        .or_else(|| {
            if let Ok(id) = args.name.parse::<u32>() {
                cluster.nodes.iter().find(|n| n.id == id)
            } else {
                None
            }
        })
        .with_context(|| format!("node '{}' not found in cluster config", args.name))?;

    let app_addr: SocketAddr = me
        .app_addr
        .parse()
        .with_context(|| format!("bad app_addr for {}: {}", me.name, me.app_addr))?;

    let pr_addr: SocketAddr = me
        .pr_addr
        .parse()
        .with_context(|| format!("bad pr_addr for {}: {}", me.name, me.pr_addr))?;

    let http_addr: Option<SocketAddr> = me.http_addr.as_ref().and_then(|s| s.parse().ok());

    let mut peers = Vec::new();
    for p in &cluster.nodes {
        if p.id == me.id {
            continue;
        }
        let p_app: SocketAddr = p
            .app_addr
            .parse()
            .with_context(|| format!("bad app_addr for {}: {}", p.name, p.app_addr))?;

        let p_pr: SocketAddr = p
            .pr_addr
            .parse()
            .with_context(|| format!("bad pr_addr for {}: {}", p.name, p.pr_addr))?;

        peers.push(Peer {
            id: p.id,
            name: p.name.clone(),
            app_addr: p_app,
            pr_addr: p_pr,
        });
    }

    Ok(NodeCfg {
        my_id: me.id,
        my_name: me.name.clone(),
        app_addr,
        pr_addr,
        http_addr,
        peers,
        fanout,
        num_trees,
    })
}

/// ================================
/// UDP listener
/// ================================
async fn udp_listener_task(state: Arc<AppState>) -> Result<()> {
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        let (n, src) = state.udp.recv_from(&mut buf).await?;
        let bytes = &buf[..n];

        if let Ok(m) = bincode::deserialize::<Msg>(bytes) {
            let mid = m.id.clone();

            {
                let mut st = state.stats.lock().await;
                st.recv_total += 1;
                match m.kind {
                    MsgKind::Heartbeat { .. } => st.hb_total += 1,
                    MsgKind::PublishText => st.publish_text_total += 1,
                    MsgKind::PublishBinaryChunk => st.publish_bin_total += 1,
                }
            }

            // Logger::log_app chez toi prend 5 args (kind, peer, msg_id:String, hash32, ts_ms)
            {
                let hash32 = hash_msg_32(&m);
                let ts_ms = m.ts_ms;
                let mut lg = state.pr_logger.lock().await;
                let _ = lg.log_app("RECV", Some(src.port() as u32), mid.clone(), hash32, ts_ms);
            }

            push_event(&state, format!("[UDP] from={src} id={} kind={:?}", m.id, m.kind)).await;

            handle_incoming_msg(&state, m).await?;
        } else {
            push_event(&state, format!("[UDP] invalid msg from={src} n={n}")).await;
        }
    }
}

async fn handle_incoming_msg(state: &Arc<AppState>, m: Msg) -> Result<()> {
    // dedup
    {
        let mut seen = state.seen.lock().await;
        if !seen.insert(m.id.clone()) {
            return Ok(());
        }
        let mut st = state.stats.lock().await;
        st.unique_msg_ids = seen.len();
    }

    // deliver
    {
        let mut st = state.stats.lock().await;
        st.deliver_total += 1;
    }

    // journal DELIVER
    {
        let hash32 = hash_msg_32(&m);
        let ts_ms = m.ts_ms;
        let mut lg = state.pr_logger.lock().await;
        let _ = lg.log_app("DELIVER", None, m.id.clone(), hash32, ts_ms);
    }

    gossip_forward(state, &m).await?;
    Ok(())
}

async fn gossip_forward(state: &Arc<AppState>, m: &Msg) -> Result<()> {
    let bytes = bincode::serialize(m).context("bincode::serialize Msg")?;

    let mut peers = state.cfg.peers.clone();

    // IMPORTANT (Tokio): ne pas garder ThreadRng à travers un .await (future non-Send)
    {
        let mut rng = rand::thread_rng();
        peers.shuffle(&mut rng);
    } // <- rng DROPPÉ ici

    let take = state.cfg.fanout.min(peers.len());
    for p in peers.into_iter().take(take) {
        let _ = state.udp.send_to(&bytes, p.app_addr).await;

        // journal SEND
        let hash32 = hash_msg_32(m);
        let ts_ms = m.ts_ms;
        let mut lg = state.pr_logger.lock().await;
        let _ = lg.log_app("SEND", Some(p.id), m.id.clone(), hash32, ts_ms);
    }
    Ok(())
}

/// ================================
/// PR TCP listener (simple accept + trace)
/// ================================
async fn pr_tcp_listener_task(state: Arc<AppState>) -> Result<()> {
    let listener = TcpListener::bind(state.cfg.pr_addr)
        .await
        .with_context(|| format!("bind pr_addr {}", state.cfg.pr_addr))?;

    push_event(&state, format!("[PR] listening on {}", state.cfg.pr_addr)).await;

    loop {
        let (sock, src) = listener.accept().await?;
        let st = state.clone();
        tokio::spawn(async move {
            let st2 = st.clone(); // pour pouvoir relogger après un move
            if let Err(e) = handle_pr_connection(st, sock, src).await {
                let _ = push_event(&st2, format!("[PR] conn err: {e:#}")).await;
            }
        });
    }
}

async fn handle_pr_connection(state: Arc<AppState>, _sock: TcpStream, src: SocketAddr) -> Result<()> {
    {
        let mut lg = state.pr_logger.lock().await;
        let _ = lg.log_pr_in(&format!("accepted tcp PR from {src}"));
    }
    push_event(&state, format!("[PR] accepted {src}")).await;
    Ok(())
}

/// ================================
/// Heartbeat scheduler
/// ================================
async fn heartbeat_task(state: Arc<AppState>) -> Result<()> {
    loop {
        tokio::time::sleep(Duration::from_millis(800)).await;

        let counter = state.hb_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let tree_id = if state.cfg.num_trees == 0 {
            0u8
        } else {
            (counter % (state.cfg.num_trees as u64)) as u8
        };

        let ts_ms = now_ms();
        let id = format!("{}-hb-{}-{}", state.cfg.my_name, tree_id, counter);

        let msg = Msg {
            id,
            from: state.cfg.my_name.clone(),
            kind: MsgKind::Heartbeat { counter, tree_id },
            payload: Vec::new(),
            ts_ms,
            tree_id,
        };

        handle_incoming_msg(&state, msg).await?;
    }
}

/// ================================
/// HTTP
/// ================================
#[derive(serde::Deserialize)]
struct PublishTextIn {
    text: String,
}

async fn http_stats(State(state): State<Arc<AppState>>) -> Json<StatsOut> {
    let st = state.stats.lock().await;
    let ev = state.last_events.lock().await;

    Json(StatsOut {
        me: state.cfg.my_name.clone(),
        recv_total: st.recv_total,
        deliver_total: st.deliver_total,
        hb_total: st.hb_total,
        publish_text_total: st.publish_text_total,
        publish_bin_total: st.publish_bin_total,
        unique_msg_ids: st.unique_msg_ids,
        last_events: ev.iter().cloned().collect(),
    })
}

// NOTE: handlers axum -> utilise std::result::Result (PAS anyhow::Result)
async fn http_publish_text(
    State(state): State<Arc<AppState>>,
    Json(inp): Json<PublishTextIn>,
) -> impl IntoResponse {
    let ts_ms = now_ms();
    let id = format!("{}-txt-{}", state.cfg.my_name, ts_ms);

    let msg = Msg {
        id: id.clone(),
        from: state.cfg.my_name.clone(),
        kind: MsgKind::PublishText,
        payload: inp.text.into_bytes(),
        ts_ms,
        tree_id: 0,
    };

    if let Err(e) = handle_incoming_msg(&state, msg).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "ok": false, "err": format!("{e:#}") })),
        );
    }

    (StatusCode::OK, Json(serde_json::json!({ "ok": true, "msg_id": id })))
}

#[derive(serde::Deserialize)]
struct PublishBinaryDemoIn {
    total_bytes: usize,
    chunk_size: usize,
    tree_id: u8,
}

async fn http_publish_binary_demo(
    State(state): State<Arc<AppState>>,
    Json(inp): Json<PublishBinaryDemoIn>,
) -> impl IntoResponse {
    let total = inp.total_bytes.max(1);
    let chunk_size = inp.chunk_size.max(1);
    let tree_id = inp.tree_id;

    let total_chunks = (total + chunk_size - 1) / chunk_size;

    // payload random
    let mut all = vec![0u8; total];

    // IMPORTANT (Tokio): ne pas garder ThreadRng à travers un .await (future non-Send)
    {
        let mut rng = rand::thread_rng();
        rng.fill_bytes(&mut all);
    } // <- rng DROPPÉ ici

    for idx in 0..total_chunks {
        let start = idx * chunk_size;
        let end = ((idx + 1) * chunk_size).min(total);
        let chunk = &all[start..end];

        let ts_ms = now_ms();
        let id = format!("{}-bin-{}-{}-{}", state.cfg.my_name, tree_id, idx, ts_ms);

        // IMPORTANT: chez toi PublishBinaryChunk est un variant "unit",
        // donc on encode idx/total_chunks dans payload si besoin.
        let mut payload = Vec::new();
        payload.extend_from_slice(&(idx as u32).to_be_bytes());
        payload.extend_from_slice(&(total_chunks as u32).to_be_bytes());
        payload.extend_from_slice(chunk);

        let msg = Msg {
            id,
            from: state.cfg.my_name.clone(),
            kind: MsgKind::PublishBinaryChunk,
            payload,
            ts_ms,
            tree_id,
        };

        if let Err(e) = handle_incoming_msg(&state, msg).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "ok": false, "err": format!("{e:#}") })),
            );
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "total_bytes": total,
            "chunk_size": chunk_size,
            "total_chunks": total_chunks,
            "tree_id": tree_id
        })),
    )
}

async fn run_http_server(state: Arc<AppState>) -> Result<()> {
    let bind = state
        .cfg
        .http_addr
        .unwrap_or_else(|| "0.0.0.0:0".parse().unwrap());

    let app = Router::new()
        .route("/stats", get(http_stats))
        .route("/publish_text", post(http_publish_text))
        .route("/publish_binary_demo", post(http_publish_binary_demo))
        .with_state(state);

    let listener = TcpListener::bind(bind).await?;
    let local = listener.local_addr()?;
    println!("[http] listening on http://{local}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// ================================
/// MAIN
/// ================================
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let cfg = parse_cluster(&args)?;

    // UDP bind
    let udp = Arc::new(
        UdpSocket::bind(cfg.app_addr)
            .await
            .with_context(|| format!("bind app_addr {}", cfg.app_addr))?,
    );

    // log dir + file
    std::fs::create_dir_all(&args.log_dir).ok();
    let log_path = args.log_dir.join(format!("{}.log", cfg.my_name));

    // signing key déterministe
    let signing_key = signing_key_from_node_name(&cfg.my_name);

    // Logger::open chez toi : open(node_id, path, signing_key)
    let pr_logger = PrLogger::open(cfg.my_id, &log_path, signing_key)
        .with_context(|| format!("Logger::open failed: {}", log_path.display()))?;

    let state = Arc::new(AppState {
        cfg,
        udp,
        stats: Mutex::new(AppStats::default()),
        seen: Mutex::new(HashSet::new()),
        last_events: Mutex::new(VecDeque::new()),
        pr_logger: Mutex::new(pr_logger),
        hb_counter: AtomicU64::new(0),
    });

    push_event(
        &state,
        format!(
            "[boot] me={} app={} pr={}",
            state.cfg.my_name, state.cfg.app_addr, state.cfg.pr_addr
        ),
    )
    .await;

    // tasks
    let t_udp: JoinHandle<Result<()>> = tokio::spawn(udp_listener_task(state.clone()));
    let t_pr: JoinHandle<Result<()>> = tokio::spawn(pr_tcp_listener_task(state.clone()));
    let t_hb: JoinHandle<Result<()>> = tokio::spawn(heartbeat_task(state.clone()));

    let t_http: Option<JoinHandle<Result<()>>> = if args.http {
        Some(tokio::spawn(run_http_server(state.clone())))
    } else {
        None
    };

    // wait ctrl+c
    tokio::signal::ctrl_c().await.ok();
    push_event(&state, "[shutdown] ctrl_c received").await;

    t_udp.abort();
    t_pr.abort();
    t_hb.abort();
    if let Some(t) = t_http {
        t.abort();
    }

    Ok(())
}
