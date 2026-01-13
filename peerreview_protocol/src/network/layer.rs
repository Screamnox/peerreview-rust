use std::io;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

use crate::types::messages::PeerReviewMsg;

/// Receiving side: NOT clonable (owns the receiver).
pub struct NetRx {
    pub rx: mpsc::Receiver<(u32, PeerReviewMsg)>,
}

/// Sending side: clonable (owns a sender).
#[derive(Clone)]
pub struct NetTx {
    pub tx: mpsc::Sender<(u32, PeerReviewMsg)>,
}

/// Small helper to build network channels.
pub struct NetworkLayer;

impl NetworkLayer {
    pub fn new() -> (NetRx, NetTx) {
        let (tx, rx) = mpsc::channel(1024);
        (NetRx { rx }, NetTx { tx })
    }

    /// Spawn a TCP listener that receives length-prefixed bincode(v2) frames.
    pub async fn spawn_listener(
        bind_addr: &str,
        deliver_tx: mpsc::Sender<(u32, PeerReviewMsg)>,
    ) -> io::Result<()> {
        let listener = TcpListener::bind(bind_addr).await?;
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let deliver_tx2 = deliver_tx.clone();
                tokio::spawn(async move {
                    loop {
                        let mut len_buf = [0u8; 4];
                        if sock.read_exact(&mut len_buf).await.is_err() {
                            break;
                        }
                        let len = u32::from_be_bytes(len_buf) as usize;
                        if len == 0 || len > 16 * 1024 * 1024 {
                            break;
                        }

                        let mut buf = vec![0u8; len];
                        if sock.read_exact(&mut buf).await.is_err() {
                            break;
                        }

                        match bincode::decode_from_slice::<PeerReviewMsg, _>(
                            &buf,
                            bincode::config::standard(),
                        ) {
                            Ok((msg, _read)) => {
                                // If msg includes sender id, use it; otherwise keep 0.
                                let from = msg_from(&msg).unwrap_or(0);
                                let _ = deliver_tx2.send((from, msg)).await;
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        });
        Ok(())
    }

    /// Send one PR message as length-prefixed bincode(v2) to a peer address.
    pub async fn send_to(peer_addr: &str, msg: &PeerReviewMsg) -> io::Result<()> {
        let mut stream = TcpStream::connect(peer_addr).await?;
        let payload = bincode::encode_to_vec(msg, bincode::config::standard())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        let len = (payload.len() as u32).to_be_bytes();
        stream.write_all(&len).await?;
        stream.write_all(&payload).await?;
        Ok(())
    }
}

/// If PeerReviewMsg contains a sender field, wire it here.
/// Otherwise return None.
fn msg_from(_m: &PeerReviewMsg) -> Option<u32> {
    None
}
