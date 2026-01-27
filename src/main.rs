use std::env;
use std::thread;
use std::time::Duration;

use overlay::node::OverlayNode;
use overlay::payload::OverlayPayload;
use overlay::tree::build_trees;

use peerreview_rust::PeerReviewRuntime;
use peerreview_rust::types::Config;
use peerreview_rust::types::node::NodeId;

mod overlay;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        eprintln!(
            "Usage: {} <node_config.toml> <num_nodes>",
            args[0]
        );
        std::process::exit(1);
    }

    let config_path = &args[1];
    let config = Config::from_file(config_path).unwrap();

    let num_nodes: usize = args[2]
        .parse()
        .expect("num_nodes must be an integer");

    // --- Start PeerReview runtime ---
    let (runtime, app_rx) = PeerReviewRuntime::new(config_path)?;
    let runtime_tx = runtime.get_task_sender();

    let pr_thread = thread::spawn(move || {
        runtime.run().expect("PeerReview runtime crashed");
    });

    // --- Build overlay membership ---
    let nodes: Vec<NodeId> = (1..=num_nodes as NodeId).collect();
    let overlay = build_trees(&nodes, 10);

    let self_id = config.node.id;

    let mut overlay_node = OverlayNode::new(
        self_id,
        overlay.clone(),
        runtime_tx,
        app_rx,
    );

    // --- Source behavior (node 1 only) ---
    if self_id == 1 {
        for chunk_id in 0..20 {
            let tree = (chunk_id % 10) as usize;

            let payload = OverlayPayload::Chunk {
                chunk_id,
                tree,
                data: "Hello".as_bytes().to_vec(),
            };

            for &child in overlay.children(tree, self_id) {
                overlay_node.send_chunk(child, payload.clone());
            }

            thread::sleep(Duration::from_millis(500));
            overlay_node.poll_peerreview();
        }
    }

    // --- All nodes process incoming messages ---
    loop {
        overlay_node.poll_peerreview();
        thread::sleep(Duration::from_millis(100));
    }
}
