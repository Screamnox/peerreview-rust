use ed25519_dalek::PublicKey;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};

use super::layer::NetworkLayer;
use crate::types::config::PeersConfig;
use crate::types::node::NodeId;

pub struct Bootstrap;

impl Bootstrap {
    /// Connecte le noeud à tous les pairs depuis peers.toml
    ///
    /// # Stratégie
    /// - Thread principal : se connecte aux pairs avec ID < node_id (avec retry)
    /// - Thread d'écoute : accepte toutes les connexions entrantes
    /// - Attend que toutes les connexions soient établies
    ///
    /// # Arguments
    /// * `node_id` - ID de ce noeud
    /// * `network` - Arc<Mutex<NetworkLayer>> (thread-safe)
    /// * `peers_config` - Configuration des pairs depuis peers.toml
    /// * `timeout_secs` - Timeout pour les connexions TCP
    ///
    /// # Retour
    /// Liste des (NodeId, PublicKey, Vec<NodeId>) pour chaque pair connecté
    pub fn connect_to_peers(
        node_id: NodeId,
        listen_addr: &str,
        network: Arc<Mutex<NetworkLayer>>,
        peers_config: &PeersConfig,
        timeout_secs: u64,
    ) -> Result<Vec<(NodeId, PublicKey, Vec<NodeId>)>, String> {
        let timeout = Duration::from_secs(timeout_secs);
        
        // Liste des pairs attendus
        let expected_peers: Vec<NodeId> = peers_config
            .peers
            .iter()
            .filter(|p| p.id != node_id)
            .map(|p| p.id)
            .collect();

        let expected_count = expected_peers.len();
        println!("[Bootstrap] Node {} attend {} pairs: {:?}", node_id, expected_count, expected_peers);

        let done = Arc::new(AtomicBool::new(false));

        // Thread acceptant les connexions entrantes
        let accept_handle = {
            let network = Arc::clone(&network);
            let done = Arc::clone(&done);

            // TODO: Change with arg listen_addr OR network config
            let listen_addr: SocketAddr = listen_addr.parse().unwrap();
            let listener = std::net::TcpListener::bind(listen_addr)
                .map_err(|e| e.to_string())?;

            listener.set_nonblocking(false)
                .map_err(|e| e.to_string())?;

            std::thread::spawn(move || {
                println!("[Bootstrap] Thread d'écoute démarré");

                listener.set_nonblocking(true).unwrap();

                while !done.load(Ordering::SeqCst) {
                    match Self::accept_connection_with_handshake(node_id, &listener, timeout) {
                        Ok((peer_id, stream)) => {
                            let mut net = network.lock().unwrap();

                            if !net.has_peer(peer_id) {
                                net.add_peer(peer_id, stream);
                                println!("[Bootstrap] Pair {} connecté (entrant) [{}/{}]", 
                                    peer_id, net.peer_count(), expected_count);
                            } else {
                                println!("[Bootstrap] Pair {} déjà connecté, ignoré", peer_id);
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        Err(e) => {
                            eprintln!("[Bootstrap] Erreur acceptation : {}", e);
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }

                println!("[Bootstrap] Thread d'écoute arrêté");
            })
        };

        // Connexions sortantes vers les pairs avec ID < node_id (AVEC RETRY)
        let connect_handle = {
            let network = Arc::clone(&network);
            let done = Arc::clone(&done);
            let peers_config = peers_config.clone();
            
            std::thread::spawn(move || {
                println!("[Bootstrap] Thread de connexion sortante démarré");
                
                let start = Instant::now();
                let mut last_attempt: HashMap<NodeId, Instant> = std::collections::HashMap::new();
                const RETRY_AFTER: Duration = Duration::from_secs(2);

                // Continue tant qu'on n'a pas tous les pairs ou timeout
                while !done.load(Ordering::SeqCst) {
                    if start.elapsed() > timeout {
                        eprintln!("[Bootstrap] Timeout connexions sortantes");
                        break;
                    }

                    for peer in peers_config.peers.iter().filter(|p| p.id < node_id) {
                        // Skip si déjà connecté
                        if network.lock().unwrap().has_peer(peer.id) {
                            continue;
                        }

                        // Skip si retentative récente 
                        let now = Instant::now();
                        let should_retry = last_attempt
                            .get(&peer.id)
                            .map(|t| now.duration_since(*t) >= RETRY_AFTER)
                            .unwrap_or(true);

                        if !should_retry {
                            continue;
                        }

                        last_attempt.insert(peer.id, now);

                        match Self::connect_with_handshake(node_id, &peer.address, timeout) {
                            Ok((peer_id, stream)) => {
                                let mut net = network.lock().unwrap();

                                if !net.has_peer(peer_id) {
                                    net.add_peer(peer_id, stream);
                                    println!("[Bootstrap] Pair {} connecté (sortant) [{}/{}]", 
                                        peer_id, net.peer_count(), expected_count);
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[Bootstrap] Tentative de connexion à {} : {} (retry...)",
                                    peer.id, e
                                );
                            }
                        }
                    }

                    // Petit délai entre les tentatives
                    std::thread::sleep(Duration::from_millis(200));
                }
                
                println!("[Bootstrap] Thread de connexion sortante terminé");
            })
        };

        // Attendre que toutes les connexions soient établies
        let start = Instant::now();
        loop {
            let current_count = network.lock().unwrap().peer_count();
            
            if current_count >= expected_count {
                println!("[Bootstrap] Tous les pairs connectés ({}/{})", current_count, expected_count);
                done.store(true, Ordering::SeqCst);
                break;
            }

            if start.elapsed() > timeout {
                return Err(format!(
                    "Timeout bootstrap: seulement {}/{} pairs connectés après {:?}",
                    current_count, expected_count, timeout
                ));
            }

            // Log progression toutes les 2 secondes
            if start.elapsed().as_secs() % 2 == 0 {
                println!("[Bootstrap] Progression: {}/{} pairs connectés...", current_count, expected_count);
            }

            std::thread::sleep(Duration::from_millis(200));
        }

        // Attendre la fin des threads
        let _ = accept_handle.join();
        let _ = connect_handle.join();

        // Construire les infos des pairs
        let mut infos = Vec::new();
        for peer in peers_config.peers.iter().filter(|p| p.id != node_id) {
            let pk = Self::decode_public_key(&peer.public_key)?;
            infos.push((peer.id, pk, peer.witnesses.clone()));
        }

        println!("[Bootstrap] Terminé : {} pairs connectés", infos.len());
        Ok(infos)
    }

    /// Établit une connexion avec handshake
    ///
    /// # Protocole de handshake
    /// 1. Noeud A initie la connexion avec noeud B
    /// 2. Noeud A envoie son ID (4 bytes big-endian)
    /// 3. Noeud B répond avec son ID (4 bytes big-endian)
    fn connect_with_handshake(
        our_id: NodeId,
        address: &str,
        timeout: Duration,
    ) -> std::io::Result<(NodeId, TcpStream)> {
        let mut addrs_iter = address.to_socket_addrs().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("Adresse invalide: {}", address))
        })?;
        let socket_addr = addrs_iter.next().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("Adresse non résolue: {}", address))
        })?;

        let mut stream = TcpStream::connect_timeout(&socket_addr, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        // Envoyer notre ID
        stream.write_all(&our_id.to_be_bytes())?;
        stream.flush()?;

        // Recevoir l'ID du pair
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf)?;
        let peer_id = u32::from_be_bytes(buf);

        Ok((peer_id, stream))
    }

    /// Accepte une connexion avec handshake
    ///
    /// # Protocole de handshake
    /// 1. Noeud B accepte la connexion
    /// 2. Noeud B reçoit l'ID du noeud A (4 bytes big-endian)
    /// 3. Noeud B répond avec son ID (4 bytes big-endian)
    fn accept_connection_with_handshake(
        our_id: NodeId,
        listener: &std::net::TcpListener,
        timeout: Duration,
    ) -> std::io::Result<(NodeId, TcpStream)> {
        let (mut stream, _addr) = listener.accept()?;

        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        // Recevoir l'ID du pair
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf)?;
        let peer_id = u32::from_be_bytes(buf);

        // Envoyer notre ID
        stream.write_all(&our_id.to_be_bytes())?;
        stream.flush()?;

        Ok((peer_id, stream))
    }

    /// Décode une clé publique depuis base64
    fn decode_public_key(base64_str: &str) -> Result<PublicKey, String> {
        use base64::{engine::general_purpose, Engine as _};

        let bytes = general_purpose::STANDARD
            .decode(base64_str)
            .map_err(|e| format!("Échec de décodage base64: {}", e))?;

        if bytes.len() != 32 {
            return Err(format!(
                "Taille de clé invalide: {} (attendu 32)",
                bytes.len()
            ));
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&bytes);

        PublicKey::from_bytes(&key_bytes).map_err(|e| format!("Clé publique invalide: {}", e))
    }
}