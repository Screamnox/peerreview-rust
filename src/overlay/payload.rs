use bincode::{Encode, Decode};

pub type ChunkId = u64;
pub type TreeId = usize;

#[derive(Debug, Clone, Encode, Decode)]
pub enum OverlayPayload {
    Chunk {
        chunk_id: ChunkId,
        tree: TreeId,
        data: Vec<u8>,
    },
}

pub fn encode(payload: &OverlayPayload) -> Vec<u8> {
    bincode::encode_to_vec(payload, bincode::config::standard()).expect("overlay encode failed")
}

pub fn decode(bytes: &[u8]) -> OverlayPayload {
    let (msg, _) = bincode::decode_from_slice(bytes, bincode::config::standard()).expect("overlay decode failed");
    msg
}
