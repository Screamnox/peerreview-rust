use clap::Parser;
use ed25519_dalek::VerifyingKey;
use peerreview_protocol::journal::entry::LogEntry;
use peerreview_protocol::journal::logger::Logger;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Parser, Debug)]
#[command(about = "Verify a cluster of logs (cross-check SEND/RECV/DELIVER + fork detection)")]
struct Args {
    /// Repeated: node_id=pubkeyhex32
    /// Example: --node-key node10=014e... --node-key node1=ab12...
    #[arg(long = "node-key")]
    node_keys: Vec<String>,

    /// Repeated log path for that node
    /// Example: --log node10:/tmp/node10.log --log node1:/tmp/node1.log
    #[arg(long)]
    log: Vec<String>,

    /// Strict prev_hash chain inside each file
    #[arg(long, default_value_t = true)]
    strict_chain: bool,
}

fn parse_node_key(s: &str) -> Result<(String, VerifyingKey), String> {
    let (node, hexpk) = s.split_once('=').ok_or("expected node=pubkeyhex")?;
    let bytes = hex::decode(hexpk.trim()).map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err(format!("pubkey must be 32 bytes, got {}", bytes.len()));
    }
    let mut b32 = [0u8; 32];
    b32.copy_from_slice(&bytes);
    Ok((node.to_string(), VerifyingKey::from_bytes(&b32).unwrap()))
}

fn parse_log_arg(s: &str) -> Result<(String, String), String> {
    let (node, path) = s.split_once(':').ok_or("expected node:/path/to/log")?;
    Ok((node.to_string(), path.to_string()))
}

fn read_entries(path: &str) -> Result<Vec<LogEntry>, String> {
    let f = File::open(path).map_err(|e| e.to_string())?;
    let r = BufReader::new(f);
    let mut out = vec![];
    for line in r.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(LogEntry::from_json_line(&line).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn extract_msg_id(payload: &str) -> Option<String> {
    // payload: "msg_id=... peer=... ts_ms=... hash=..."
    payload
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix("msg_id=").map(|v| v.to_string()))
}

fn main() {
    let args = Args::parse();

    // node -> verifying key
    let mut keys: HashMap<String, VerifyingKey> = HashMap::new();
    for nk in &args.node_keys {
        match parse_node_key(nk) {
            Ok((n, vk)) => {
                keys.insert(n, vk);
            }
            Err(e) => {
                eprintln!("FAULT: bad --node-key {nk}: {e}");
                std::process::exit(2);
            }
        }
    }

    // node -> list of logs
    let mut logs_by_node: HashMap<String, Vec<String>> = HashMap::new();
    for l in &args.log {
        match parse_log_arg(l) {
            Ok((n, p)) => logs_by_node.entry(n).or_default().push(p),
            Err(e) => {
                eprintln!("FAULT: bad --log {l}: {e}");
                std::process::exit(2);
            }
        }
    }

    // 1) verify each log file with its pubkey
    for (node, paths) in &logs_by_node {
        let vk = match keys.get(node) {
            Some(v) => v,
            None => {
                eprintln!("FAULT: missing --node-key for node {node}");
                std::process::exit(2);
            }
        };
        for p in paths {
            if let Err(e) = Logger::verify_log_file(p, vk, args.strict_chain) {
                eprintln!("FAULT: node={node} log={p} failed local verify: {e}");
                std::process::exit(2);
            }
        }
    }

    // 2) fork detection: if a node has multiple logs, check if they share same prefix chain.
    // We detect fork if:
    // - both verify individually BUT have different head hash OR diverge at same seq with different hash.
    for (node, paths) in &logs_by_node {
        if paths.len() <= 1 {
            continue;
        }
        let mut chains: Vec<Vec<[u8; 32]>> = vec![];
        for p in paths {
            let entries = read_entries(p).unwrap();
            let mut c = vec![];
            for e in &entries {
                c.push(e.hash_bytes_32().unwrap());
            }
            chains.push(c);
        }

        // compare all pairs
        for i in 0..chains.len() {
            for j in (i + 1)..chains.len() {
                let a = &chains[i];
                let b = &chains[j];
                let min = a.len().min(b.len());
                let mut diverged = false;
                for k in 0..min {
                    if a[k] != b[k] {
                        diverged = true;
                        break;
                    }
                }
                if diverged || (a.len() != b.len() && a[min - 1] != b[min - 1]) {
                    eprintln!("FAULT: fork detected for node={node} (multiple valid logs diverge)");
                    std::process::exit(2);
                }
            }
        }
    }

    // 3) cross-check SEND/RECV/DELIVER using msg_id
    // Map msg_id -> {senders, receivers, deliverers}
    let mut sends: HashMap<String, HashSet<String>> = HashMap::new();
    let mut recvs: HashMap<String, HashSet<String>> = HashMap::new();
    let mut delivers: HashMap<String, HashSet<String>> = HashMap::new();

    // Also track "who logged what kind for msg_id"
    for (node, paths) in &logs_by_node {
        for p in paths {
            let entries = read_entries(p).unwrap();
            for e in &entries {
                if !(e.kind == "SEND" || e.kind == "RECV" || e.kind == "DELIVER") {
                    continue;
                }
                let mid = match extract_msg_id(&e.payload) {
                    Some(m) => m,
                    None => continue,
                };
                match e.kind.as_str() {
                    "SEND" => {
                        sends.entry(mid).or_default().insert(node.clone());
                    }
                    "RECV" => {
                        recvs.entry(mid).or_default().insert(node.clone());
                    }
                    "DELIVER" => {
                        delivers.entry(mid).or_default().insert(node.clone());
                    }
                    _ => {}
                }
            }
        }
    }

    // Expected: if a msg_id exists, at least one SEND should exist.
    // If DELIVER exists on some node, then RECV should exist on that same node (or the node is the originator).
    // We approximate originator if it has SEND for that msg_id.
    for (mid, del_nodes) in &delivers {
        for n in del_nodes {
            let is_originator = sends.get(mid).map(|hs| hs.contains(n)).unwrap_or(false);
            let has_recv = recvs.get(mid).map(|hs| hs.contains(n)).unwrap_or(false);

            if !is_originator && !has_recv {
                eprintln!("FAULT: message {mid} delivered on {n} without RECV and not originator (omission or log manipulation)");
                std::process::exit(2);
            }
        }
    }

    // Omission detection: If SEND exists from i, we expect at least one RECV somewhere (not perfect without routing graph,
    // but catches “send logged but nobody received”).
    for (mid, senders) in &sends {
        if !recvs.contains_key(mid) {
            eprintln!("FAULT: message {mid} has SEND by {:?} but no RECV anywhere (omission/crash)", senders);
            std::process::exit(2);
        }
    }

    // If RECV exists, DELIVER is optional (depends on app), but we can still report.
    println!("OK: cluster logs verified (local sig+chain) + cross-check passed");
    std::process::exit(0);
}
