use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::types::PeerReviewMsg;

pub struct NetworkLayer;

impl NetworkLayer {
    pub fn new() -> Self {
        Self
    }

    pub async fn listen<F>(&self, bind: SocketAddr, on_msg: F) -> std::io::Result<()>
    where
        F: Fn(SocketAddr, PeerReviewMsg) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind(bind).await?;
        let on_msg = Arc::new(on_msg);

        loop {
            let (mut sock, addr) = listener.accept().await?;
            let cb = on_msg.clone();

            tokio::spawn(async move {
                loop {
                    let mut len_buf = [0u8; 4];
                    if sock.read_exact(&mut len_buf).await.is_err() {
                        break;
                    }
                    let len = u32::from_be_bytes(len_buf) as usize;
                    let mut buf = vec![0u8; len];
                    if sock.read_exact(&mut buf).await.is_err() {
                        break;
                    }

                    match bincode::decode_from_slice::<PeerReviewMsg, _>(&buf, bincode::config::standard()) {
                        Ok((msg, _used)) => (cb)(addr, msg),
                        Err(_e) => {
                            // on ignore silencieusement : si c’est du “brut” PR côté tests
                        }
                    }
                }
            });
        }
    }

    pub async fn send(&self, peer: SocketAddr, msg: &PeerReviewMsg) -> std::io::Result<()> {
        let mut s = TcpStream::connect(peer).await?;
        let payload = bincode::encode_to_vec(msg, bincode::config::standard())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        let len = (payload.len() as u32).to_be_bytes();
        s.write_all(&len).await?;
        s.write_all(&payload).await?;
        Ok(())
    }
}
