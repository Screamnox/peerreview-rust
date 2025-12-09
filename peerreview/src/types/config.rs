use serde::Deserialize;
use std::fs;
use std::io;
use std::net::SocketAddr;

#[derive(Debug, Deserialize)]
pub struct PeerToml {
    pub id: u32,
    pub addr: String,
    pub public_key_file: String,
}

#[derive(Debug, Deserialize)]
pub struct PeersConfig {
    pub me: u32,
    pub peers: Vec<PeerToml>,
}

impl PeersConfig {
    pub fn load(path: &str) -> io::Result<Self> {
        let s = fs::read_to_string(path)?;
        let cfg: PeersConfig = toml::from_str(&s)
            .map_err(|e: toml::de::Error| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        Ok(cfg)
    }

    pub fn other_peers(&self) -> impl Iterator<Item = &PeerToml> {
        self.peers.iter().filter(move |p| p.id != self.me)
    }

    pub fn parse_addr(addr: &str) -> io::Result<SocketAddr> {
        addr.parse::<SocketAddr>()
            .map_err(|e: std::net::AddrParseError| {
                io::Error::new(io::ErrorKind::InvalidInput, e.to_string())
            })
    }
}
