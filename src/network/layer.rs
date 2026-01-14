use mio::{Events, Interest, Poll, Token, Waker};
use mio::net::{TcpStream};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Sender, Receiver};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::types::messages::PeerReviewMsg;
use crate::types::node::NodeId;

const WAKE_TOKEN: Token = Token(usize::MAX);

/// Requête d'envoi de message
struct SendRequest {
    peer_id: NodeId,
    encoded: Vec<u8>,
}

/// Network layer est un wrapper réseau pour gérer
/// communications TCP avec les noeuds
/// 
/// # Architecture
/// - 1 thread reactor (mio::Poll) : gère tous les I/O non-bloquants
/// - N threads workers : traitent les messages décodés
/// 
/// # Règles de propriété
/// - SEUL le reactor touche les TcpStream
/// - Les workers ne voient jamais les sockets
/// - Les workers reçoivent uniquement (peer_id, message)
#[derive(Debug)]
pub struct NetworkLayer {
    poll: Poll,             // poll évènements I/O

    /// Mapping Token <-> NodeId
    token_to_peer: HashMap<Token, NodeId>,
    peer_to_token: HashMap<NodeId, Token>,

    /// Connexions TCP
    connections: HashMap<Token, TcpStream>,

    /// Buffers de lecture et d'écriture par connexion
    /// Note: Permet en lecture d'attendre le bon nombre d'octets
    /// avant de décoder le message.
    read_buffers: HashMap<Token, Vec<u8>>,
    write_buffers: HashMap<Token, Vec<u8>>,

    // TODO: Worker thread pool
    /// Canal pour recevoir les requêtes d'envoi depuis d'autres threads
    send_rx: Option<Receiver<SendRequest>>,

    /// Prochain token disponible
    next_token: usize,
}

impl NetworkLayer {
    /// Créer une nouvelle couche réseau
    ///
    /// # Arguments
    /// * `listen_addr` - Adresse d'écoute (ex: "0.0.0.0:5001")
    pub fn new() -> io::Result<Self> {
        let poll = Poll::new()?;

        Ok(Self {
            poll,
            token_to_peer: HashMap::new(),
            peer_to_token: HashMap::new(),
            connections: HashMap::new(),
            read_buffers: HashMap::new(),
            write_buffers: HashMap::new(),
            send_rx: None,
            next_token: 0,
        })
    }
    
    /// Compte le nombre de pairs connectés
    pub fn peer_count(&self) -> usize {
        self.peer_to_token.len()
    }
    
    /// Vérifie si un pair est connecté
    pub fn has_peer(&self, peer_id: NodeId) -> bool {
        self.peer_to_token.contains_key(&peer_id)
    }
    
    /// Retourne la liste des IDs de pairs connectés
    pub fn get_peer_ids(&self) -> Vec<NodeId> {
        self.peer_to_token.keys().copied().collect()
    }

    /// Enregistre un nouveau pair
    /// 
    /// Note : Appelée apr_s le bootstrap
    /// 
    /// IMPORTANT : Cette méthode doit être appelée AVANT start_event_loop()
    /// 
    /// # Arguments
    /// * `peer_id` - ID du pair
    /// * `stream` - Connexion TCP STANDARD du pair
    pub fn register_peer(&mut self, peer_id: NodeId, mut stream: TcpStream) -> io::Result<()> {
        // TODO: Check if peer already added
        
        // stream.set_nodelay(true)?;

        let token = Token(self.next_token);
        self.next_token += 1;

        self.poll.registry().register(
            &mut stream,
            token,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        self.token_to_peer.insert(token, peer_id);
        self.peer_to_token.insert(peer_id, token);
        self.connections.insert(token, stream);
        self.read_buffers.insert(token, Vec::new());
        self.write_buffers.insert(token, Vec::new());

        println!("[NetworkLayer] Pair {} enregistré avec token {:?}", peer_id, token);
        
        Ok(())
    }

    /// Démarre la boucle d'évènement dans un thread
    /// 
    /// Cette méthode prend ownership de self et ne retourne un NetworkSender
    /// pour envoyer des messages depuis d'autres threads.
    /// 
    /// # Arguments
    /// * `callback` - Fonction appelée pour chaque message reçu (1 thread par msg)
    /// 
    /// # Retour
    /// * `NetworkSender` - Interface thread-safe pour envoyer des messages
    /// * `JoinHandle` - Handle du thread reactor
    pub fn start_event_loop(
        mut self,
        callback: impl Fn(NodeId, PeerReviewMsg) + Send + Sync + 'static
    ) -> io::Result<(NetworkSender, JoinHandle<io::Result<()>>)> {
        let mut events = Events::with_capacity(128);

        let callback: Arc<dyn Fn(NodeId, PeerReviewMsg) + Send + Sync> = Arc::new(callback);

        // TODO: Check if useful to clone
        let (send_tx, send_rx) = mpsc::channel();
        self.send_rx = Some(send_rx);

        let waker = Arc::new(Waker::new(self.poll.registry(), WAKE_TOKEN)?);

        let sender = NetworkSender { 
            sender: send_tx,
            waker: Arc::clone(&waker),
        };

        println!("[NetworkLayer] Boucle d'événements démarrée");

        // wait the thread to be ready
        let (ready_tx, ready_rx) = mpsc::channel::<()>();

        let handle = thread::spawn(move || {
            ready_tx.send(()).expect("Failed to send ready signal");
            self.run_event_loop(&mut events, callback)
        });

        ready_rx.recv()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Reactor failed to start"))?;
        
        // TODO: Replace by initial pool before ready_tx OR pass it to run_event_loop
        // std::thread::sleep(std::time::Duration::from_millis(50));   // ensure poll started in run_event_loop
    
        println!("[NetworkLayer] Event loop is ready");

        Ok((sender, handle))
    }

    /// Boucle principale du reactor
    fn run_event_loop(
        &mut self,
        events: &mut Events,
        callback: Arc<dyn Fn(NodeId, PeerReviewMsg) + Send + Sync>,
    ) -> io::Result<()> {
        loop {
            self.poll.poll(events, None)?;

            // Traiter les requêtes d'envoi en attente (non-bloquant)
            let mut send_requests = Vec::new();
            if let Some(ref rx) = self.send_rx {
                while let Ok(req) = rx.try_recv() {
                    send_requests.push(req);
                }
            }

            for req in send_requests {
                if let Err(e) = self.queue_send(req.peer_id, req.encoded) {
                        eprintln!("[NetworkLayer] Erreur queue_send pour {}: {}", req.peer_id, e);
                }
            }

            // Traiter les évènements I/O
            for event in events.iter() {
                match event.token() {
                    WAKE_TOKEN => continue,
                    token => {
                        if event.is_readable() {
                            if let Err(e) = self.handle_readable(token, Arc::clone(&callback)) {
                                eprintln!("[NetworkLayer] Erreur lecture token {:?}: {}", token, e);
                                self.drop_peer(token)?;
                            }
                        }

                        if event.is_writable() {
                            if let Err(e) = self.handle_writable(token) {
                                eprintln!("[NetworkLayer] Erreur écriture token {:?}: {}", token, e);
                                self.drop_peer(token)?;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Ajoute des données au buffer d'écriture d'un pair
    fn queue_send(&mut self, peer_id: NodeId, encoded: Vec<u8>) -> io::Result<()> {    
        let token = self.peer_to_token.get(&peer_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("Pair {} non connecté", peer_id)))?;
        
        let write_buffer = self.write_buffers.get_mut(token).unwrap();
        write_buffer.extend_from_slice(&encoded);
        
        // Essayer d'écrire immédiatement
        self.handle_writable(*token)?;

        Ok(())
    }

    /// Gère les évènements de lecture (décode message)
    fn handle_readable(
        &mut self,
        token: Token,
        callback: Arc<dyn Fn(NodeId, PeerReviewMsg) + Send + Sync>,
    ) -> io::Result<()> {
        let peer_id = match self.token_to_peer.get(&token) {
            Some(&id) => id,
            None => return Ok(()),  // Pair déjà supprimé
        };

        let stream = self.connections.get_mut(&token).unwrap();
        let buffer = self.read_buffers.get_mut(&token).unwrap();

        let mut temp = [0u8; 4096];
        
        loop {
            match stream.read(&mut temp) {
                Ok(0) => {
                    println!("[NetworkLayer] Peer {} disconnected", peer_id);
                    self.drop_peer(token)?;
                    return Ok(());
                }
                Ok(n) => {
                    // TODO: Here, maybe send it to a worker
                    buffer.extend_from_slice(&temp[..n]);

                    while let Some(msg) = Self::try_decode_message(buffer)? {
                        let cb = Arc::clone(&callback);
                        thread::spawn(move || {
                            cb(peer_id, msg);
                        });
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    /// Gérer les évènements d'écriture
    fn handle_writable(&mut self, token: Token) -> io::Result<()> {
        let write_buffer = match self.write_buffers.get_mut(&token) {
            Some(buf) => buf,
            None => {
                return Ok(())
            }
        };

        if write_buffer.is_empty() {
            return Ok(());
        }

        let stream = self.connections.get_mut(&token).unwrap();

        loop {
            match stream.write(write_buffer) {
                Ok(0) => {
                    return Err(io::Error::new(io::ErrorKind::WriteZero, "Write returned 0"));
                }
                Ok(n) => {
                    // Retire les bytes écrits
                    write_buffer.drain(..n);

                    if write_buffer.is_empty() {
                        break;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    break
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                    continue
                }
                Err(e) => {
                    return Err(e)
                }
            }
        }

        Ok(())
    }

    /// Décode un message depuis le buffer (protocole: 4 bytes len + payload)
    fn try_decode_message(buffer: &mut Vec<u8>) -> io::Result<Option<PeerReviewMsg>> {
        const BYTE_SIZE_LEN: usize = 4;

        // Besoin de 4 bytes pour la longueur
        if buffer.len() < BYTE_SIZE_LEN {
            return Ok(None);
        }

        // Note : longueur en big-endian
        let len = u32::from_be_bytes(
            [buffer[0], buffer[1], buffer[2], buffer[3]]
        ) as usize;

        // TODO: Add len max size

        if buffer.len() < BYTE_SIZE_LEN + len {
            return Ok(None);
        }

        let payload = buffer[BYTE_SIZE_LEN..BYTE_SIZE_LEN+len].to_vec();
        buffer.drain(..BYTE_SIZE_LEN+len);

        let (msg, _) = bincode::decode_from_slice(&payload, bincode::config::standard())
            .map_err(|e| io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Erreur de désérialisation: {}", e),
            )
        )?;

        Ok(Some(msg))
    }

    /// Supprime un pair (appelé en cas de déconnexion ou erreur)
    fn drop_peer(&mut self, token: Token) -> io::Result<()> {
        if let Some(peer_id) = self.token_to_peer.remove(&token) {
            self.peer_to_token.remove(&peer_id);
            
            // Désenregistrer du poll
            if let Some(mut stream) = self.connections.remove(&token) {
                let _ = self.poll.registry().deregister(&mut stream);
            }

            self.read_buffers.remove(&token);
            self.write_buffers.remove(&token);
            
            println!("[NetworkLayer] Pair {} supprimé (token {:?})", peer_id, token);
        }
        
        Ok(())
    }
}

/// API d'envoi de messages (thread-safe via canal)
/// 
/// Cette structure peut être clonée et partagée entre threads
#[derive(Clone)]
pub struct NetworkSender {
    sender: Sender<SendRequest>,
    waker: Arc<Waker>,
}

impl NetworkSender {
    /// Envoie un message à un pair
    /// 
    /// Cette méthode est thread-safe et peut être appelée depuis n'importe quel thread
    /// (y compris depuis le callback de réception).
    /// Le message sera mis en queue et envoyé par le reactor.
    pub fn send_message(&self, peer_id: NodeId, msg: &PeerReviewMsg) -> io::Result<()> {
        // Encoder le message
        let payload = bincode::encode_to_vec(msg, bincode::config::standard())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Encodage: {}", e)))?;
        
        // Framing: 4 bytes len + payload
        let len = payload.len() as u32;
        let mut encoded = Vec::with_capacity(4 + payload.len());
        encoded.extend_from_slice(&len.to_be_bytes());
        encoded.extend_from_slice(&payload);
        
        // Envoyer au reactor
        self.sender.send(SendRequest { peer_id, encoded })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Reactor fermé"))?;
        
        self.waker
            .wake()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Wake failed"))?;

        Ok(())
    }
}

// TODO: Remove
/// Bootstrap helper (temporaire)
impl NetworkLayer {
    /// Ajoute un pair
    /// 
    /// DEPRECATED : Utiliser register_peer() à la place
    pub fn add_peer(&mut self, peer_id: NodeId, stream: std::net::TcpStream) {
        println!("[NetworkLayer] Pair {} ajouté ({})", peer_id, 
            stream.peer_addr().unwrap());
        
        // Convertir std::net::TcpStream en mio::net::TcpStream
        stream.set_nonblocking(true).unwrap();
        let mio_stream = TcpStream::from_std(stream);
        
        // Enregistrer le pair
        self.register_peer(peer_id, mio_stream).unwrap();
    }
}