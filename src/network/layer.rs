use mio::{Events, Interest, Poll, Token, Waker};
use mio::net::{TcpStream};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Sender, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::RuntimeTask;
use crate::types::messages::PeerReviewMsg;
use crate::types::node::NodeId;

/// Wake-up poll when the reactor received
/// a command from mpsc channel.
const WAKE_TOKEN: Token = Token(usize::MAX);

/// Connection state of a peer
struct Connection {
    token: Token,
    stream: TcpStream,
    read_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
}

/// Internal commands sent to the reactor
enum ReactorCommand {
    Send { peer_id: NodeId, encoded: Vec<u8> },
    Shutdown,
}

/// Shared state between users and reactor thread
struct NetworkState {
    /// Commands to send to the reactor
    command_tx: Sender<ReactorCommand>,

    /// Waker to notify reactor of new commands
    waker: Arc<Waker>,

    /// Current peer list (updated by reactor) 
    peers: Arc<Mutex<Vec<NodeId>>>,
}

/// Network layer is a network wrapper for TCP stream peers
pub struct NetworkLayer {
    state: Arc<NetworkState>,
    message_rx: Receiver<(NodeId, PeerReviewMsg)>,
    reactor_handle: Option<JoinHandle<io::Result<()>>>
}

impl NetworkLayer {
    /// Create a new network layer and start reactor thread
    pub fn new(initial_peers: HashMap<NodeId, std::net::TcpStream>, task_tx:Sender<RuntimeTask>) -> io::Result<Self> {
        let poll = Poll::new()?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKE_TOKEN)?);

        let (command_tx, command_rx) = mpsc::channel();
        let (message_tx, message_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        let peers = Arc::new(Mutex::new(Vec::new()));
        let peers_clone = Arc::clone(&peers);

        // Copy initial_peers before passing to reactor thread
        let mut mio_peers = HashMap::new();
        for (peer_id, stream) in initial_peers {
            stream.set_nonblocking(true)?;
            let mio_stream = TcpStream::from_std(stream);
            mio_peers.insert(peer_id, mio_stream);
        }

        let handle = thread::spawn(move || {
            let mut reactor = Reactor {
                poll,
                connections: HashMap::new(),
                token_to_peer: HashMap::new(),
                command_rx,
                message_tx,
                task_tx,
                peers: peers_clone,
                next_token: 0,
            };

            // Register initial peers
            for (peer_id, stream) in mio_peers {
                if let Err(e) = reactor.register_peer(peer_id, stream) {
                    eprintln!("[Reactor] Failed to register peer {}: {}", peer_id, e);
                }
            }

            reactor.run(ready_tx)
        });

        // Wait for reactor to be ready
        ready_rx.recv()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Reactor failed to start"))?;

        let state = Arc::new(NetworkState {
            command_tx,
            waker,
            peers,
        });

        Ok(Self {
            state,
            message_rx,
            reactor_handle: Some(handle),
        })
    }

    /// Send a message to a peer
    pub fn send(&self, peer_id: NodeId, msg: &PeerReviewMsg) -> io::Result<()> {
        let encoded = super::messages::encode(msg)?;
        
        self.state.command_tx
            .send(ReactorCommand::Send { peer_id, encoded })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Reactor closed"))?;
        
        // Wake the reactor to process immediately
        let _ = self.state.waker.wake();
        
        Ok(())
    }
    
    /// Receive a message (blocking)
    /// 
    /// Returns (peer_id, message) or error if reactor is closed
    pub fn recv(&self) -> io::Result<(NodeId, PeerReviewMsg)> {
        self.message_rx.recv()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Reactor closed"))
    }
    
    /// Try to receive a message (non-blocking)
    /// 
    /// Returns Some((peer_id, message)) if a message is available, None otherwise
    pub fn try_recv(&self) -> io::Result<Option<(NodeId, PeerReviewMsg)>> {
        match self.message_rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "Reactor closed"))
            }
        }
    }
    
    /// Get the number of connected peers
    pub fn peer_count(&self) -> usize {
        self.state.peers.lock().unwrap().len()
    }
    
    /// Check if a specific peer is connected
    pub fn has_peer(&self, peer_id: NodeId) -> bool {
        self.state.peers.lock().unwrap().contains(&peer_id)
    }
    
    /// Get list of all connected peer IDs
    pub fn get_peer_ids(&self) -> Vec<NodeId> {
        self.state.peers.lock().unwrap().clone()
    }
    
    /// Shutdown the network layer gracefully
    pub fn shutdown(mut self) -> io::Result<()> {
        let _ = self.state.command_tx.send(ReactorCommand::Shutdown);
        let _ = self.state.waker.wake();
        
        if let Some(handle) = self.reactor_handle.take() {
            handle.join()
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "Reactor panicked"))?
        } else {
            Ok(())
        }
    }
}

impl Drop for NetworkLayer {
    fn drop(&mut self) {
        let _ = self.state.command_tx.send(ReactorCommand::Shutdown);
        let _ = self.state.waker.wake();
    }
}

struct Reactor {
    poll: Poll,
    connections: HashMap<NodeId, Connection>,
    token_to_peer: HashMap<Token, NodeId>,
    command_rx: Receiver<ReactorCommand>,
    message_tx: Sender<(NodeId, PeerReviewMsg)>,
    task_tx: Sender<RuntimeTask>,
    peers: Arc<Mutex<Vec<NodeId>>>,
    next_token: usize,
}

impl Reactor {
    fn run(mut self, ready_signal: Sender<()>) -> io::Result<()> {
        let mut events = Events::with_capacity(128);
        let mut should_shutdown = false;

        // Initial poll
        // self.poll.poll(&mut events, Some(Duration::from_millis(0)))?;

        // Signal that the reactor is ready to poll
        let _ = ready_signal.send(());

        loop {
            self.poll.poll(&mut events, None)?;

            while let Ok(cmd) = self.command_rx.try_recv() {
                match cmd {
                    ReactorCommand::Send { peer_id, encoded } => {
                        if let Err(e) = self.queue_send(peer_id, encoded) {
                            eprintln!("[Reactor] Send error for peer {}: {}", peer_id, e);
                        }
                    }
                    ReactorCommand::Shutdown => {
                        should_shutdown = true;
                        break;
                    }
                }
            }

            if should_shutdown {
                println!("[Reactor] Shutting down");
                break;
            }

            for event in events.iter() {
                match event.token() {
                    WAKE_TOKEN => {
                        // Just a wake-up call
                        continue;
                    }
                    token => {
                        if event.is_readable() {
                            if let Err(e) = self.handle_readable(token) {
                                eprintln!("[Reactor] Read error on {:?}: {}", token, e);
                                let _ = self.drop_peer(token);
                            }
                        }
                        
                        if event.is_writable() {
                            if let Err(e) = self.handle_writable(token) {
                                eprintln!("[Reactor] Write error on {:?}: {}", token, e);
                                let _ = self.drop_peer(token);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn register_peer(&mut self, peer_id: NodeId, mut stream: TcpStream) -> io::Result<()> {
        let token = Token(self.next_token);
        self.next_token += 1;

        self.poll.registry().register(
            &mut stream,
            token,
            Interest::READABLE | Interest::WRITABLE
        )?;

        let conn = Connection {
            token,
            stream,
            read_buffer: Vec::new(),
            write_buffer: Vec::new(),
        };

        self.connections.insert(peer_id, conn);
        self.token_to_peer.insert(token, peer_id);

        let mut peers = self.peers.lock().unwrap();
        if !peers.contains(&peer_id) {
            peers.push(peer_id);
        }

        println!("[Reactor] Peer {} registered with token {:?}", peer_id, token);
        
        Ok(())
    }

    fn queue_send(&mut self, peer_id: NodeId, encoded: Vec<u8>) -> io::Result<()> {
        let token = {
            let conn = self.connections.get_mut(&peer_id)
                .ok_or_else(|| io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Peer {} not connected", peer_id)
                ))?;

            conn.write_buffer.extend_from_slice(&encoded);
            conn.token
        };

        self.handle_writable(token)?;

        Ok(())
    }

    fn handle_readable(&mut self, token: Token) -> io::Result<()> {
        let peer_id = *self.token_to_peer.get(&token)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Token not found"))?;

        let conn = self.connections.get_mut(&peer_id).unwrap();

        let mut temp = [0u8; 4096];

        loop {
            match conn.stream.read(&mut temp) {
                Ok(0) => {
                    println!("[Reactor] Peer {} disconnected", peer_id);
                    return self.drop_peer(token);
                }
                Ok(n) => {
                    conn.read_buffer.extend_from_slice(&temp[..n]);

                    while let Some(msg) = super::messages::try_decode(&mut conn.read_buffer)? {
                        // Send to the user via channel
                        if self.message_tx.send((peer_id, msg)).is_err() {
                            // User dropped the receiver, might as well stop
                            return Err(io::Error::new(
                                io::ErrorKind::BrokenPipe,
                                "Message receiver dropped"
                            ));
                        }

                        // Send a signal to the runtime
                        self.task_tx
                            .send(RuntimeTask::ReceiveMessage)
                            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))?;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    fn handle_writable(&mut self, token: Token) -> io::Result<()> {
        let peer_id = match self.token_to_peer.get(&token) {
            Some(&id) => id,
            None => return Ok(()),
        };
        
        let conn = self.connections.get_mut(&peer_id).unwrap();
        
        if conn.write_buffer.is_empty() {
            return Ok(());
        }
        
        loop {
            match conn.stream.write(&conn.write_buffer) {
                Ok(0) => {
                    return Err(io::Error::new(io::ErrorKind::WriteZero, "Write returned 0"));
                }
                Ok(n) => {
                    conn.write_buffer.drain(..n);
                    
                    if conn.write_buffer.is_empty() {
                        break;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        
        Ok(())
    }

    fn drop_peer(&mut self, token: Token) -> io::Result<()> {
        if let Some(peer_id) = self.token_to_peer.remove(&token) {
            if let Some(mut conn) = self.connections.remove(&peer_id) {
                let _ = self.poll.registry().deregister(&mut conn.stream);
            }

            let mut peers = self.peers.lock().unwrap();
            peers.retain(|&id| id != peer_id);
            
            println!("[Reactor] Peer {} removed (token {:?})", peer_id, token);
        }
        
        Ok(())
    }
}
