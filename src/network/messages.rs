use std::io;

use crate::types::PeerReviewMsg;

/// Encodage et decodage des messages
const HEADER_SIZE: usize = 4; // TODO: len (usize) + thread_id (usize)
const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

pub fn encode(msg: &PeerReviewMsg) -> io::Result<Vec<u8>> {
    let payload = bincode::encode_to_vec(msg, bincode::config::standard()).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Encoding error: {}", e))
    })?;

    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Message too large",
        ));
    }

    let len = (payload.len() as u32).to_be_bytes();
    let mut frame = Vec::with_capacity(HEADER_SIZE + payload.len());
    frame.extend_from_slice(&len);
    frame.extend_from_slice(&payload);

    Ok(frame)
}

pub fn try_decode(buffer: &mut Vec<u8>) -> io::Result<Option<PeerReviewMsg>> {
    if buffer.len() < HEADER_SIZE {
        return Ok(None);
    }

    let len = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;

    if len > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Message exceeds limit",
        ));
    }

    if buffer.len() < HEADER_SIZE + len {
        return Ok(None);
    }

    let payload = buffer[HEADER_SIZE..HEADER_SIZE + len].to_vec();
    buffer.drain(..HEADER_SIZE + len);

    let (msg, _) =
        bincode::decode_from_slice(&payload, bincode::config::standard()).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Decoding error: {}", e))
        })?;

    Ok(Some(msg))
}
