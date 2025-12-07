use common_proto::{AppMsg, AppMsgKind, BinaryChunk};
use std::fs::File;
use std::io::{Read, BufReader};

pub fn send_file_as_chunks(
    path: &str,
    file_id: &str,
    tree_id: u8,
    node_id: &str,
    now_ms: impl Fn() -> u64,
    send_app_msg: impl Fn(AppMsg) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let chunk_size: usize = 64 * 1024;
    let mut buf = vec![0u8; chunk_size];
    let mut chunks: Vec<Vec<u8>> = Vec::new();

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        chunks.push(buf[..n].to_vec());
    }
    let total_chunks = chunks.len() as u32;

    for (i, data) in chunks.into_iter().enumerate() {
        let chunk = BinaryChunk {
            file_id: file_id.to_string(),
            index: i as u32,
            total_chunks,
            data,
        };

        let payload = bincode::serialize(&chunk)?;
        let msg = AppMsg {
            kind: AppMsgKind::BinaryChunk,
            tree_id,
            from: node_id.to_string(),
            ts_ms: now_ms(),
            payload,
        };

        send_app_msg(msg)?;
    }

    Ok(())
}
