use anyhow::{anyhow, Result};
use clap::Parser;
use peerreview_protocol::journal::entry::LogEntry;
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

    let mut sends: HashSet<Triple> = HashSet::new();
    let mut recvs: HashSet<Triple> = HashSet::new();
    let mut delivers: HashSet<Triple> = HashSet::new();

    for (node, path) in &args.log {
        let f = File::open(path)?;
        for line in BufReader::new(f).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let e = LogEntry::from_json_line(&line)?;
            let kind = e.kind.clone();
            let kv = parse_payload_kv(&e.payload);

            if kind == "SEND" || kind == "RECV" || kind == "DELIVER" || kind == "DROP" {
                let msg_id = kv.get("msg_id").cloned().unwrap_or_default();
                let from = kv.get("from").cloned().unwrap_or_default();
                let to = kv.get("to").cloned().unwrap_or_default();

                if msg_id.is_empty() || from.is_empty() || to.is_empty() {
                    continue;
                }

                let t = Triple { from, to, msg_id };

                match kind.as_str() {
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

            // keep node referenced (avoid unused warning)
            let _ = node;
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
