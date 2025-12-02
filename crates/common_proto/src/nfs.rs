//! Messages et structures pour l'application NFS
//!
//! Définit les opérations, réponses et structures RPC pour le système de fichiers réseau.

use serde::{Deserialize, Serialize};

/// Identifiant d'un noeud
pub type NodeId = String;

/// Opérations NFS supportées
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NFSOperation {
    /// Lire des données d'un fichier
    Read {
        file_path: String,
        offset: u64,
        length: u64,
    },
    /// Écrire des données dans un fichier
    Write {
        file_path: String,
        offset: u64,
        data: Vec<u8>,
    },
    /// Supprimer un fichier
    Delete {
        file_path: String,
    },
    /// Lister le contenu d'un répertoire
    List {
        dir_path: String,
    },
}

/// Réponses aux opérations NFS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NFSResponse {
    /// Lecture réussie
    ReadOk { data: Vec<u8> },
    /// Écriture réussie
    WriteOk { bytes_written: u64 },
    /// Suppression réussie
    DeleteOk,
    /// Listage réussi
    ListOk { entries: Vec<String> },
    /// Erreur
    Error { message: String },
}

/// Message RPC NFS envoyé par le client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFSRequest {
    /// Identifiant unique de la requête
    pub rpc_id: String,
    /// Noeud source (client)
    pub from: NodeId,
    /// Opération à effectuer
    pub operation: NFSOperation,
    /// Timestamp de la requête (pour déterminisme)
    pub timestamp: u64,
}

/// Message de réponse RPC NFS envoyé par le serveur
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFSReply {
    /// Identifiant de la requête originale
    pub rpc_id: String,
    /// Noeud source (serveur)
    pub from: NodeId,
    /// Réponse à l'opération
    pub response: NFSResponse,
    /// Timestamp de la réponse
    pub timestamp: u64,
}

/// Configuration d'un serveur NFS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFSServerConfig {
    /// Identifiant du serveur
    pub id: NodeId,
    /// Chemin du volume exporté
    pub volume_path: String,
    /// Adresse d'écoute TCP
    pub listen_addr: String,
    /// Port de l'API HTTP
    pub http_api: u16,
}

/// Configuration d'un client NFS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFSClientConfig {
    /// Identifiant du client
    pub id: NodeId,
    /// Serveur par défaut
    pub default_server: String,
}

/// Configuration du cluster NFS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFSClusterConfig {
    /// Liste des serveurs
    pub servers: Vec<NFSServerConfig>,
    /// Liste des clients
    pub clients: Vec<NFSClientConfig>,
}

/// Statistiques du serveur NFS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFSServerStats {
    /// Identifiant du serveur
    pub node_id: NodeId,
    /// Nombre d'opérations traitées
    pub operations_count: usize,
    /// Dernières opérations
    pub last_operations: Vec<String>,
    /// Taille du volume en octets
    pub volume_size: u64,
    /// Nombre de fichiers
    pub file_count: usize,
}
