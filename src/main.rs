use std::env;
use std::thread;
use std::time::Duration;

use peerreview_rust::types::Config;
use peerreview_rust::{PeerReviewRuntime, RuntimeTask};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <node_config.toml>", args[0]);
        std::process::exit(1);
    }
    let config_file = &args[1];
    let config = Config::from_file(config_file)?;

    // Init peerreview runtime
    let runtime = PeerReviewRuntime::new(config_file).unwrap();

    // PeerReview channels
    let task_sender = runtime.get_task_sender();
    let shutdown_sender = runtime.get_shutdown_sender();

    // Spawn runtime in separate thread
    let runtime_handle = thread::spawn(move || {
        runtime.run().expect("Runtime failed");
    });

    thread::sleep(Duration::from_secs(10));

    // Application logic
    thread::spawn(move || {
        // Send a message
        task_sender
            .send(RuntimeTask::SendMessage {
                dest: if config.node.id == 1 { 2 } else { 1 },
                msg: "Hello, peer!".to_string(),
            })
            .unwrap();

        // Wait a bit
        thread::sleep(Duration::from_secs(5));

        // Shutdown
        shutdown_sender.send(true).unwrap();
    });

    // Wait for runtime to complete
    runtime_handle.join().unwrap();

    Ok(())
}
