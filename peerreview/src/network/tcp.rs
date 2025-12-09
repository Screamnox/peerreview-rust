use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::sync::{Arc, Mutex};
use std::thread;

use bincode::Options;
use serde::{Serialize, de::DeserializeOwned};

use crate::types::messages::PeerReviewMsg;

#[derive(Clone)]
pub struct NetworkLayer {
    listener_addr: SocketAddr,
}

impl NetworkLayer {
    pub fn new(bind_addr: &str) -> std::io::Result<Self> {
        let addr: SocketAddr = bind_addr.parse().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
        })?;
        Ok(Self { listener_addr: addr })
    }

    /// Lance un thread qui accepte les connexions entrantes et appelle un callback
    pub fn listen<F>(&self, callback: F) -> std::io::Result<()>
    where
        F: Fn(SocketAddr, PeerReviewMsg) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind(self.listener_addr)?;
        println!("[PR] listening on {}", self.listener_addr);

        let cb = Arc::new(callback);

        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut s) => {
                        let peer_addr = s.peer_addr().unwrap_or_else(|_| {
                            "0.0.0.0:0".parse().unwrap()
                        });
                        let cb = cb.clone();
                        thread::spawn(move || {
                            loop {
                                match recv_framed::<PeerReviewMsg>(&mut s) {
                                    Ok(msg) => {
                                        cb(peer_addr, msg);
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[PR] error on recv from {}: {}",
                                            peer_addr, e
                                        );
                                        break;
                                    }
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

    /// Envoie un message PeerReviewMsg sur un TcpStream ouvert
    pub fn send_message(
        &self,
        stream: &mut TcpStream,
        msg: &PeerReviewMsg,
    ) -> std::io::Result<()> {
        send_framed(stream, msg)
    }

    /// Ouvre une connexion vers un pair
    pub fn connect(&self, addr: &str) -> std::io::Result<TcpStream> {
        TcpStream::connect(addr)
    }
}

/// ---- helpers framing + bincode ----

fn bincode_cfg() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

fn send_framed<T: Serialize>(
    stream: &mut TcpStream,
    msg: &T,
) -> std::io::Result<()> {
    let bytes = bincode_cfg().serialize(msg).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;

    let len = bytes.len() as u32;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn recv_framed<T: DeserializeOwned>(
    stream: &mut TcpStream,
) -> std::io::Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;

    let msg = bincode_cfg().deserialize(&buf).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;
    Ok(msg)
}
