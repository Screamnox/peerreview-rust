/// Gestion de la configuration PeerReview via fichier TOML
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use super::node::NodeId;

/// Valeurs par défaut
fn default_log_max_lines() -> usize {
    1000
}
fn default_log_min_line_size() -> usize {
    256
}
fn default_state_file() -> String {
    "node_state.json".to_string()
}
fn default_connection_timeout() -> u64 {
    10
}
fn default_ack_timeout() -> u64 {
    5
}
fn default_audit_interval() -> u64 {
    30
}
fn default_consistency_interval() -> u64 {
    60
}
fn default_evidence_interval() -> u64 {
    120
}

/// Configuration d'un noeud PeerReview
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub node: NodeConfig,
    pub network: NetworkConfig,
    pub timers: TimersConfig,
    pub witnesses: WitnessesConfig,
    pub watched: WatchedConfig,
}

/// Configuration spécifique au noeud
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: NodeId,

    pub log_file: String,

    #[serde(default = "default_log_max_lines")]
    pub log_max_lines: usize,

    #[serde(default = "default_log_min_line_size")]
    pub log_min_line_size: usize,

    pub keypair_file: String,

    #[serde(default = "default_state_file")]
    pub state_file: String,
}

/// Configuration réseau
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_address: String,

    pub peers_file: String,

    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,

    #[serde(default = "default_ack_timeout")]
    pub ack_timeout_secs: u64,
}

/// Configuration des timers périodiques
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimersConfig {
    #[serde(default = "default_audit_interval")]
    pub audit_interval_secs: u64,

    #[serde(default = "default_consistency_interval")]
    pub consistency_check_secs: u64,

    #[serde(default = "default_evidence_interval")]
    pub evidence_transfer_secs: u64,
}

/// Configuration des témoins
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessesConfig {
    /// Liste des IDs témoins existant dans peers.toml
    pub list: Vec<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchedConfig {
    /// Liste des IDs des noeuds observés existant dans peers.toml
    pub list: Vec<NodeId>,
}

/// Entrée dans le fichier peers.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfigEntry {
    /// ID du pair
    pub id: NodeId,

    /// Adresse TCP (ex: "127.0.0.1:5042")
    pub address: String,

    /// Clé publique encodée en base64
    pub public_key: String,

    /// Liste des IDs témoins du pair
    pub witnesses: Vec<NodeId>,
}

/// Configuration des pairs (peers.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeersConfig {
    pub peers: Vec<PeerConfigEntry>,
}

impl Config {
    /// Charge la configuration depuis un fichier TOML
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Erreur de lecture du fichier de config: {}", e))?;

        toml::from_str(&content).map_err(|e| format!("Erreur de parsing TOML (config): {}", e))
    }

    /// Sauvegarde la configuration dans un fichier TOML
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Erreur de sérialisation TOML (config): {}", e))?;

        fs::write(path, content).map_err(|e| format!("Erreur d'écriture du fichier config: {}", e))
    }

    /// Crée une configuration par défaut pour un nœud donné
    pub fn default_for_node(node_id: NodeId, port: u16) -> Self {
        Self {
            node: NodeConfig {
                id: node_id,
                log_file: format!("node{}.log", node_id),
                log_max_lines: default_log_max_lines(),
                log_min_line_size: default_log_min_line_size(),
                keypair_file: format!("node{}.key", node_id),
                state_file: format!("node{}_state.json", node_id),
            },
            network: NetworkConfig {
                listen_address: format!("0.0.0.0:{}", port),
                peers_file: "peers.toml".to_string(),
                connection_timeout_secs: default_connection_timeout(),
                ack_timeout_secs: default_ack_timeout(),
            },
            timers: TimersConfig {
                audit_interval_secs: default_audit_interval(),
                consistency_check_secs: default_consistency_interval(),
                evidence_transfer_secs: default_evidence_interval(),
            },
            witnesses: WitnessesConfig {
                list: Vec::new(), // À remplir manuellement
            },
            watched: WatchedConfig { list: Vec::new() },
        }
    }
}

impl PeersConfig {
    /// Charge la configuration des pairs depuis un fichier TOML
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Erreur de lecture du fichier peers: {}", e))?;

        toml::from_str(&content).map_err(|e| format!("Erreur de parsing TOML (peers): {}", e))
    }

    /// Sauvegarde la configuration des pairs dans un fichier TOML
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Erreur de sérialisation TOML (peers): {}", e))?;

        fs::write(path, content).map_err(|e| format!("Erreur d'écriture du fichier peers: {}", e))
    }
}
