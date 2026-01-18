use ed25519_dalek::PublicKey;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::types::config::PeersConfig;
use crate::types::node::NodeId;

pub struct Bootstrap;

impl Bootstrap {
    /// Connect to all peers from peers.toml
    ///
    /// # Strategy
    /// - Main thread: connects to peers with ID < node_id (with retry)
    /// - Accept thread: accepts all incoming connections
    /// - Waits until all connections are established
    ///
    /// # Arguments
    /// * `node_id` - ID of this node
    /// * `listen_addr` - Address to listen on (e.g., "0.0.0.0:5001")
    /// * `peers_config` - Peer configuration from peers.toml
    /// * `timeout_secs` - Timeout for TCP connections
    ///
    /// # Returns
    /// HashMap<NodeId, TcpStream> for all connected peers
    pub fn connect_to_peers(
        node_id: NodeId,
        listen_addr: &str,
        peers_config: &PeersConfig,
        timeout_secs: u64,
    ) -> Result<HashMap<NodeId, TcpStream>, String> {
        let timeout = Duration::from_secs(timeout_secs);
        
        // Expected peers (excluding ourselves)
        let expected_peers: Vec<NodeId> = peers_config
            .peers
            .iter()
            .filter(|p| p.id != node_id)
            .map(|p| p.id)
            .collect();

        let expected_count = expected_peers.len();
        println!("[Bootstrap] Node {} expecting {} peers: {:?}", node_id, expected_count, expected_peers);

        let connections: Arc<Mutex<HashMap<NodeId, TcpStream>>> = 
            Arc::new(Mutex::new(HashMap::new()));

        let connected_count = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));

        // Thread accepting incoming connections
        let accept_handle = {
            let connections = Arc::clone(&connections);
            let connected_count = Arc::clone(&connected_count);
            let done = Arc::clone(&done);

            let listen_addr: SocketAddr = listen_addr.parse()
                .map_err(|e| format!("Invalid listen address: {}", e))?;
            
            let listener = std::net::TcpListener::bind(listen_addr)
                .map_err(|e| format!("Failed to bind listener: {}", e))?;

            listener.set_nonblocking(true)
                .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

            std::thread::spawn(move || {
                println!("[Bootstrap] Accept thread started");

                while !done.load(Ordering::SeqCst) {
                    match Self::accept_connection_with_handshake(node_id, &listener, timeout) {
                        Ok((peer_id, stream)) => {
                            let mut conns = connections.lock().unwrap();

                            if !conns.contains_key(&peer_id) {
                                conns.insert(peer_id, stream);
                                let count = connected_count.fetch_add(1, Ordering::SeqCst) + 1;
                                println!("[Bootstrap] Peer {} connected (incoming) [{}/{}]", 
                                    peer_id, count, expected_count);
                            } else {
                                println!("[Bootstrap] Peer {} already connected, ignored", peer_id);
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        Err(e) => {
                            eprintln!("[Bootstrap] Accept error: {}", e);
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }

                println!("[Bootstrap] Accept thread stopped");
            })
        };

        // Outgoing connections to peers with ID < node_id (WITH RETRY)
        let connect_handle = {
            let connections = Arc::clone(&connections);
            let connected_count = Arc::clone(&connected_count);
            let done = Arc::clone(&done);
            let peers_config = peers_config.clone();
            
            std::thread::spawn(move || {
                println!("[Bootstrap] Outgoing connection thread started");
                
                let start = Instant::now();
                let mut last_attempt: HashMap<NodeId, Instant> = HashMap::new();
                const RETRY_AFTER: Duration = Duration::from_secs(2);

                while !done.load(Ordering::SeqCst) {
                    if start.elapsed() > timeout {
                        eprintln!("[Bootstrap] Outgoing connections timeout");
                        break;
                    }

                    for peer in peers_config.peers.iter().filter(|p| p.id < node_id) {
                        // Skip if already connected
                        if connections.lock().unwrap().contains_key(&peer.id) {
                            continue;
                        }

                        // Skip if recent retry
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
                                let mut conns = connections.lock().unwrap();

                                if !conns.contains_key(&peer_id) {
                                    conns.insert(peer_id, stream);
                                    let count = connected_count.fetch_add(1, Ordering::SeqCst) + 1;
                                    println!("[Bootstrap] Peer {} connected (outgoing) [{}/{}]", 
                                        peer_id, count, expected_count);
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[Bootstrap] Connection attempt to {} failed: {} (retrying...)",
                                    peer.id, e
                                );
                            }
                        }
                    }

                    std::thread::sleep(Duration::from_millis(200));
                }
                
                println!("[Bootstrap] Outgoing connection thread finished");
            })
        };

        // Wait for all connections to be established
        let start = Instant::now();
        let mut last_log = Instant::now();
        
        loop {
            let current_count = connected_count.load(Ordering::SeqCst);
            
            if current_count >= expected_count {
                println!("[Bootstrap] All peers connected ({}/{})", current_count, expected_count);
                done.store(true, Ordering::SeqCst);
                break;
            }

            if start.elapsed() > timeout {
                done.store(true, Ordering::SeqCst);
                let _ = accept_handle.join();
                let _ = connect_handle.join();
                
                return Err(format!(
                    "Bootstrap timeout: only {}/{} peers connected after {:?}",
                    current_count, expected_count, timeout
                ));
            }

            // Log progress every 3 seconds
            if last_log.elapsed() >= Duration::from_secs(3) {
                println!("[Bootstrap] Progress: {}/{} peers connected...", current_count, expected_count);
                last_log = Instant::now();
            }

            std::thread::sleep(Duration::from_millis(200));
        }

        // Wait for threads to finish
        let _ = accept_handle.join();
        let _ = connect_handle.join();

        // Extract connections from Arc<Mutex<>>
        let connections = Arc::try_unwrap(connections)
            .map_err(|_| "Failed to unwrap connections Arc")?
            .into_inner()
            .map_err(|_| "Failed to unwrap connections Mutex")?;

        println!("[Bootstrap] Complete: {} peers connected", connections.len());
        Ok(connections)
    }

    /// Get peer information from config
    /// 
    /// Returns Vec<(NodeId, PublicKey, Vec<NodeId>)> for all peers except the given node_id
    pub fn get_peer_info(
        node_id: NodeId,
        peers_config: &PeersConfig,
    ) -> Result<Vec<(NodeId, PublicKey, Vec<NodeId>)>, String> {
        let mut infos = Vec::new();
        
        for peer in peers_config.peers.iter().filter(|p| p.id != node_id) {
            let pk = Self::decode_public_key(&peer.public_key)?;
            infos.push((peer.id, pk, peer.witnesses.clone()));
        }
        
        Ok(infos)
    }

    /// Establish connection with handshake
    ///
    /// # Handshake protocol
    /// 1. Node A initiates connection to node B
    /// 2. Node A sends its ID (4 bytes big-endian)
    /// 3. Node B responds with its ID (4 bytes big-endian)
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

        stream.write_all(&our_id.to_be_bytes())?;
        stream.flush()?;

        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf)?;
        let peer_id = u32::from_be_bytes(buf);

        Ok((peer_id, stream))
    }

    /// Accept connection with handshake
    ///
    /// # Handshake protocol
    /// 1. Node B accepts connection
    /// 2. Node B receives ID from node A (4 bytes big-endian)
    /// 3. Node B responds with its ID (4 bytes big-endian)
    fn accept_connection_with_handshake(
        our_id: NodeId,
        listener: &std::net::TcpListener,
        timeout: Duration,
    ) -> std::io::Result<(NodeId, TcpStream)> {
        let (mut stream, _addr) = listener.accept()?;

        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf)?;
        let peer_id = u32::from_be_bytes(buf);

        stream.write_all(&our_id.to_be_bytes())?;
        stream.flush()?;

        Ok((peer_id, stream))
    }

    /// Decode public key from base64
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