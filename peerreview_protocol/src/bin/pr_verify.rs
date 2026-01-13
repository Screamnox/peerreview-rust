use clap::Parser;
use ed25519_dalek::VerifyingKey;
use peerreview_protocol::journal::logger::Logger;

#[derive(Parser, Debug)]
#[command(about = "Verify a single PR log file")]
struct Args {
    /// Path to a log file (jsonl)
    #[arg(long)]
    log: String,

    /// Verifying key in hex (32 bytes)
    #[arg(long)]
    pubkey: String,

    /// Enforce prev_hash chaining strictly
    #[arg(long, default_value_t = false)]
    strict_chain: bool,
}

fn main() {
    let args = Args::parse();

    let pk_bytes = match hex::decode(args.pubkey.trim()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("FAULT: pubkey hex decode failed: {e}");
            std::process::exit(2);
        }
    };
    if pk_bytes.len() != 32 {
        eprintln!("FAULT: pubkey must be 32 bytes, got {}", pk_bytes.len());
        std::process::exit(2);
    }
    let mut pk32 = [0u8; 32];
    pk32.copy_from_slice(&pk_bytes);
    let vk = VerifyingKey::from_bytes(&pk32).unwrap();

    match Logger::verify_log_file(&args.log, &vk, args.strict_chain) {
        Ok(()) => {
            println!("OK: log {} verified successfully", args.log);
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("FAULT: {}", e);
            std::process::exit(2);
        }
    }
}
