//! Application NFS simplifiée pour PeerReview
//!
//! Serveur de fichiers réseau avec opérations READ, WRITE, DELETE, LIST.

use anyhow::Result;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use common_proto::{
    NFSClusterConfig, NFSOperation, NFSReply, NFSRequest, NFSResponse, NFSServerConfig,
    NFSServerStats, NodeId,
};
use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::{Read as IoRead, Seek, SeekFrom, Write as IoWrite};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ----------------- CLI Arguments -----------------
#[derive(Parser, Debug)]
#[command(author, version, about = "NFS Node - Serveur de fichiers réseau")]
struct Args {
    /// Fichier de configuration du noeud
    #[arg(short, long)]
    config: String,

    /// Fichier de configuration du cluster
    #[arg(short = 'l', long)]
    cluster: String,
}

// ----------------- Horloge Déterministe -----------------
#[derive(Debug)]
pub struct DeterministicClock {
    current_time: AtomicU64,
}

impl DeterministicClock {
    pub fn new() -> Self {
        Self {
            current_time: AtomicU64::new(0),
        }
    }

    pub fn now(&self) -> u64 {
        self.current_time.load(Ordering::SeqCst)
    }

    pub fn set_time(&self, time: u64) {
        self.current_time.store(time, Ordering::SeqCst);
    }

    /// Met à jour le temps si le nouveau temps est plus grand
    pub fn update_time(&self, time: u64) {
        self.current_time.fetch_max(time, Ordering::SeqCst);
    }
}

impl Default for DeterministicClock {
    fn default() -> Self {
        Self::new()
    }
}

// ----------------- État du Serveur NFS -----------------
#[derive(Clone)]
struct ServerState {
    /// Identifiant du serveur
    my_id: NodeId,
    /// Racine du volume exporté
    volume_root: PathBuf,
    /// Horloge déterministe
    clock: Arc<DeterministicClock>,
    /// Compteur d'opérations
    operations_count: Arc<AtomicU64>,
    /// Historique des opérations (pour affichage)
    operation_history: Arc<Mutex<VecDeque<String>>>,
    /// Verrous par fichier pour sérialisation des opérations
    file_locks: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl ServerState {
    fn new(config: &NFSServerConfig) -> Result<Self> {
        let volume_root = PathBuf::from(&config.volume_path);

        // Créer le répertoire du volume s'il n'existe pas
        fs::create_dir_all(&volume_root)?;

        Ok(Self {
            my_id: config.id.clone(),
            volume_root,
            clock: Arc::new(DeterministicClock::new()),
            operations_count: Arc::new(AtomicU64::new(0)),
            operation_history: Arc::new(Mutex::new(VecDeque::with_capacity(100))),
            file_locks: Arc::new(DashMap::new()),
        })
    }

    /// Obtenir le verrou pour un fichier (sérialisation des opérations)
    fn get_file_lock(&self, path: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.file_locks
            .entry(path.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Résoudre un chemin relatif vers le chemin absolu dans le volume
    fn resolve_path(&self, relative_path: &str) -> PathBuf {
        // Nettoyer le chemin pour éviter les traversées de répertoire
        let clean_path = relative_path
            .trim_start_matches('/')
            .replace("..", "");
        self.volume_root.join(clean_path)
    }

    /// Exécuter une opération NFS
    async fn execute_operation(&self, operation: &NFSOperation) -> NFSResponse {
        match operation {
            NFSOperation::Read {
                file_path,
                offset,
                length,
            } => self.do_read(file_path, *offset, *length).await,

            NFSOperation::Write {
                file_path,
                offset,
                data,
            } => self.do_write(file_path, *offset, data).await,

            NFSOperation::Delete { file_path } => self.do_delete(file_path).await,

            NFSOperation::List { dir_path } => self.do_list(dir_path).await,
        }
    }

    /// Opération READ
    async fn do_read(&self, file_path: &str, offset: u64, length: u64) -> NFSResponse {
        let path = self.resolve_path(file_path);
        let lock = self.get_file_lock(file_path);
        let _guard = lock.lock().await;

        match fs::File::open(&path) {
            Ok(mut file) => {
                if let Err(e) = file.seek(SeekFrom::Start(offset)) {
                    return NFSResponse::Error {
                        message: format!("Seek error: {}", e),
                    };
                }

                let mut buffer = vec![0u8; length as usize];
                match file.read(&mut buffer) {
                    Ok(bytes_read) => {
                        buffer.truncate(bytes_read);
                        NFSResponse::ReadOk { data: buffer }
                    }
                    Err(e) => NFSResponse::Error {
                        message: format!("Read error: {}", e),
                    },
                }
            }
            Err(e) => NFSResponse::Error {
                message: format!("Open error: {}", e),
            },
        }
    }

    /// Opération WRITE
    async fn do_write(&self, file_path: &str, offset: u64, data: &[u8]) -> NFSResponse {
        let path = self.resolve_path(file_path);
        let lock = self.get_file_lock(file_path);
        let _guard = lock.lock().await;

        // Créer les répertoires parents si nécessaire
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return NFSResponse::Error {
                    message: format!("Create dir error: {}", e),
                };
            }
        }

        // Ouvrir ou créer le fichier
        let file_result = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path);

        match file_result {
            Ok(mut file) => {
                if let Err(e) = file.seek(SeekFrom::Start(offset)) {
                    return NFSResponse::Error {
                        message: format!("Seek error: {}", e),
                    };
                }

                match file.write(data) {
                    Ok(bytes_written) => NFSResponse::WriteOk {
                        bytes_written: bytes_written as u64,
                    },
                    Err(e) => NFSResponse::Error {
                        message: format!("Write error: {}", e),
                    },
                }
            }
            Err(e) => NFSResponse::Error {
                message: format!("Open error: {}", e),
            },
        }
    }

    /// Opération DELETE
    async fn do_delete(&self, file_path: &str) -> NFSResponse {
        let path = self.resolve_path(file_path);
        let lock = self.get_file_lock(file_path);
        let _guard = lock.lock().await;

        match fs::remove_file(&path) {
            Ok(()) => NFSResponse::DeleteOk,
            Err(e) => NFSResponse::Error {
                message: format!("Delete error: {}", e),
            },
        }
    }

    /// Opération LIST
    async fn do_list(&self, dir_path: &str) -> NFSResponse {
        let path = self.resolve_path(dir_path);
        let lock = self.get_file_lock(dir_path);
        let _guard = lock.lock().await;

        match fs::read_dir(&path) {
            Ok(entries) => {
                let mut names: Vec<String> = Vec::new();
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        names.push(name.to_string());
                    }
                }
                names.sort();
                NFSResponse::ListOk { entries: names }
            }
            Err(e) => NFSResponse::Error {
                message: format!("List error: {}", e),
            },
        }
    }

    /// Enregistrer une opération dans l'historique
    fn log_operation(&self, operation: &NFSOperation, response: &NFSResponse) {
        let op_str = match operation {
            NFSOperation::Read { file_path, .. } => format!("READ {}", file_path),
            NFSOperation::Write { file_path, .. } => format!("WRITE {}", file_path),
            NFSOperation::Delete { file_path } => format!("DELETE {}", file_path),
            NFSOperation::List { dir_path } => format!("LIST {}", dir_path),
        };

        let result_str = match response {
            NFSResponse::ReadOk { data } => format!("OK ({} bytes)", data.len()),
            NFSResponse::WriteOk { bytes_written } => format!("OK ({} bytes)", bytes_written),
            NFSResponse::DeleteOk => "OK".to_string(),
            NFSResponse::ListOk { entries } => format!("OK ({} entries)", entries.len()),
            NFSResponse::Error { message } => format!("ERROR: {}", message),
        };

        let log_entry = format!("{} -> {}", op_str, result_str);

        let mut history = self.operation_history.lock();
        if history.len() >= 100 {
            history.pop_front();
        }
        history.push_back(log_entry);

        self.operations_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Calculer les statistiques du volume
    fn compute_stats(&self) -> NFSServerStats {
        let mut volume_size = 0u64;
        let mut file_count = 0usize;

        if let Ok(entries) = fs::read_dir(&self.volume_root) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        volume_size += metadata.len();
                        file_count += 1;
                    }
                }
            }
        }

        let last_operations: Vec<String> = {
            let history = self.operation_history.lock();
            history.iter().rev().take(10).cloned().collect()
        };

        NFSServerStats {
            node_id: self.my_id.clone(),
            operations_count: self.operations_count.load(Ordering::Relaxed) as usize,
            last_operations,
            volume_size,
            file_count,
        }
    }
}

// ----------------- HTTP API Handlers -----------------

/// Requête HTTP pour READ
#[derive(Deserialize)]
struct HttpReadRequest {
    path: String,
    offset: Option<u64>,
    length: Option<u64>,
}

/// Requête HTTP pour WRITE
#[derive(Deserialize)]
struct HttpWriteRequest {
    path: String,
    offset: Option<u64>,
    data: String, // Base64 ou texte brut
}

/// Requête HTTP pour DELETE
#[derive(Deserialize)]
struct HttpDeleteRequest {
    path: String,
}

/// Requête HTTP pour LIST
#[derive(Deserialize)]
struct HttpListRequest {
    path: String,
}

/// Réponse HTTP générique
#[derive(Serialize)]
struct HttpResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_written: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entries: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn http_read(
    State(state): State<ServerState>,
    Json(req): Json<HttpReadRequest>,
) -> Json<HttpResponse> {
    let operation = NFSOperation::Read {
        file_path: req.path,
        offset: req.offset.unwrap_or(0),
        length: req.length.unwrap_or(4096),
    };

    let response = state.execute_operation(&operation).await;
    state.log_operation(&operation, &response);

    match response {
        NFSResponse::ReadOk { data } => Json(HttpResponse {
            status: "ReadOk".to_string(),
            data: Some(String::from_utf8_lossy(&data).to_string()),
            bytes_written: None,
            entries: None,
            error: None,
        }),
        NFSResponse::Error { message } => Json(HttpResponse {
            status: "Error".to_string(),
            data: None,
            bytes_written: None,
            entries: None,
            error: Some(message),
        }),
        _ => unreachable!(),
    }
}

async fn http_write(
    State(state): State<ServerState>,
    Json(req): Json<HttpWriteRequest>,
) -> Json<HttpResponse> {
    let operation = NFSOperation::Write {
        file_path: req.path,
        offset: req.offset.unwrap_or(0),
        data: req.data.into_bytes(),
    };

    let response = state.execute_operation(&operation).await;
    state.log_operation(&operation, &response);

    match response {
        NFSResponse::WriteOk { bytes_written } => Json(HttpResponse {
            status: "WriteOk".to_string(),
            data: None,
            bytes_written: Some(bytes_written),
            entries: None,
            error: None,
        }),
        NFSResponse::Error { message } => Json(HttpResponse {
            status: "Error".to_string(),
            data: None,
            bytes_written: None,
            entries: None,
            error: Some(message),
        }),
        _ => unreachable!(),
    }
}

async fn http_delete(
    State(state): State<ServerState>,
    Json(req): Json<HttpDeleteRequest>,
) -> Json<HttpResponse> {
    let operation = NFSOperation::Delete {
        file_path: req.path,
    };

    let response = state.execute_operation(&operation).await;
    state.log_operation(&operation, &response);

    match response {
        NFSResponse::DeleteOk => Json(HttpResponse {
            status: "DeleteOk".to_string(),
            data: None,
            bytes_written: None,
            entries: None,
            error: None,
        }),
        NFSResponse::Error { message } => Json(HttpResponse {
            status: "Error".to_string(),
            data: None,
            bytes_written: None,
            entries: None,
            error: Some(message),
        }),
        _ => unreachable!(),
    }
}

async fn http_list(
    State(state): State<ServerState>,
    Json(req): Json<HttpListRequest>,
) -> Json<HttpResponse> {
    let operation = NFSOperation::List {
        dir_path: req.path,
    };

    let response = state.execute_operation(&operation).await;
    state.log_operation(&operation, &response);

    match response {
        NFSResponse::ListOk { entries } => Json(HttpResponse {
            status: "ListOk".to_string(),
            data: None,
            bytes_written: None,
            entries: Some(entries),
            error: None,
        }),
        NFSResponse::Error { message } => Json(HttpResponse {
            status: "Error".to_string(),
            data: None,
            bytes_written: None,
            entries: None,
            error: Some(message),
        }),
        _ => unreachable!(),
    }
}

async fn http_stats(State(state): State<ServerState>) -> Json<NFSServerStats> {
    Json(state.compute_stats())
}

// ----------------- TCP Server -----------------

/// Traiter une connexion TCP entrante
async fn handle_tcp_connection(mut stream: TcpStream, state: ServerState) {
    let peer_addr = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    loop {
        // Lire la longueur du message (4 bytes big-endian)
        let mut len_buf = [0u8; 4];
        if stream.read_exact(&mut len_buf).await.is_err() {
            break;
        }
        let msg_len = u32::from_be_bytes(len_buf) as usize;

        // Lire le message
        let mut msg_buf = vec![0u8; msg_len];
        if stream.read_exact(&mut msg_buf).await.is_err() {
            break;
        }

        // Désérialiser la requête
        let request: NFSRequest = match bincode::deserialize(&msg_buf) {
            Ok(req) => req,
            Err(e) => {
                eprintln!("{} deserialize error from {}: {}", state.my_id, peer_addr, e);
                continue;
            }
        };

        // Mettre à jour l'horloge avec le timestamp de la requête
        state.clock.update_time(request.timestamp);

        // Exécuter l'opération
        let response = state.execute_operation(&request.operation).await;
        state.log_operation(&request.operation, &response);

        // Construire la réponse
        let reply = NFSReply {
            rpc_id: request.rpc_id,
            from: state.my_id.clone(),
            response,
            timestamp: state.clock.now(),
        };

        // Sérialiser et envoyer
        let reply_bytes = bincode::serialize(&reply).expect("serialize reply");
        let len_bytes = (reply_bytes.len() as u32).to_be_bytes();

        if stream.write_all(&len_bytes).await.is_err() {
            break;
        }
        if stream.write_all(&reply_bytes).await.is_err() {
            break;
        }
    }
}

// ----------------- Main -----------------

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Charger les configurations
    let node_cfg: NFSServerConfig =
        serde_yaml::from_str(&fs::read_to_string(&args.config)?)?;
    let _cluster_cfg: NFSClusterConfig =
        serde_yaml::from_str(&fs::read_to_string(&args.cluster)?)?;

    println!("Starting NFS server: {}", node_cfg.id);
    println!("Volume path: {}", node_cfg.volume_path);

    // Créer l'état du serveur
    let state = ServerState::new(&node_cfg)?;

    // Démarrer le serveur TCP
    let listen_addr: SocketAddr = node_cfg.listen_addr.parse()?;
    let tcp_listener = TcpListener::bind(listen_addr).await?;
    println!("{} listening on TCP {}", node_cfg.id, listen_addr);

    let tcp_state = state.clone();
    tokio::spawn(async move {
        loop {
            match tcp_listener.accept().await {
                Ok((stream, addr)) => {
                    println!("{} accepted TCP connection from {}", tcp_state.my_id, addr);
                    let conn_state = tcp_state.clone();
                    tokio::spawn(handle_tcp_connection(stream, conn_state));
                }
                Err(e) => {
                    eprintln!("TCP accept error: {}", e);
                }
            }
        }
    });

    // Démarrer l'API HTTP
    let http_addr = SocketAddr::from(([0, 0, 0, 0], node_cfg.http_api));
    let app = Router::new()
        .route("/read", post(http_read))
        .route("/write", post(http_write))
        .route("/delete", post(http_delete))
        .route("/list", post(http_list))
        .route("/stats", get(http_stats))
        .with_state(state);

    println!("{} HTTP API on {}", node_cfg.id, http_addr);

    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
