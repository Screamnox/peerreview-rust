use serde::Deserialize;
use std::{fs, net::SocketAddr};

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
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let s = fs::read_to_string(path)?;
        let cfg: PeersConfig = toml::from_str(&s)?;
        Ok(cfg)
    }

    pub fn my_id(&self) -> u32 {
        self.me
    }

    pub fn other_peers(&self) -> impl Iterator<Item = &PeerToml> {
        self.peers.iter().filter(move |p| p.id != self.me)
    }

    pub fn parse_addr(addr: &str) -> std::io::Result<SocketAddr> {
        addr.parse().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
        })
    }
}
