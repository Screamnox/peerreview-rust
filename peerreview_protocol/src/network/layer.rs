use std::{net::SocketAddr, sync::Arc};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::types::PeerReviewMsg;

#[derive(Clone)]
pub struct NetworkLayer;

impl NetworkLayer {
    pub fn new() -> Self {
        Self
    }

    /// Listen PR : accepte connexions, lit length-prefixed, decode bincode(v2), appelle handler.
    pub async fn listen(
        &self,
        bind: SocketAddr,
        on_msg: Arc<dyn Fn(SocketAddr, PeerReviewMsg) + Send + Sync + 'static>,
    ) -> std::io::Result<()> {
        let listener = TcpListener::bind(bind).await?;
        loop {
            let (mut sock, addr) = listener.accept().await?;
            let handler = on_msg.clone();

            tokio::spawn(async move {
                loop {
                    // 4 bytes length prefix (BE)
                    let mut len_buf = [0u8; 4];
                    if sock.read_exact(&mut len_buf).await.is_err() {
                        break;
                    }
                    let len = u32::from_be_bytes(len_buf) as usize;

                    let mut buf = vec![0u8; len];
                    if sock.read_exact(&mut buf).await.is_err() {
                        break;
                    }

                    match bincode::decode_from_slice::<PeerReviewMsg, _>(
                        &buf,
                        bincode::config::standard(),
                    ) {
                        Ok((msg, _used)) => (handler)(addr, msg),
                        Err(_) => break,
                    }
                }
            });
        }
    }

    /// Send (connect + write frame)
    pub async fn send(&self, addr: SocketAddr, msg: PeerReviewMsg) -> std::io::Result<()> {
        let mut stream = TcpStream::connect(addr).await?;
        self.send_on_stream(&mut stream, msg).await
    }

    /// Send raw = same as send (helper)
    pub async fn send_raw(&self, addr: SocketAddr, msg: PeerReviewMsg) -> std::io::Result<()> {
        self.send(addr, msg).await
    }

    async fn send_on_stream(
        &self,
        stream: &mut TcpStream,
        msg: PeerReviewMsg,
    ) -> std::io::Result<()> {
        let payload = bincode::encode_to_vec(msg, bincode::config::standard())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        let len = payload.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&payload).await?;
        Ok(())
    }
}
