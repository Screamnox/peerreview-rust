pub mod journal;
pub mod network;
pub mod protocols;
pub mod types;

use std::io;
use std::time::Instant;
use ed25519_dalek::Keypair;
use rand::rngs::OsRng;
use std::sync::mpsc::channel;
use std::thread;

use types::node::NodeId;
use types::Node;
use types::Config;
use types::config::PeersConfig;

use journal::Logger;

use network::Bootstrap;

use network::NetworkLayer;

pub enum PeerReviewType {
    CommitmentSend,
    CommitmentRecv,
    CommitmentAck,
    ChallengeResponseSend,
    ChallengeResponseRecv,
    ChallengeResponseReq,
    ConsistencySend,
    ConsistencyRecv,
    EvidenceSend,
    EvidenceRecv,
    AuditSend,
    AuditRecv,
}

pub struct PendingResponse {
    protocol_type: PeerReviewType,
    peer_id: NodeId,
    created_time: Instant,
}

pub struct PeerReviewRuntime {
    node: Node,
    
    /// Application command channel
    task_rx: std::sync::mpsc::Receiver<(PeerReviewType,NodeId,String)>,
    task_tx: std::sync::mpsc::Sender<(PeerReviewType,NodeId,String)>,

    shutdown_rx: std::sync::mpsc::Receiver<bool>,
    shutdown_tx: std::sync::mpsc::Sender<bool>,

    /// Pending responses we're waiting for
    pending_responses: Vec<PendingResponse>,
}

impl PeerReviewRuntime {
    pub fn new(config_file: &str) -> std::io::Result<Self> {

        let config = Config::from_file(config_file).unwrap();
        let peers_config = PeersConfig::from_file(&config.network.peers_file).unwrap();

        // Keypair
        let mut rng = OsRng;
        let keypair = Keypair::generate(&mut rng);

        // Logger
        let logger = Logger::new(
            &config.node.log_file,
            config.node.log_max_lines,
            config.node.log_min_line_size,
        )?;

        // Bootstrap
        let connections = Bootstrap::connect_to_peers(
            config.node.id,
            &config.network.listen_address, 
            &peers_config,
            config.network.connection_timeout_secs
        ).unwrap();

        let peer_infos = Bootstrap::get_peer_info(config.node.id, &peers_config).unwrap();

        let (task_tx, task_rx) = channel::<(PeerReviewType,NodeId,String)>();
        let (shutdown_tx, shutdown_rx) = channel::<bool>();

        // Node
        let mut node = Node::new(
            config.node.id,
            keypair,
            logger,
            config.witnesses.list.clone(),
            NetworkLayer::new(connections, task_tx.clone())?,
        );
        
        for (peer_id, public_key, witnesses) in peer_infos {
            node.add_peer(peer_id, public_key, witnesses);
        }

        Ok(Self {
            node,
            task_rx,
            task_tx,
            shutdown_rx,
            shutdown_tx,
            pending_responses: Vec::new(),
        })
    }

    pub fn run(mut self) -> std::io::Result<()> {
        println!("[Runtime] PeerReview runtime started");
        
        let mut shutdown = false;

        let handle = thread::spawn(|| {
            audit_and_consistency();
        });

        while !shutdown {
            shutdown = self.shutdown_rx.try_recv().unwrap();
            let (task, peerid , msg) = self.task_rx.recv().unwrap();
            match task {
                PeerReviewType::CommitmentSend => {
                    self.node.send_message(peerid, &msg);
                    // TODO : Ne pas oublier de matcher sur l'erreur pour gérer le cas du noeud qui réponds pas
                }
                PeerReviewType::CommitmentRecv => {
                    self.node.recv_message(peerid, &msg);
                }
                PeerReviewType::CommitmentAck => {
                    self.node.verify_ack(peerid, &msg);
                }
                PeerReviewType::ChallengeResponseReq => {
                    self.node.ask_cr();
                }
                PeerReviewType::ChallengeResponseSend => {
                    self.node.send_cr();
                }
                PeerReviewType::ChallengeResponseRecv => {
                    self.node.answer_cr();
                }
                PeerReviewType::AuditRecv => {
                    self.node.answer_audit();
                }
                PeerReviewType::ConsistencyRecv => {
                    self.node.answer_consistency();
                }
                PeerReviewType::EvidenceRecv => {
                    self.node.check_and_update_exposed();
                }
                default => {
                    return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Ce type de message PeerReview n'est pas traité par la réception du runtime"
                    ))
                }
            }
        }

        handle.join().unwrap();

        Ok(())
    }
}