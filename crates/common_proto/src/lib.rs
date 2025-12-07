use serde::{Deserialize, Serialize};

pub type NodeId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MsgKind {
    /// Heartbeat périodique, un par arbre
    Heartbeat { counter: u64, tree_id: u8 },

    /// Message applicatif texte (payload = UTF-8)
    PublishText,

    /// Message applicatif binaire, envoyé en chunks.
    /// Le payload contient un `BinaryChunk` sérialisé avec bincode.
    PublishBinaryChunk,
}

/// Message de base échangé entre les nœuds.
/// `payload` est un vecteur d'octets, interprété en fonction de `kind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Msg {
    pub id: String,
    pub from: NodeId,
    pub kind: MsgKind,
    pub payload: Vec<u8>,
    pub ts_ms: u64,
    pub tree_id: u8,
}

/// Un morceau (chunk) d’un flux binaire (ex : fichier ou vidéo).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryChunk {
    /// Identifiant logique du flux (ex: "video1").
    pub file_id: String,
    /// Index du chunk, à partir de 0.
    pub index: u32,
    /// Nombre total de chunks attendus pour ce fichier.
    pub total_chunks: u32,
    /// Les octets de ce chunk.
    pub data: Vec<u8>,
}
