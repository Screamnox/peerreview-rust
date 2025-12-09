use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use serde::{de::DeserializeOwned, Serialize};

use crate::types::messages::PeerReviewMsg;

#[derive(Clone)]
pub struct NetworkLayer {
    listener_addr: SocketAddr,
}

impl NetworkLayer {
    pub fn new(bind_addr: &str) -> io::Result<Self> {
        let addr: SocketAddr = bind_addr
            .parse::<SocketAddr>()
            .map_err(|e: std::net::AddrParseError| {
                io::Error::new(io::ErrorKind::InvalidInput, e.to_string())
            })?;
        Ok(Self { listener_addr: addr })
    }

    /// Écoute les connexions entrantes et appelle un callback pour chaque message reçu.
    pub fn listen<F>(&self, callback: F) -> io::Result<()>
    where
        F: Fn(SocketAddr, PeerReviewMsg) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind(self.listener_addr)?;
        println!("[PR] listening on {}", self.listener_addr);

        let cb = Arc::new(callback);

        thread::spawn(move || {
            for stream_res in listener.incoming() {
                match stream_res {
                    Ok(mut stream) => {
                        let peer_addr = stream
                            .peer_addr()
                            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap());
                        let cb_inner = cb.clone();
                        thread::spawn(move || loop {
                            match recv_framed::<PeerReviewMsg>(&mut stream) {
                                Ok(msg) => {
                                    cb_inner(peer_addr, msg);
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[PR] error on recv from {}: {}",
                                        peer_addr, e
                                    );
                                    break;
                                }
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("[PR] accept error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Ouvre une connexion vers un pair
    pub fn connect(&self, addr: &str) -> io::Result<TcpStream> {
        TcpStream::connect(addr)
    }

    /// Envoie un message PeerReviewMsg sur un TcpStream ouvert
    pub fn send_message(
        &self,
        stream: &mut TcpStream,
        msg: &PeerReviewMsg,
    ) -> io::Result<()> {
        send_framed(stream, msg)
    }
}

/// ---- helpers framing + bincode (v2) ----

fn send_framed<T: Serialize>(stream: &mut TcpStream, msg: &T) -> io::Result<()> {
    let bytes = bincode::serde::encode_to_vec(msg, bincode::config::standard())
        .map_err(|e: bincode::error::EncodeError| {
            io::Error::new(io::ErrorKind::InvalidData, e.to_string())
        })?;

    let len = bytes.len() as u32;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn recv_framed<T: DeserializeOwned>(stream: &mut TcpStream) -> io::Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;

    let (msg, _consumed): (T, usize) =
        bincode::serde::decode_from_slice(&buf, bincode::config::standard())
            .map_err(|e: bincode::error::DecodeError| {
                io::Error::new(io::ErrorKind::InvalidData, e.to_string())
            })?;
    Ok(msg)
}
