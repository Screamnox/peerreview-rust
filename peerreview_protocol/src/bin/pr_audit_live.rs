use anyhow::{anyhow, Context, Result};
use clap::Parser;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use peerreview_protocol::journal::entry::LogEntry;
use peerreview_protocol::types::config::ClusterConfig;
use peerreview_protocol::types::messages::{Commitment, Evidence, PeerReviewMsg};
use peerreview_protocol::types::node::verifying_key_from_name;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Encode PR msg over TCP: [u32 len][bincode payload]
async fn pr_send(stream: &mut TcpStream, msg: &PeerReviewMsg) -> Result<()> {
    let payload = bincode::encode_to_vec(msg, bincode::config::standard())
        .context("bincode encode PR")?;
    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&payload).await?;
    Ok(())
}

async fn pr_recv(stream: &mut TcpStream) -> Result<PeerReviewMsg> {
    let mut lenb = [0u8; 4];
    stream.read_exact(&mut lenb).await?;
    let len = u32::from_be_bytes(lenb) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let (msg, _) =
        bincode::decode_from_slice::<PeerReviewMsg, _>(&buf, bincode::config::standard())
            .context("bincode decode PR")?;
    Ok(msg)
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "configs/docker/cluster.yaml")]
    cluster: String,

    /// Suspect node name, ex: node10
    #[arg(long)]
    suspect: String,

    /// Timeout for one request (ms)
    #[arg(long, default_value_t = 1500)]
    timeout_ms: u64,
}

fn pick_low_high(commits: &[Commitment]) -> Option<(Commitment, Commitment)> {
    if commits.len() < 2 {
        return None;
    }
    let mut v = commits.to_vec();
    v.sort_by_key(|c| c.seq);
    Some((v.first().cloned().unwrap(), v.last().cloned().unwrap()))
}

fn verify_equivocation(commits: &[Commitment]) -> Option<Evidence> {
    use std::collections::HashMap;
    let mut m: HashMap<u64, [u8; 32]> = HashMap::new();
    for c in commits {
        if let Some(prev) = m.get(&c.seq) {
            if prev != &c.head_hash {
                return Some(Evidence::Equivocation {
                    node_id: c.node_id,
                    seq: c.seq,
                    head_a: *prev,
                    head_b: c.head_hash,
                });
            }
        } else {
            m.insert(c.seq, c.head_hash);
        }
    }
    None
}

/// Verify log segment against authenticator `a_to`.
/// We do NOT rely on LogEntry::verify_sig (doesn't exist).
fn verify_log_segment(
    lines: &[String],
    from_seq: u64,
    to_seq: u64,
    a_to: &Commitment,
    vk: &VerifyingKey,
) -> Result<()> {
    if lines.is_empty() {
        return Err(anyhow!("empty log slice"));
    }

    // Parse entries
    let mut entries = Vec::new();
    for l in lines {
        let e = LogEntry::from_json_line(l)
            .map_err(|e| anyhow!("bad json log entry: {e}"))?;
        entries.push(e);
    }

    // seq coverage check (best effort)
    let min_seq = entries.iter().map(|e| e.seq).min().unwrap();
    let max_seq = entries.iter().map(|e| e.seq).max().unwrap();
    if min_seq > from_seq || max_seq < to_seq {
        return Err(anyhow!(
            "slice does not cover requested range: got [{min_seq},{max_seq}] want [{from_seq},{to_seq}]"
        ));
    }

    // Build map by seq
    let mut by_seq: std::collections::BTreeMap<u64, LogEntry> = std::collections::BTreeMap::new();
    for e in entries {
        by_seq.insert(e.seq, e);
    }

    // Verify signature + chain from from_seq..to_seq
    // We reconstruct hash chain using prev_hash fields inside entries.
    let first = by_seq
        .get(&from_seq)
        .ok_or_else(|| anyhow!("missing from_seq entry in slice"))?;

    let mut cur_prev = first
        .prev_hash_bytes_32()
        .map_err(|e| anyhow!("bad prev_hash at from_seq: {e}"))?;

    for s in from_seq..=to_seq {
        let e = by_seq.get(&s).ok_or_else(|| anyhow!("missing seq {s} in slice"))?;

        // verify sig over entry.hash
        let hash = e.hash_bytes_32().map_err(|er| anyhow!("{er}"))?;
        let sig64 = e.sig_bytes_64().map_err(|er| anyhow!("{er}"))?;
        let sig = Signature::from_bytes(&sig64);
        vk.verify(&hash, &sig)
            .map_err(|er| anyhow!("entry sig invalid seq={}: {er}", e.seq))?;

        // verify prev_hash chain
        let prev = e.prev_hash_bytes_32().map_err(|er| anyhow!("{er}"))?;
        if prev != cur_prev {
            return Err(anyhow!(
                "hash-chain break at seq {s}: expected prev {} got {}",
                hex::encode(cur_prev),
                hex::encode(prev)
            ));
        }
        cur_prev = hash;
    }

    // Final head must match authenticator a_to.head_hash
    if cur_prev != a_to.head_hash {
        return Err(anyhow!(
            "final head mismatch: computed {} but authenticator says {}",
            hex::encode(cur_prev),
            hex::encode(a_to.head_hash)
        ));
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let cfg = ClusterConfig::from_yaml_file(&args.cluster)?;

    let suspect = cfg
        .find_by_name(&args.suspect)
        .ok_or_else(|| anyhow!("unknown suspect {}", args.suspect))?;
    let suspect_id = suspect.id;

    // witnesses (dedicated if configured)
    let witnesses = cfg.witnesses_for(suspect_id, 3);
    if witnesses.is_empty() {
        return Err(anyhow!("no witnesses configured for suspect"));
    }

    let suspect_vk = verifying_key_from_name(&args.suspect);

    // 1) Query witnesses for suspect commitments
    let mut all_commits: Vec<Commitment> = Vec::new();
    for w in &witnesses {
        let mut sock = TcpStream::connect(&w.pr_addr)
            .await
            .with_context(|| format!("connect witness {}", w.pr_addr))?;

        let q = PeerReviewMsg::WitnessQuery { node_id: suspect_id };
        timeout(Duration::from_millis(args.timeout_ms), pr_send(&mut sock, &q)).await??;

        let rep = timeout(Duration::from_millis(args.timeout_ms), pr_recv(&mut sock)).await??;
        match rep {
            PeerReviewMsg::WitnessReply { commits, .. } => {
                for c in commits {
                    // verify authenticator signature
                    c.verify(&suspect_vk)
                        .map_err(|e| anyhow!("bad commitment sig: {e}"))?;
                    all_commits.push(c);
                }
            }
            _ => return Err(anyhow!("unexpected reply from witness")),
        }
    }

    if all_commits.len() < 2 {
        println!(
            "VERDICT: SUSPECTED (need >=2 authenticators, got {})",
            all_commits.len()
        );
        return Ok(());
    }

    // 2) Equivocation check purely from witness view
    if let Some(ev) = verify_equivocation(&all_commits) {
        println!("VERDICT: FAULT (equivocation): {:?}", ev);
        return Ok(());
    }

    let (a_from, a_to) = pick_low_high(&all_commits).unwrap();
    let from_seq = a_from.seq.saturating_add(1);
    let to_seq = a_to.seq;

    // If witnesses only have one head (or no progress), we can't form a non-empty challenge range.
    // This is NOT a fault; it just means "insufficient interval".
    if to_seq < from_seq {
        println!(
            "VERDICT: OK (insufficient interval: from_seq={} to_seq={}, witnesses={})",
            from_seq,
            to_seq,
            witnesses.len()
        );
        return Ok(());
    }


    // 3) Challenge suspect for connecting segment
    let mut sock = TcpStream::connect(&suspect.pr_addr)
        .await
        .with_context(|| format!("connect suspect {}", suspect.pr_addr))?;

    let chall = PeerReviewMsg::Challenge {
        node_id: suspect_id,
        from_seq,
        to_seq,
        a_from: a_from.clone(),
        a_to: a_to.clone(),
    };

    // Send challenge (handle timeout cleanly)
    let send_ok =
        timeout(Duration::from_millis(args.timeout_ms), pr_send(&mut sock, &chall)).await;
    if send_ok.is_err() {
        let ev = Evidence::NoResponse {
            node_id: suspect_id,
            timeout_ms: args.timeout_ms,
        };
        println!("VERDICT: SUSPECTED {:?}", ev);
        return Ok(());
    }
    send_ok.unwrap()?; // <- Result<()> only

    // Receive response
    let rep = timeout(Duration::from_millis(args.timeout_ms), pr_recv(&mut sock)).await;
    if rep.is_err() {
        let ev = Evidence::NoResponse {
            node_id: suspect_id,
            timeout_ms: args.timeout_ms,
        };
        println!("VERDICT: SUSPECTED {:?}", ev);
        return Ok(());
    }

    match rep.unwrap()? {
        PeerReviewMsg::LogSlice { lines, .. } => {
            // Empty slice should not be a hard fault in a clean run; treat as "insufficient evidence".
            if lines.is_empty() {
                let ev = Evidence::NoResponse {
                    node_id: suspect_id,
                    timeout_ms: args.timeout_ms,
                };
                println!("VERDICT: SUSPECTED {:?}", ev);
                return Ok(());
            }
            if let Err(e) = verify_log_segment(&lines, from_seq, to_seq, &a_to, &suspect_vk) {
                let ev = Evidence::BadLogSegment {
                    node_id: suspect_id,
                    reason: e.to_string(),
                };
                println!("VERDICT: FAULT {:?}", ev);
                return Ok(());
            }
            println!(
                "VERDICT: OK (segment [{}..{}], witnesses={}, t={}ms)",
                from_seq,
                to_seq,
                witnesses.len(),
                now_ms()
            );
            Ok(())
        }
        other => Err(anyhow!("unexpected reply from suspect: {:?}", other)),
    }
}
