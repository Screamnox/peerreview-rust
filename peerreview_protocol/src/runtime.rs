use std::io;
use tokio::task::JoinHandle;
use tokio::sync::mpsc;

use crate::types::NodeId;
use crate::types::messages::PeerReviewMsg;

/// Runtime léger et stable : il ne force pas l’architecture réseau,
/// mais donne une structure standard "comme dans l’article":
/// - canaux in/out
/// - handles de tâches
///
/// IMPORTANT: même si ton node principal n'utilise pas encore ce runtime,
/// il doit exister car `lib.rs` le ré-exporte.
pub struct PeerReviewRuntime {
    /// Entrée (messages PR reçus du réseau)
    pub inbound: mpsc::Receiver<(NodeId, PeerReviewMsg)>,

    /// Sortie (messages PR à envoyer au réseau)
    pub outbound: mpsc::Sender<(NodeId, PeerReviewMsg)>,

    handles: Vec<JoinHandle<()>>,
}

impl PeerReviewRuntime {
    /// Crée un runtime "standalone" (utile tests / démos),
    /// sans imposer la couche réseau.
    pub fn new_channel_only(buffer: usize) -> Self {
        let (tx, rx) = mpsc::channel(buffer);
        Self {
            inbound: rx,
            outbound: tx,
            handles: Vec::new(),
        }
    }

    /// Ajoute un handle de tâche au runtime (optionnel).
    pub fn push_handle(&mut self, h: JoinHandle<()>) {
        self.handles.push(h);
    }

    /// Attend toutes les tâches enregistrées.
    pub async fn join(mut self) -> io::Result<()> {
        while let Some(h) = self.handles.pop() {
            // Si une tâche panique, on remonte une erreur propre
            h.await.map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        }
        Ok(())
    }
}
