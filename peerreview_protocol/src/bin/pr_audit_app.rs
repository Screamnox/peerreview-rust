use anyhow::{anyhow, Result};
use clap::Parser;
use peerreview_protocol::journal::entry::LogEntry;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Parser, Debug)]
struct Args {
    /// repeatable: --log node4=/tmp/prlogs/node4.log
    #[arg(long, value_parser = parse_kv, num_args=1..)]
    log: Vec<(String, String)>,

    /// if set: require DELIVER too (stricter)
    #[arg(long, default_value_t = false)]
    require_deliver: bool,

    /// if set: verify deterministic replay using STATE_HASH/CHECKPOINT if present
    #[arg(long, default_value_t = true)]
    check_replay: bool,
}

fn parse_kv(s: &str) -> std::result::Result<(String, String), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| "expected node=path".to_string())?;
    Ok((k.to_string(), v.to_string()))
}

fn parse_payload_kv(payload: &str) -> HashMap<String, String> {
    // very simple "k=v" tokens separated by spaces
    let mut m = HashMap::new();
    for tok in payload.split_whitespace() {
        if let Some((k, v)) = tok.split_once('=') {
            m.insert(k.to_string(), v.to_string());
        }
    }
    m
}

fn sha256_state(prev: [u8; 32], msg_id: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(prev);
    hasher.update(msg_id.as_bytes());
    let out = hasher.finalize();
    let mut h = [0u8; 32];
    h.copy_from_slice(&out[..]);
    h
}

fn replay_check(node: &str, entries: &[LogEntry]) -> Result<()> {
    let mut h = [0u8; 32];
    let mut deliver_idx: u64 = 0;
    let mut saw_state = false;

    for e in entries {
        let kind = e.kind.as_str();
        let kv = parse_payload_kv(&e.payload);

        match kind {
            "DELIVER" => {
                let msg_id = kv.get("msg_id").cloned().unwrap_or_default();
                if msg_id.is_empty() {
                    continue;
                }
                deliver_idx += 1;
                h = sha256_state(h, &msg_id);
            }
            "STATE_HASH" | "CHECKPOINT" => {
                saw_state = true;
                let idx_s = kv
                    .get("deliver_idx")
                    .ok_or_else(|| anyhow!("{kind} missing deliver_idx"))?;
                let idx: u64 = idx_s
                    .parse()
                    .map_err(|_| anyhow!("{kind} bad deliver_idx={idx_s}"))?;

                let hex_h = kv
                    .get("state_hash")
                    .ok_or_else(|| anyhow!("{kind} missing state_hash"))?;
                let got = hex::decode(hex_h)
                    .map_err(|_| anyhow!("{kind} bad state_hash hex"))?;
                if got.len() != 32 {
                    return Err(anyhow!("{kind} bad state_hash len={}", got.len()));
                }

                // We log STATE_HASH after DELIVER, so idx must match current deliver_idx.
                if idx != deliver_idx {
                    return Err(anyhow!(
                        "INVALID REPLAY idx mismatch: kind={} idx={} current_deliver_idx={} (node={})",
                        kind,
                        idx,
                        deliver_idx,
                        node
                    ));
                }

                if got.as_slice() != h {
                    return Err(anyhow!(
                        "INVALID REPLAY hash mismatch at deliver_idx={} (node={})",
                        deliver_idx,
                        node
                    ));
                }
            }
            _ => {}
        }
    }

    if saw_state {
        println!(
            "REPLAY: OK node={} (deliver_count={}, final_state_hash={})",
            node,
            deliver_idx,
            hex::encode(h)
        );
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Triple {
    from: String,
    to: String,
    msg_id: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.log.len() < 2 {
        return Err(anyhow!("need at least 2 logs"));
    }

    // Load per-node entries (keep order)
    let mut per_node: HashMap<String, Vec<LogEntry>> = HashMap::new();
    for (node, path) in &args.log {
        let f = File::open(path)?;
        let mut v = Vec::new();
        for line in BufReader::new(f).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let e = LogEntry::from_json_line(&line)?;
            v.push(e);
        }
        v.sort_by_key(|e| e.seq);
        per_node.insert(node.clone(), v);
    }

    // Optional: deterministic replay verification (if STATE_HASH/CHECKPOINT exist)
    if args.check_replay {
        for (node, entries) in &per_node {
            if let Err(e) = replay_check(node, entries) {
                println!(
                    "VERDICT: FAULT (INVALID STATE REPLAY) node={} reason={}",
                    node, e
                );
                return Ok(());
            }
        }
    }

    // Cross-log correlation: SEND <-> RECV <-> DELIVER
    let mut sends: HashSet<Triple> = HashSet::new();
    let mut recvs: HashSet<Triple> = HashSet::new();
    let mut delivers: HashSet<Triple> = HashSet::new();

    for (_node, entries) in &per_node {
        for e in entries {
            let kind = e.kind.as_str();
            let kv = parse_payload_kv(&e.payload);

            if kind == "SEND" || kind == "RECV" || kind == "DELIVER" || kind == "DROP" {
                let msg_id = kv.get("msg_id").cloned().unwrap_or_default();
                let from = kv.get("from").cloned().unwrap_or_default();
                let to = kv.get("to").cloned().unwrap_or_default();

                if msg_id.is_empty() || from.is_empty() || to.is_empty() {
                    continue;
                }

                let t = Triple { from, to, msg_id };
                match kind {
                    "SEND" => {
                        sends.insert(t);
                    }
                    "RECV" => {
                        recvs.insert(t);
                    }
                    "DELIVER" => {
                        delivers.insert(t);
                    }
                    _ => {}
                }
            }
        }
    }

    // invariant 1: every SEND must have matching RECV
    for t in &sends {
        if !recvs.contains(t) {
            println!(
                "VERDICT: FAULT (OMISSION/DROP) missing RECV for SEND: from={} to={} msg_id={}",
                t.from, t.to, t.msg_id
            );
            return Ok(());
        }
        if args.require_deliver && !delivers.contains(t) {
            println!(
                "VERDICT: FAULT (OMISSION) missing DELIVER for SEND: from={} to={} msg_id={}",
                t.from, t.to, t.msg_id
            );
            return Ok(());
        }
    }

    // invariant 2: every RECV must have matching SEND
    for t in &recvs {
        if !sends.contains(t) {
            println!(
                "VERDICT: FAULT (INVENTED MESSAGE) RECV without SEND: from={} to={} msg_id={}",
                t.from, t.to, t.msg_id
            );
            return Ok(());
        }
    }

    println!(
        "VERDICT: OK ({} sends, {} recvs, {} delivers)",
        sends.len(),
        recvs.len(),
        delivers.len()
    );
    Ok(())
}
