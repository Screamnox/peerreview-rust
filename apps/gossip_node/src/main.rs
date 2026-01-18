use bincode::{Decode, Encode};
use clap::Parser;
use anyhow::{Context, Result};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use peerreview_protocol::{
    journal::logger::Logger as PrLogger,
    types::{
        config::{ClusterConfig, ClusterNode},
        messages::{Commitment, PeerReviewMsg},
        node::{node_id_from_name, signing_key_from_name, verifying_key_from_name},
        NodeId,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    time,
};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

fn sha256_32(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut a = [0u8; 32];
    a.copy_from_slice(&out[..32]);
    a
}

/// --------------------- PR TCP (bincode2) ---------------------
async fn pr_send(stream: &mut TcpStream, msg: &PeerReviewMsg) -> Result<()> {
    let payload =
        bincode::encode_to_vec(msg, bincode::config::standard()).context("bincode encode PR")?;
    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&payload).await?;
    Ok(())
}

async fn pr_recv(stream: &mut TcpStream) -> Result<PeerReviewMsg> {
    let mut lenb = [0u8; 4];
    stream.read_exact(&mut lenb).await?;
    let len = u32::from_be_bytes(lenb) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let (msg, _) =
        bincode::decode_from_slice::<PeerReviewMsg, _>(&buf, bincode::config::standard())
            .context("bincode decode PR")?;
    Ok(msg)
}

/// --------------------- APP TCP (bincode2) ---------------------
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
struct AppMsg {
    from: String,
    to: String,
    msg_id: String,
    ts_ms: u64,
    text: String,
}

async fn app_send(stream: &mut TcpStream, msg: &AppMsg) -> Result<()> {
    let payload =
        bincode::encode_to_vec(msg, bincode::config::standard()).context("bincode encode APP")?;
    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&payload).await?;
    Ok(())
}

async fn app_recv(stream: &mut TcpStream) -> Result<AppMsg> {
    let mut lenb = [0u8; 4];
    stream.read_exact(&mut lenb).await?;
    let len = u32::from_be_bytes(lenb) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let (msg, _) =
        bincode::decode_from_slice::<AppMsg, _>(&buf, bincode::config::standard())
            .context("bincode decode APP")?;
    Ok(msg)
}

#[derive(Clone)]
struct Peer {
    id: NodeId,
    name: String,
    pr_addr: String,
}

#[derive(Default)]
struct WitnessStore {
    commits: HashMap<NodeId, Vec<Commitment>>,
}

#[derive(Clone, Default)]
struct FaultConfig {
    equivocate_head: bool,
    drop_from: Option<String>,
    forge_recv_from: Option<String>,
}

fn parse_fault(s: &str) -> FaultConfig {
    // formats:
    //  "equivocate_head"
    //  "drop_from=node4"
    //  "forge_recv_from=node4"
    //  combinations: "equivocate_head,drop_from=node4"
    let mut f = FaultConfig::default();
    for part in s.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()) {
        if part == "equivocate_head" {
            f.equivocate_head = true;
        } else if let Some(v) = part.strip_prefix("drop_from=") {
            f.drop_from = Some(v.to_string());
        } else if let Some(v) = part.strip_prefix("forge_recv_from=") {
            f.forge_recv_from = Some(v.to_string());
        }
    }
    f
}

#[derive(Clone)]
struct AppState {
    me_name: String,
    me_id: NodeId,
    me: ClusterNode,
    cluster: ClusterConfig,

    me_sk: Arc<Mutex<SigningKey>>,
    me_vk: VerifyingKey,

    logger: Arc<Mutex<PrLogger>>,
    witnesses: Vec<Peer>,
    witness_store: Arc<Mutex<WitnessStore>>,
    fault: FaultConfig,

    events: Arc<Mutex<VecDeque<String>>>,

    recv_total: Arc<Mutex<u64>>,
    pr_commit_sent: Arc<Mutex<u64>>,
    pr_commit_recv: Arc<Mutex<u64>>,
}

impl AppState {
    async fn push_event(&self, s: impl Into<String>) {
        let mut ev = self.events.lock().await;
        ev.push_front(s.into());
        while ev.len() > 40 {
            ev.pop_back();
        }
    }

    fn vk_for_node_id(&self, node_id: NodeId) -> Option<VerifyingKey> {
        self.cluster
            .find_by_id(node_id)
            .map(|n| verifying_key_from_name(&n.name))
    }

    async fn log_kv(&self, kind: &str, payload: String) {
        let mut lg = self.logger.lock().await;
        let _ = lg.log_event(kind, None, payload);
    }
}

async fn tcp_connect(addr: &str) -> Result<TcpStream> {
    Ok(TcpStream::connect(addr)
        .await
        .with_context(|| format!("connect {addr}"))?)
}

fn bind_addr_from(_node_name: &str, addr: &str) -> Result<SocketAddr> {
    if let Some((_host, port)) = addr.rsplit_once(':') {
        let bind = format!("0.0.0.0:{port}");
        Ok(bind.parse().context("parse bind addr")?)
    } else {
        Ok(addr.parse().context("parse bind addr")?)
    }
}

async fn make_commitment_signed(
    state: &AppState,
    kind: &str,
    seq: u64,
    head_hash: [u8; 32],
) -> Commitment {
    let sk = state.me_sk.lock().await.clone();
    Commitment::new_signed(state.me_id, seq, head_hash, kind.to_string(), now_ms(), &sk)
}

async fn send_commitment_to_witnesses(state: &AppState, c: Commitment) {
    for w in &state.witnesses {
        let mut stream = match tcp_connect(&w.pr_addr).await {
            Ok(s) => s,
            Err(e) => {
                let _ = state
                    .push_event(format!("[PR] cannot connect witness {}: {e:#}", w.name))
                    .await;
                continue;
            }
        };
        let msg = PeerReviewMsg::Commitment(c.clone());
        if pr_send(&mut stream, &msg).await.is_ok() {
            *state.pr_commit_sent.lock().await += 1;
        }
    }
}

async fn pr_commitment_task(state: AppState) -> Result<()> {
    let mut tick = time::interval(Duration::from_secs(3));
    loop {
        tick.tick().await;

        let (seq, head_hash) = {
            let lg = state.logger.lock().await;
            (lg.seq(), lg.prev_hash32())
        };

        if state.fault.equivocate_head && !state.witnesses.is_empty() {
            state
                .push_event(format!(
                    "[PR][FAULT] equivocate_head enabled (seq={seq})"
                ))
                .await;

            for (idx, w) in state.witnesses.iter().enumerate() {
                let mut stream = match tcp_connect(&w.pr_addr).await {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let mut alt = head_hash;
                if idx % 2 == 1 {
                    let mut b = Vec::new();
                    b.extend_from_slice(&head_hash);
                    b.extend_from_slice(b"ALT");
                    alt = sha256_32(&b);
                }

                let c = make_commitment_signed(&state, "LOG_HEAD", seq, alt).await;
                let msg = PeerReviewMsg::Commitment(c.clone());
                if pr_send(&mut stream, &msg).await.is_ok() {
                    *state.pr_commit_sent.lock().await += 1;
                }
            }
            continue;
        }

        let c = make_commitment_signed(&state, "LOG_HEAD", seq, head_hash).await;
        send_commitment_to_witnesses(&state, c).await;
    }
}

async fn pr_server_task(state: AppState) -> Result<()> {
    let bind_sa = bind_addr_from(&state.me_name, &state.me.pr_addr)?;
    let listener = TcpListener::bind(bind_sa).await?;
    state
        .push_event(format!("[PR] listening on {}", state.me.pr_addr))
        .await;

    loop {
        let (mut sock, src) = listener.accept().await?;
        let st = state.clone();
        tokio::spawn(async move {
            let r = async {
                let msg = pr_recv(&mut sock).await?;
                match msg {
                    PeerReviewMsg::Ping => {
                        pr_send(&mut sock, &PeerReviewMsg::Pong).await?;
                    }

                    PeerReviewMsg::Commitment(c) => {
                        *st.pr_commit_recv.lock().await += 1;

                        if st.cluster.is_witness(&st.me_name) {
                            let vk = st
                                .vk_for_node_id(c.node_id)
                                .ok_or_else(|| anyhow::anyhow!("unknown node_id {}", c.node_id))?;

                            if c.verify(&vk).is_ok() {
                                let mut ws = st.witness_store.lock().await;
                                ws.commits.entry(c.node_id).or_default().push(c.clone());
                            }
                        }
                        pr_send(&mut sock, &PeerReviewMsg::Ack).await?;
                    }

                    PeerReviewMsg::Challenge {
                        node_id,
                        from_seq,
                        to_seq,
                        ..
                    } => {
                        let lines = {
                            let lg = st.logger.lock().await;
                            lg.read_log_window(from_seq, to_seq)?
                        };
                        pr_send(&mut sock, &PeerReviewMsg::LogSlice { node_id, lines }).await?;
                    }

                    PeerReviewMsg::WitnessQuery { node_id } => {
                        let commits = if st.cluster.is_witness(&st.me_name) {
                            let ws = st.witness_store.lock().await;
                            ws.commits.get(&node_id).cloned().unwrap_or_default()
                        } else {
                            vec![]
                        };
                        pr_send(&mut sock, &PeerReviewMsg::WitnessReply { node_id, commits })
                            .await?;
                    }

                    _ => {
                        pr_send(&mut sock, &PeerReviewMsg::Ack).await?;
                    }
                }
                Result::<()>::Ok(())
            }
            .await;

            if let Err(e) = r {
                let _ = st
                    .push_event(format!("[PR] handler error from {src}: {e:#}"))
                    .await;
            }
        });
    }
}

/// --------------------- APP SERVER (TCP) ---------------------
async fn app_server_task(state: AppState) -> Result<()> {
    let bind_sa = bind_addr_from(&state.me_name, &state.me.app_addr)?;
    let listener = TcpListener::bind(bind_sa).await?;
    state
        .push_event(format!("[APP] listening on {}", state.me.app_addr))
        .await;

    loop {
        let (mut sock, _) = listener.accept().await?;
        let st = state.clone();
        tokio::spawn(async move {
            let r = async {
                let msg = app_recv(&mut sock).await?;

                // Fault: drop/omit all messages from some sender
                if let Some(drop_from) = &st.fault.drop_from {
                    if &msg.from == drop_from {
                        st.push_event(format!(
                            "[APP][FAULT] drop_from={} dropped msg_id={}",
                            drop_from, msg.msg_id
                        ))
                        .await;

                        // log explicit DROP (optional)
                        st.log_kv(
                            "DROP",
                            format!(
                                "ts={} msg_id={} from={} to={}",
                                now_ms(),
                                msg.msg_id,
                                msg.from,
                                msg.to
                            ),
                        )
                        .await;

                        return Result::<()>::Ok(());
                    }
                }

                *st.recv_total.lock().await += 1;

                // Log RECV + DELIVER
                st.log_kv(
                    "RECV",
                    format!(
                        "ts={} msg_id={} from={} to={} text_hash={}",
                        msg.ts_ms,
                        msg.msg_id,
                        msg.from,
                        msg.to,
                        hex::encode(sha256_32(msg.text.as_bytes()))
                    ),
                )
                .await;

                st.log_kv(
                    "DELIVER",
                    format!(
                        "ts={} msg_id={} from={} to={}",
                        now_ms(),
                        msg.msg_id,
                        msg.from,
                        msg.to
                    ),
                )
                .await;

                st.push_event(format!("[APP] recv msg_id={} from={}", msg.msg_id, msg.from))
                    .await;

                Ok(())
            }
            .await;

            if let Err(e) = r {
                let _ = st
                    .push_event(format!("[APP] server handler error: {e:#}"))
                    .await;
            }
        });
    }
}

/// Fault: forge fake RECV without a matching SEND
async fn forge_recv_task(state: AppState) -> Result<()> {
    let mut tick = time::interval(Duration::from_secs(4));
    loop {
        tick.tick().await;
        let Some(from) = state.fault.forge_recv_from.clone() else { continue };

        let fake_id = format!("FAKE-{}-{}", state.me_name, now_ms());
        state
            .log_kv(
                "RECV",
                format!(
                    "ts={} msg_id={} from={} to={} text_hash={}",
                    now_ms(),
                    fake_id,
                    from,
                    state.me_name,
                    hex::encode([0u8; 32])
                ),
            )
            .await;

        state
            .push_event(format!(
                "[APP][FAULT] forged RECV msg_id={} from={}",
                fake_id, from
            ))
            .await;
    }
}

#[derive(Serialize)]
struct Stats {
    me: String,
    is_witness: bool,
    recv_total: u64,
    pr_commit_sent: u64,
    pr_commit_recv: u64,
    last_events: Vec<String>,
    fault: String,
}

async fn http_stats(State(st): State<AppState>) -> Json<Stats> {
    let ev = st.events.lock().await;
    let last_events = ev.iter().take(12).cloned().collect::<Vec<_>>();
    Json(Stats {
        me: st.me_name.clone(),
        is_witness: st.cluster.is_witness(&st.me_name),
        recv_total: *st.recv_total.lock().await,
        pr_commit_sent: *st.pr_commit_sent.lock().await,
        pr_commit_recv: *st.pr_commit_recv.lock().await,
        last_events,
        fault: format!(
            "equivocate_head={} drop_from={:?} forge_recv_from={:?}",
            st.fault.equivocate_head, st.fault.drop_from, st.fault.forge_recv_from
        ),
    })
}

#[derive(Deserialize)]
struct PublishTextReq {
    text: String,
}

#[derive(Serialize)]
struct PublishTextResp {
    ok: bool,
    msg_id: String,
}

async fn http_publish_text(
    State(st): State<AppState>,
    Json(req): Json<PublishTextReq>,
) -> Json<PublishTextResp> {
    let msg_id = format!("{}-txt-{}", st.me_name, now_ms());
    let ts = now_ms();
    let line = format!("APP_TEXT id={msg_id} ts={ts} {}", req.text);

    {
        let mut lg = st.logger.lock().await;
        let hash32 = sha256_32(line.as_bytes());
        let _ = lg.log_app("PUBLISH", None, msg_id.clone(), hash32, ts);
    }

    st.push_event(format!("[APP] publish_text id={msg_id}"))
        .await;

    let (seq, head_hash) = {
        let lg = st.logger.lock().await;
        (lg.seq(), lg.prev_hash32())
    };
    let c = make_commitment_signed(&st, "APP_EVENT", seq, head_hash).await;
    send_commitment_to_witnesses(&st, c).await;

    Json(PublishTextResp { ok: true, msg_id })
}

#[derive(Deserialize)]
struct SendTextReq {
    to: String,   // node name
    text: String,
}

#[derive(Serialize)]
struct SendTextResp {
    ok: bool,
    msg_id: String,
}

/// Generate a SEND, send over APP TCP to dest, and rely on dest to log RECV/DELIVER.
async fn http_send_text(
    State(st): State<AppState>,
    Json(req): Json<SendTextReq>,
) -> Json<SendTextResp> {
    let to_node = st
        .cluster
        .find_by_name(&req.to)
        .cloned()
        .unwrap_or_else(|| st.me.clone());

    let msg_id = format!("{}->{}-{}", st.me_name, req.to, now_ms());
    let ts = now_ms();

    // Log SEND (payload parseable)
    st.log_kv(
        "SEND",
        format!(
            "ts={} msg_id={} from={} to={} text_hash={}",
            ts,
            msg_id,
            st.me_name,
            req.to,
            hex::encode(sha256_32(req.text.as_bytes()))
        ),
    )
    .await;

    // Send to destination
    let app_msg = AppMsg {
        from: st.me_name.clone(),
        to: req.to.clone(),
        msg_id: msg_id.clone(),
        ts_ms: ts,
        text: req.text,
    };

    let mut sock = match tcp_connect(&to_node.app_addr).await {
        Ok(s) => s,
        Err(e) => {
            st.push_event(format!("[APP] send connect failed: {e:#}"))
                .await;
            return Json(SendTextResp { ok: false, msg_id });
        }
    };

    if let Err(e) = app_send(&mut sock, &app_msg).await {
        st.push_event(format!("[APP] send failed: {e:#}")).await;
        return Json(SendTextResp { ok: false, msg_id });
    }

    st.push_event(format!("[APP] sent msg_id={} to={}", msg_id, req.to))
        .await;

    // Send authenticator reflecting new head
    let (seq, head_hash) = {
        let lg = st.logger.lock().await;
        (lg.seq(), lg.prev_hash32())
    };
    let c = make_commitment_signed(&st, "APP_EVENT", seq, head_hash).await;
    send_commitment_to_witnesses(&st, c).await;

    Json(SendTextResp { ok: true, msg_id })
}

#[derive(Deserialize)]
struct AskWitnessReq {
    node_name: String,
    witness_name: String,
}

async fn http_pr_ask_witness(
    State(st): State<AppState>,
    Json(req): Json<AskWitnessReq>,
) -> Json<serde_json::Value> {
    let node_id = node_id_from_name(&req.node_name);
    let w = match st.cluster.find_by_name(&req.witness_name) {
        Some(n) => n,
        None => return Json(serde_json::json!({"ok":false,"error":"witness not found"})),
    };

    let mut stream = match tcp_connect(&w.pr_addr).await {
        Ok(s) => s,
        Err(e) => return Json(serde_json::json!({"ok":false,"error":format!("{e:#}")})),
    };

    let q = PeerReviewMsg::WitnessQuery { node_id };
    if let Err(e) = pr_send(&mut stream, &q).await {
        return Json(serde_json::json!({"ok":false,"error":format!("{e:#}")}));
    }

    let resp = match pr_recv(&mut stream).await {
        Ok(r) => r,
        Err(e) => return Json(serde_json::json!({"ok":false,"error":format!("{e:#}")})),
    };

    match resp {
        PeerReviewMsg::WitnessReply { commits, .. } => {
            Json(serde_json::json!({"ok":true,"count":commits.len(),"commits":commits}))
        }
        _ => Json(serde_json::json!({"ok":false,"error":"unexpected reply"})),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    #[derive(clap::Parser, Debug)]
    #[command(author, version, about)]
    struct Args {
        #[arg(long)]
        name: String,

        #[arg(long, default_value = "configs/docker/cluster.yaml")]
        cluster: PathBuf,

        #[arg(long, default_value = "peerreview_logs")]
        log_dir: PathBuf,

        #[arg(long)]
        http: bool,

        /// fault string: "equivocate_head" or "drop_from=node4" or "forge_recv_from=node4" or combo with commas
        #[arg(long, default_value = "")]
        fault: String,
    }

    let args = Args::parse();

    let cluster = ClusterConfig::from_yaml_file(&args.cluster)
        .with_context(|| format!("ClusterConfig::from_yaml_file ({:?})", args.cluster))?;

    let me = cluster
        .find_by_name(&args.name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("node not found in cluster config"))?;

    let me_id = me.id;

    let me_sk_raw = signing_key_from_name(&args.name);
    let me_vk = me_sk_raw.verifying_key();
    let me_sk = Arc::new(Mutex::new(me_sk_raw));

    let log_path = args.log_dir.join(&args.name);
    tokio::fs::create_dir_all(&log_path).await.ok();

    let pr_logger = {
        let sk = me_sk.lock().await.clone();
        PrLogger::open(me_id, &log_path, sk).context("Logger::open failed")?
    };

    let witnesses = cluster
        .witnesses
        .iter()
        .filter_map(|wname| cluster.find_by_name(wname).cloned())
        .filter(|n| n.name != args.name)
        .map(|n| Peer {
            id: n.id,
            name: n.name.clone(),
            pr_addr: n.pr_addr.clone(),
        })
        .collect::<Vec<_>>();

    let fault = parse_fault(&args.fault);

    let state = AppState {
        me_name: args.name.clone(),
        me_id,
        me,
        cluster,
        me_sk,
        me_vk,
        logger: Arc::new(Mutex::new(pr_logger)),
        witnesses,
        witness_store: Arc::new(Mutex::new(WitnessStore::default())),
        fault: fault.clone(),
        events: Arc::new(Mutex::new(VecDeque::new())),
        recv_total: Arc::new(Mutex::new(0)),
        pr_commit_sent: Arc::new(Mutex::new(0)),
        pr_commit_recv: Arc::new(Mutex::new(0)),
    };

    state
        .push_event(format!(
            "[BOOT] me={} id={} witness={} fault={}",
            state.me_name,
            state.me_id,
            state.cluster.is_witness(&state.me_name),
            args.fault
        ))
        .await;

    tokio::spawn(pr_server_task(state.clone()));
    tokio::spawn(pr_commitment_task(state.clone()));
    tokio::spawn(app_server_task(state.clone()));
    tokio::spawn(forge_recv_task(state.clone()));

    if args.http {
        let http_addr = state
            .me
            .http_addr
            .clone()
            .context("http_addr missing in cluster config for this node")?;
        let bind_sa: SocketAddr = http_addr.parse().context("parse http_addr")?;

        let app = Router::new()
            .route("/stats", get(http_stats))
            .route("/publish_text", post(http_publish_text))
            .route("/send_text", post(http_send_text))
            .route("/pr/ask_witness", post(http_pr_ask_witness))
            .with_state(state.clone());

        println!("[http] listening on http://{bind_sa}");
        axum::serve(TcpListener::bind(bind_sa).await?, app).await?;
    } else {
        loop {
            time::sleep(Duration::from_secs(3600)).await;
        }
    }

    Ok(())
}
