//! Client NFS simple pour PeerReview
//!
//! Implémente la "machine à états triviale" décrite dans le papier PeerReview Section 6.3.1:
//! "La machine à états implémentée par les clients est triviale :
//!  elle accepte simplement les requêtes du noyau et les convertit en RPC,
//!  qu'elle envoie ensuite au serveur."
//!
//! Le client NFS:
//! - Accepte des commandes (read, write, delete, list)
//! - Les convertit en requêtes NFS RPC
//! - Les envoie au serveur via TCP
//! - Reçoit et affiche les réponses

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use common_proto::{NFSOperation, NFSReply, NFSRequest, NFSResponse, NodeId};
use deterministic_fs::DeterministicClock;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

// ----------------- CLI Arguments -----------------

#[derive(Parser, Debug)]
#[command(author, version, about = "NFS Client - Client de fichiers réseau")]
struct Args {
    /// Identifiant du client
    #[arg(short, long)]
    id: String,

    /// Adresse du serveur NFS (ex: localhost:9001)
    #[arg(short, long)]
    server: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Lire un fichier
    Read {
        /// Chemin du fichier
        #[arg(short, long)]
        path: String,

        /// Offset de lecture
        #[arg(short, long, default_value = "0")]
        offset: u64,

        /// Nombre d'octets à lire
        #[arg(short, long, default_value = "4096")]
        length: u64,
    },

    /// Écrire dans un fichier
    Write {
        /// Chemin du fichier
        #[arg(short, long)]
        path: String,

        /// Données à écrire
        #[arg(short, long)]
        data: String,

        /// Offset d'écriture
        #[arg(short, long, default_value = "0")]
        offset: u64,
    },

    /// Supprimer un fichier
    Delete {
        /// Chemin du fichier
        #[arg(short, long)]
        path: String,
    },

    /// Lister un répertoire
    List {
        /// Chemin du répertoire
        #[arg(short, long, default_value = "/")]
        path: String,
    },

    /// Mode interactif
    Interactive,
}

// ----------------- Client NFS -----------------

/// Client NFS avec machine à états triviale
///
/// Conforme à la Section 6.3.1 du papier PeerReview:
/// "La machine à états implémentée par les clients est triviale"
pub struct NFSClient {
    /// Identifiant du client
    id: NodeId,
    /// Adresse du serveur
    server_addr: SocketAddr,
    /// Horloge déterministe pour synchronisation avec le serveur
    clock: Arc<DeterministicClock>,
}

impl NFSClient {
    /// Crée un nouveau client NFS
    pub fn new(id: NodeId, server_addr: SocketAddr) -> Self {
        Self {
            id,
            server_addr,
            clock: Arc::new(DeterministicClock::new()),
        }
    }

    /// Envoie une requête RPC au serveur
    ///
    /// Machine à états triviale:
    /// 1. Créer la requête RPC
    /// 2. Se connecter au serveur
    /// 3. Envoyer la requête (bincode)
    /// 4. Recevoir la réponse
    /// 5. Mettre à jour l'horloge
    async fn send_rpc(&mut self, operation: NFSOperation) -> Result<NFSResponse> {
        // Étape 1: Créer la requête RPC
        let request = NFSRequest {
            rpc_id: Uuid::new_v4().to_string(),
            from: self.id.clone(),
            operation: operation.clone(),
            timestamp: self.clock.now(),
        };

        println!(
            "[Client {}] Envoi RPC: {:?} (timestamp={})",
            self.id,
            operation,
            request.timestamp
        );

        // Étape 2: Se connecter au serveur
        let mut stream = TcpStream::connect(&self.server_addr)
            .await
            .with_context(|| format!("Failed to connect to server {}", self.server_addr))?;

        println!("[Client {}] Connecté au serveur {}", self.id, self.server_addr);

        // Étape 3: Envoyer la requête (format: length + message)
        let msg_bytes = bincode::serialize(&request).context("Failed to serialize request")?;
        let len_bytes = (msg_bytes.len() as u32).to_be_bytes();

        stream
            .write_all(&len_bytes)
            .await
            .context("Failed to send message length")?;
        stream
            .write_all(&msg_bytes)
            .await
            .context("Failed to send message")?;

        println!("[Client {}] Requête envoyée ({} bytes)", self.id, msg_bytes.len());

        // Étape 4: Recevoir la réponse
        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .await
            .context("Failed to read response length")?;
        let reply_len = u32::from_be_bytes(len_buf) as usize;

        let mut reply_buf = vec![0u8; reply_len];
        stream
            .read_exact(&mut reply_buf)
            .await
            .context("Failed to read response")?;

        let reply: NFSReply =
            bincode::deserialize(&reply_buf).context("Failed to deserialize response")?;

        println!(
            "[Client {}] Réponse reçue de {} (timestamp={})",
            self.id, reply.from, reply.timestamp
        );

        // Étape 5: Mettre à jour l'horloge (synchronisation Lamport)
        self.clock.update_time(reply.timestamp);

        Ok(reply.response)
    }

    /// Lit un fichier
    pub async fn read(&mut self, path: &str, offset: u64, length: u64) -> Result<Vec<u8>> {
        let operation = NFSOperation::Read {
            file_path: path.to_string(),
            offset,
            length,
        };

        match self.send_rpc(operation).await? {
            NFSResponse::ReadOk { data } => {
                println!("[Client {}] ✅ Lecture réussie: {} bytes", self.id, data.len());
                Ok(data)
            }
            NFSResponse::Error { message } => {
                println!("[Client {}] ❌ Erreur: {}", self.id, message);
                anyhow::bail!(message)
            }
            _ => anyhow::bail!("Unexpected response type"),
        }
    }

    /// Écrit dans un fichier
    pub async fn write(&mut self, path: &str, offset: u64, data: Vec<u8>) -> Result<u64> {
        let operation = NFSOperation::Write {
            file_path: path.to_string(),
            offset,
            data,
        };

        match self.send_rpc(operation).await? {
            NFSResponse::WriteOk { bytes_written } => {
                println!(
                    "[Client {}] ✅ Écriture réussie: {} bytes",
                    self.id, bytes_written
                );
                Ok(bytes_written)
            }
            NFSResponse::Error { message } => {
                println!("[Client {}] ❌ Erreur: {}", self.id, message);
                anyhow::bail!(message)
            }
            _ => anyhow::bail!("Unexpected response type"),
        }
    }

    /// Supprime un fichier
    pub async fn delete(&mut self, path: &str) -> Result<()> {
        let operation = NFSOperation::Delete {
            file_path: path.to_string(),
        };

        match self.send_rpc(operation).await? {
            NFSResponse::DeleteOk => {
                println!("[Client {}] ✅ Suppression réussie", self.id);
                Ok(())
            }
            NFSResponse::Error { message } => {
                println!("[Client {}] ❌ Erreur: {}", self.id, message);
                anyhow::bail!(message)
            }
            _ => anyhow::bail!("Unexpected response type"),
        }
    }

    /// Liste un répertoire
    pub async fn list(&mut self, path: &str) -> Result<Vec<String>> {
        let operation = NFSOperation::List {
            dir_path: path.to_string(),
        };

        match self.send_rpc(operation).await? {
            NFSResponse::ListOk { entries } => {
                println!(
                    "[Client {}] ✅ Listage réussi: {} entrées",
                    self.id,
                    entries.len()
                );
                Ok(entries)
            }
            NFSResponse::Error { message } => {
                println!("[Client {}] ❌ Erreur: {}", self.id, message);
                anyhow::bail!(message)
            }
            _ => anyhow::bail!("Unexpected response type"),
        }
    }
}

// ----------------- Mode Interactif -----------------

async fn interactive_mode(mut client: NFSClient) -> Result<()> {
    use std::io::{self, Write as IoWrite};

    println!("\n=== Mode Interactif NFS Client ===");
    println!("Commandes disponibles:");
    println!("  read <path> [offset] [length]");
    println!("  write <path> <data> [offset]");
    println!("  delete <path>");
    println!("  list [path]");
    println!("  quit");
    println!();

    loop {
        print!("nfs> ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;

        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "read" => {
                if parts.len() < 2 {
                    println!("Usage: read <path> [offset] [length]");
                    continue;
                }
                let path = parts[1];
                let offset = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                let length = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(4096);

                match client.read(path, offset, length).await {
                    Ok(data) => {
                        println!("Données lues ({} bytes):", data.len());
                        println!("{}", String::from_utf8_lossy(&data));
                    }
                    Err(e) => println!("Erreur: {}", e),
                }
            }

            "write" => {
                if parts.len() < 3 {
                    println!("Usage: write <path> <data> [offset]");
                    continue;
                }
                let path = parts[1];
                let data = parts[2].as_bytes().to_vec();
                let offset = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

                match client.write(path, offset, data).await {
                    Ok(bytes) => println!("Écrit {} bytes", bytes),
                    Err(e) => println!("Erreur: {}", e),
                }
            }

            "delete" => {
                if parts.len() < 2 {
                    println!("Usage: delete <path>");
                    continue;
                }
                let path = parts[1];

                match client.delete(path).await {
                    Ok(()) => println!("Fichier supprimé"),
                    Err(e) => println!("Erreur: {}", e),
                }
            }

            "list" => {
                let path = parts.get(1).copied().unwrap_or("/");

                match client.list(path).await {
                    Ok(entries) => {
                        println!("Entrées dans {}:", path);
                        for entry in entries {
                            println!("  - {}", entry);
                        }
                    }
                    Err(e) => println!("Erreur: {}", e),
                }
            }

            "quit" | "exit" => {
                println!("Au revoir!");
                break;
            }

            _ => {
                println!("Commande inconnue: {}", parts[0]);
            }
        }
    }

    Ok(())
}

// ----------------- Main -----------------

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let server_addr: SocketAddr = args
        .server
        .parse()
        .with_context(|| format!("Invalid server address: {}", args.server))?;

    let mut client = NFSClient::new(args.id.clone(), server_addr);

    println!("NFS Client démarré: {}", args.id);
    println!("Serveur: {}", server_addr);
    println!();

    match args.command {
        Command::Read { path, offset, length } => {
            let data = client.read(&path, offset, length).await?;
            println!("\n=== Contenu du fichier ===");
            println!("{}", String::from_utf8_lossy(&data));
        }

        Command::Write { path, data, offset } => {
            let bytes = client.write(&path, offset, data.into_bytes()).await?;
            println!("\n=== Résultat ===");
            println!("Écrit {} bytes dans {}", bytes, path);
        }

        Command::Delete { path } => {
            client.delete(&path).await?;
            println!("\n=== Résultat ===");
            println!("Fichier {} supprimé", path);
        }

        Command::List { path } => {
            let entries = client.list(&path).await?;
            println!("\n=== Contenu de {} ===", path);
            for entry in entries {
                println!("  {}", entry);
            }
        }

        Command::Interactive => {
            interactive_mode(client).await?;
        }
    }

    Ok(())
}
