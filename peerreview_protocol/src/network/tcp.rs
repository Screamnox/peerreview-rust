use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Envoie une frame length-prefixed : [len: u32 big-endian][payload bytes]
pub async fn send_frame(stream: &mut TcpStream, payload: &[u8]) -> std::io::Result<()> {
    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(payload).await?;
    Ok(())
}

/// Lit une frame length-prefixed : [len][payload]
pub async fn read_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Bind + accept loop (laisse le handler décider quoi faire avec les bytes)
pub async fn serve(
    bind_addr: SocketAddr,
    mut on_conn: impl FnMut(TcpStream, SocketAddr) + Send + 'static,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    loop {
        let (sock, addr) = listener.accept().await?;
        on_conn(sock, addr);
    }
}

/// Connect helper
pub async fn connect(addr: SocketAddr) -> std::io::Result<TcpStream> {
    TcpStream::connect(addr).await
}
