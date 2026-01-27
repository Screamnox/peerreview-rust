use std::collections::{HashSet};
use std::sync::mpsc::{Sender, Receiver};

use base64::{engine::general_purpose, Engine as _};

use crate::overlay::payload::*;
use peerreview_rust::{
    RuntimeTask,
    ApplicationEvent,
};
use super::tree::Overlay;

use peerreview_rust::types::node::NodeId;

pub struct OverlayNode {
    pub id: NodeId,

    /// Send commands to PeerReview
    runtime_tx: Sender<RuntimeTask>,

    /// Receive application callbacks from PeerReview
    app_rx: Receiver<ApplicationEvent>,

    overlay: Overlay,

    delivered: HashSet<ChunkId>,
    exposed_peers: HashSet<NodeId>,
}

impl OverlayNode {
    pub fn new(
        id: NodeId,
        overlay: Overlay,
        runtime_tx: Sender<RuntimeTask>,
        app_rx: Receiver<ApplicationEvent>,
    ) -> Self {
        Self {
            id,
            overlay,
            runtime_tx,
            app_rx,
            delivered: HashSet::new(),
            exposed_peers: HashSet::new(),
        }
    }

    /* ---------------- Sending ---------------- */

    pub fn send_chunk(&self, dest: NodeId, payload: OverlayPayload) {
        if self.exposed_peers.contains(&dest) {
            println!(
                "[Overlay {}] Not sending to exposed peer {}",
                self.id, dest
            );
            return;
        }

        let bytes = encode(&payload);
        let msg = general_purpose::STANDARD.encode(bytes);

        self.runtime_tx
            .send(RuntimeTask::SendMessage { dest, msg })
            .expect("send to PeerReview failed");
    }

    /* ---------------- Receiving ---------------- */

    pub fn poll_peerreview(&mut self) {
        while let Ok(event) = self.app_rx.try_recv() {
            match event {
                ApplicationEvent::Message { from, payload } => {
                    let bytes = general_purpose::STANDARD
                        .decode(payload)
                        .expect("base64 decode failed");

                    let msg = decode(&bytes);
                    self.handle_overlay_message(from, msg);
                }

                ApplicationEvent::PeerExposed { peer } => {
                    println!(
                        "[Overlay {}] Peer {} exposed — blacklisted",
                        self.id, peer
                    );
                    self.exposed_peers.insert(peer);
                }
            }
        }
    }

    fn handle_overlay_message(&mut self, from: NodeId, msg: OverlayPayload) {
        match msg {
            OverlayPayload::Chunk {
                chunk_id,
                tree,
                data,
            } => {
                if self.delivered.contains(&chunk_id) {
                    return;
                }

                println!(
                    "[Overlay {}] Received chunk {} from {} (tree {})",
                    self.id, chunk_id, from, tree
                );

                self.delivered.insert(chunk_id);

                // Forward AFTER local delivery
                for &child in self.overlay.children(tree, self.id) {
                    self.send_chunk(
                        child,
                        OverlayPayload::Chunk {
                            chunk_id,
                            tree,
                            data: data.clone(),
                        },
                    );
                }
            }
        }
    }
}
