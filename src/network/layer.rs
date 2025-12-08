use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use crate::types::PeerReviewMsg;

pub struct NetworkLayer {
    listener: TcpListener,
    peers: HashMap<u32, Arc<Mutex<TcpStream>>>,
}

impl NetworkLayer {
    // Créer et bind une socker TCP, et créer une hash map vide.
    pub fn new(bind_addr: &str) -> std::io::Result<Self> {
        unimplemented!();
    }

    /// Envoie un message à un pair
    /// Note: Sérialisation bincode + framing (taille + payload), envoi à un pair.
    pub fn send_message(&self, peer_id: u32, msg: &PeerReviewMsg) -> std::io::Result<()> {
        unimplemented!();
    }

    /// Reçoit un message (bloquant)
    /// Note: Réception bloquante d’un message sérialisé.
    pub fn recv_message(stream: &mut TcpStream) -> std::io::Result<PeerReviewMsg> {
        unimplemented!();
    }

    /// Thread d'écoute pour les connexions entrantes
    pub fn listen(&self, callback: impl Fn(u32, PeerReviewMsg) + Send + 'static) {
        unimplemented!();
    }

    /// Récupère le listener
    pub fn get_listener(&self) -> &TcpListener {
        &self.listener
    }
}
