pub mod journal;
pub mod network;
pub mod protocols;
pub mod types;

use ed25519_dalek::{Keypair, PublicKey, SecretKey};
use std::fs;
use std::io;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use types::Config;
use types::Node;
use types::PeerReviewMsg;
use types::config::PeersConfig;
use types::messages::ChallengeKey;
use types::node::NodeId;

use journal::Logger;

use network::Bootstrap;
use network::NetworkLayer;

// TODO use from config file
/// Timeout and interval (in seconds)
const RESPONSE_TIMEOUT_SECS: u64 = 30;
const AUDIT_INTERVAL_SECS: u64 = 60;
const CONSISTENCY_INTERVAL_SECS: u64 = 120;
const EVIDENCE_COLLECTION_INTERVAL_SECS: u64 = 300;

/// Task types for the runtime task queue
#[derive(Debug, Clone)]
pub enum RuntimeTask {
    /// Send a message to a peer
    SendMessage { dest: NodeId, msg: String },

    /// Receive an incoming PR message signal
    ReceiveMessage,

    /// Periodic audit check
    PeriodicAudit,

    // Note: now trigger by a threshold
    // Periodic consistency check
    // PeriodicConsistency,

    /// Periodic evidence collection
    PeriodicEvidenceCollection,
    // Shutdown signal
    // Shutdown,
}

/// Tracks pending operations waiting for responses
#[derive(Debug, Clone)]
pub struct PendingOperation {
    peer_id: NodeId,
    challenge_key: Option<ChallengeKey>,
    original_msg: Option<PeerReviewMsg>, // TODO update
    created_time: Instant,
    operation_type: OperationType,
}

#[derive(Debug, Clone)]
pub enum OperationType {
    MessageSend,
    ChallengeSend,
    AuditCheck,
    ConsistencyCheck,
    EvidenceRequest,
}

pub struct PeerReviewRuntime {
    node: Node,

    /// Task channel for application commands
    task_rx: Receiver<RuntimeTask>,
    task_tx: Sender<RuntimeTask>,

    /// Shutdown channel
    shutdown_rx: Receiver<bool>,
    shutdown_tx: Sender<bool>,

    /// Pending operations waiting for responses
    pending_operations: Vec<PendingOperation>,

    /// Last time periodic tasks were run
    last_audit: Instant,
    last_consistency_check: Instant,
    last_evidence_collection: Instant,
}

impl PeerReviewRuntime {
    /// Create a new PeerReview runtime from configuration
    pub fn new(config_file: &str) -> std::io::Result<Self> {
        let config = Config::from_file(config_file)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let peers_config = PeersConfig::from_file(&config.network.peers_file)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

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
            config.network.connection_timeout_secs,
        )
        .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

        let peer_infos = Bootstrap::get_peer_info(config.node.id, &peers_config)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Channels
        let (task_tx, task_rx) = channel::<RuntimeTask>();
        let (shutdown_tx, shutdown_rx) = channel::<bool>();

        // Node
        let mut node = Node::new(
            config.node.id,
            Self::load_keypair(&config.node.keypair_file),
            logger,
            config.witnesses.list.clone(),
            NetworkLayer::new(connections, task_tx.clone())?,
        );

        for (peer_id, public_key, witnesses) in peer_infos {
            node.add_peer(peer_id, public_key, witnesses);
        }

        let now = Instant::now();

        Ok(Self {
            node,
            task_rx,
            task_tx,
            shutdown_rx,
            shutdown_tx,
            pending_operations: Vec::new(),
            last_audit: now,
            last_consistency_check: now,
            last_evidence_collection: now,
        })
    }

    /// Get a sender for submitting tasks to the runtime
    pub fn get_task_sender(&self) -> Sender<RuntimeTask> {
        self.task_tx.clone()
    }

    /// Get a sender for shutdown signal
    pub fn get_shutdown_sender(&self) -> Sender<bool> {
        self.shutdown_tx.clone()
    }

    pub fn run(mut self) -> std::io::Result<()> {
        println!(
            "[Runtime] PeerReview runtime started for node {}",
            self.node.id
        );

        let (periodic_shutdown_tx, periodic_shutdown_rx) = channel::<bool>();
        let periodic_task_tx = self.task_tx.clone();

        let periodic_handle = thread::spawn(move || {
            loop {
                // Check for shutdown signal
                if let Ok(_) = periodic_shutdown_rx.try_recv() {
                    break;
                }

                // TODO customize the periodic part
                // Sleep before next iteration
                thread::sleep(Duration::from_secs(60));

                // Send periodic tasks
                if let Err(e) = periodic_task_tx.send(RuntimeTask::PeriodicAudit) {
                    eprintln!("[PeriodicThread] Failed to send PeriodicAudit task: {}", e);
                }

                if let Err(e) = periodic_task_tx.send(RuntimeTask::PeriodicEvidenceCollection) {
                    eprintln!(
                        "[PeriodicThread] Failed to send PeriodicEvidenceCollection task: {}",
                        e
                    );
                }
            }

            println!("[PeriodicThread] Exiting");
        });

        loop {
            // Check for shutdown
            if let Ok(_) = self.shutdown_rx.try_recv() {
                let _ = periodic_shutdown_tx.send(true); // propagate the shutdown
                break;
            }

            match self.task_rx.recv() {
                // Note: No timeout
                Ok(task) => {
                    if let Err(e) = self.handle_task(task) {
                        eprintln!("[Runtime] Error handling task: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("[Runtime] Error on the task channel: {}", e);
                    break;
                }
            }

            // Check for timed-out operations
            // Note: for ownership reason the check is after a receive msg
            if let Err(e) = self.check_timeouts() {
                eprintln!("[Runtime] Error checking timeouts: {}", e);
            }
        }

        println!("[Runtime] PeerReview runtime stopped");

        periodic_handle.join().unwrap();

        Ok(())
    }

    /// Handle incoming tasks
    fn handle_task(&mut self, task: RuntimeTask) -> std::io::Result<()> {
        match task {
            RuntimeTask::SendMessage { dest, msg } => {
                self.handle_send_message(dest, msg)?;
            }

            RuntimeTask::ReceiveMessage => {
                let (sender, msg) = self.node.recv()?;
                self.handle_receive_message(sender, msg)?;
            }

            RuntimeTask::PeriodicAudit => {
                self.handle_periodic_audit()?;
            }

            // Note: now trigger by threshold
            // RuntimeTask::PeriodicConsistency => {
            //     self.handle_periodic_consistency()?;
            // }
            RuntimeTask::PeriodicEvidenceCollection => {
                self.handle_periodic_evidence_collection()?;
            } // RuntimeTask::Shutdown => {
              //     // Handled in main loop
              // }
        }

        Ok(())
    }

    /// Handle sending a message
    fn handle_send_message(&mut self, dest: NodeId, msg: String) -> std::io::Result<()> {
        println!("[Runtime] Sending message to {}: {}", dest, msg);

        let send_msg = self.node.send_message(dest, &msg)?;

        self.pending_operations.push(PendingOperation {
            peer_id: dest,
            challenge_key: None,
            original_msg: Some(send_msg),
            created_time: Instant::now(),
            operation_type: OperationType::MessageSend,
        });

        Ok(())
    }

    /// Handle receiving a PeerReview protocol message
    fn handle_receive_message(
        &mut self,
        sender: NodeId,
        msg: PeerReviewMsg,
    ) -> std::io::Result<()> {
        match msg {
            // Commitment Protocol
            PeerReviewMsg::Send(ref send_msg) => {
                println!("[Runtime] Received SEND from {}", sender);
                // TODO return challenge id ack
                self.node.recv_message(sender, &msg)?;
            }

            PeerReviewMsg::Ack(ref ack_msg) => {
                println!("[Runtime] Received ACK from {}", sender);
                // TODO: Track original send messages to pass here
                if !self.node.verify_ack(sender, &msg) {
                    // TODO something
                }

                // TODO need id for the operation to remove
                self.remove_pending_operation(sender, OperationType::MessageSend);
            }

            // Consistency Protocol
            PeerReviewMsg::AuthenticatorBroadcast(ref auth_broadcast) => {
                println!(
                    "[Runtime] Received authenticator broadcast for node {}",
                    auth_broadcast.auth_node
                );

                // Store authenticator as a witness
                let threshold_exceeded = self
                    .node
                    .store_authenticator(auth_broadcast.auth_node, auth_broadcast.auth.clone());

                // If threshold exceeded, trigger consistency challenge
                if threshold_exceeded {
                    println!(
                        "[Runtime] Authenticator threshold exceeded for {}, triggering challenge",
                        auth_broadcast.auth_node
                    );
                    // TODO track the send msg
                    self.node
                        .send_consistency_challenge(auth_broadcast.auth_node)?;

                    self.pending_operations.push(PendingOperation {
                        peer_id: auth_broadcast.auth_node,
                        challenge_key: None,
                        original_msg: None, // TODO change it
                        created_time: Instant::now(),
                        operation_type: OperationType::ConsistencyCheck,
                    });
                }
            }

            PeerReviewMsg::ConsistencyRequest(ref req) => {
                println!("[Runtime] Received consistency request from {}", sender);
                self.node.recv_consistency_challenge(sender, &msg)?;
            }

            PeerReviewMsg::ConsistencyResponse(ref resp) => {
                println!("[Runtime] Received consistency response from {}", sender);
                // TODO get the send response
                if self.node.verify_consistency_response(sender, &msg) {
                    // TODO
                }

                // TODO remove the corresponing
                self.remove_pending_operation(sender, OperationType::ConsistencyCheck);
            }

            // Audit Protocol
            PeerReviewMsg::AuditRequest(ref req) => {
                println!("[Runtime] Received audit request from {}", sender);
                self.node.recv_audit_request(sender, &msg)?;
            }

            PeerReviewMsg::AuditResponse(ref resp) => {
                println!("[Runtime] Received audit response from {}", sender);
                // TODO handle error for other actions
                self.node.recv_audit_response(sender, &msg)?;

                // TODO Modify the operation type
                self.remove_pending_operation(sender, OperationType::AuditCheck);
            }

            // Challenge Protocol
            PeerReviewMsg::ChallengeRequest(ref challenge_req) => {
                println!(
                    "[Runtime] Received challenge request from {} for node {}",
                    sender, challenge_req.challenge.target
                );

                // If we're the target, handle the challenge
                if challenge_req.challenge.target == self.node.id {
                    self.node.recv_challenge(sender, &msg)?;
                } else {
                    // If we're a witness, forward the challenge
                    self.node
                        .witnesses_recv_challenge(challenge_req.challenge.clone())?;
                }
            }

            PeerReviewMsg::ChallengeResponse(ref response) => {
                println!("[Runtime] Received challenge response from {}", sender);
                // TODO
                if let Some(challenge) = self.node.get_challenge(sender, response.challenge_key) {
                    self.node.recv_challenge_response(sender, &msg);
                }

                // TODO
                self.remove_pending_operation(sender, OperationType::ChallengeSend);
            }

            // Evidence Transfer Protocol
            PeerReviewMsg::EvidenceRequest(ref req) => {
                println!(
                    "[Runtime] Received evidence request from {} about node {}",
                    sender, req.target
                );
                self.node.handle_evidence_request(sender, req)?;
            }

            PeerReviewMsg::EvidenceResponse(ref resp) => {
                println!(
                    "[Runtime] Received evidence response about node {}",
                    resp.target
                );
                self.node.handle_evidence_response(sender, resp)?;

                // TODO
                self.remove_pending_operation(resp.target, OperationType::EvidenceRequest);
            }

            PeerReviewMsg::ProofBroadcast(ref proof_broadcast) => {
                println!(
                    "[Runtime] Received proof broadcast: {} accuses {}",
                    proof_broadcast.proof.accuser_node, proof_broadcast.proof.faulty_node
                );

                self.node
                    .recv_exposure_proof(sender, &proof_broadcast.proof);
            }
        }

        Ok(())
    }

    /// Periodic audit of peers
    fn handle_periodic_audit(&mut self) -> std::io::Result<()> {
        println!("[Runtime] Running periodic audit");

        // TODO add children field for witnesses
        let peer_ids: Vec<NodeId> = self.node.peers.keys().copied().collect();
        for peer_id in peer_ids {
            if !self.node.is_witness(peer_id) {
                continue;
            }

            let last_seq = self.node.get_peer_last_audit_seq(peer_id).unwrap_or(0);

            // TODO add challenge corresponding
            self.node.send_audit_request(peer_id)?;

            println!("[Runtime] Audit peer {} from seq {}", peer_id, last_seq);
        }

        Ok(())
    }

    /// Periodic evidence collection from witnesses
    fn handle_periodic_evidence_collection(&mut self) -> std::io::Result<()> {
        println!("[Runtime] Running periodic evidence collection");

        // TODO add challenge corresponding
        self.node.periodic_evidence_collection()?;

        Ok(())
    }

    /// Check for timed-out operations
    fn check_timeouts(&mut self) -> std::io::Result<()> {
        let now = Instant::now();
        let timeout = Duration::from_secs(RESPONSE_TIMEOUT_SECS);

        // Find timed-out operations
        let timed_out: Vec<PendingOperation> = self
            .pending_operations
            .iter()
            .filter(|op| now.duration_since(op.created_time) > timeout)
            .cloned()
            .collect();

        // Handle timeouts
        for op in timed_out {
            println!(
                "[Runtime] Operation timed out for peer {}: {:?}",
                op.peer_id, op.operation_type
            );

            match op.operation_type {
                OperationType::MessageSend => {
                    // No response to message, send challenge to witnesses
                    println!("[Runtime] No ACK received from {}, challenging", op.peer_id);
                    // TODO save the pending message and add id for operation
                    // extract original pr_msg from operation and provide the good msg and auth
                    let pr_msg = op.original_msg.unwrap();
                    let send_msg = match pr_msg {
                        PeerReviewMsg::Send(payload) => payload,
                        _ => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid msg type",
                            ));
                        }
                    };

                    let sender_auth = types::messages::Authenticator {
                        seq: send_msg.seq,
                        hash: protocols::commitment::calculate_hash(
                            send_msg.prev_hash,
                            send_msg.seq,
                            journal::entry::LogType::Send,
                            protocols::commitment::calculate_send_content_hash(
                                send_msg.dest,
                                &send_msg.msg,
                            ),
                        ),
                        sig: send_msg.sig,
                    };

                    self.node
                        .create_send_challenge(op.peer_id, send_msg, sender_auth)?; // TODO fix
                }

                OperationType::ChallengeSend => {
                    // No response to challenge, mark as suspected indefinitely
                    println!("[Runtime] No response to challenge for {}", op.peer_id);
                    self.node
                        .set_peer_status(op.peer_id, types::PeerStatus::Suspected);
                }

                // TODO Handle other kind of non reponse
                _ => {}
            }

            // TODO change by operation id contains in peerreview msg
            // Remove the timed-out operation
            self.pending_operations.retain(|p| {
                !(p.peer_id == op.peer_id
                    && std::mem::discriminant(&p.operation_type)
                        == std::mem::discriminant(&op.operation_type))
            });
        }

        Ok(())
    }

    /// Remove a pending operation
    fn remove_pending_operation(&mut self, peer_id: NodeId, op_type: OperationType) {
        // TODO change by operation id contains in peerreview msg
        self.pending_operations.retain(|op| {
            !(op.peer_id == peer_id
                && std::mem::discriminant(&op.operation_type) == std::mem::discriminant(&op_type))
        });
    }

    fn load_keypair(path: &str) -> Keypair {
        let key_bytes = fs::read(path).expect("Failed to read key");
        let secret = SecretKey::from_bytes(&key_bytes).expect("Invalid secret key");
        let public = PublicKey::from(&secret);
        Keypair { secret, public }
    }
}
