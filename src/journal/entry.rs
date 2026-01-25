use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bincode::{Decode, Encode};
use std::fmt;

use crate::journal::logger::NB_SEMICOL;

/// Type d’action enregistrée : envoi ou réception
#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
#[repr(u8)]
pub enum LogType {
    Send = 0,
    Recv = 1,
}

/// Structure d'une entrée du journal
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct LogEntry {
    pub s_k: usize,        // numéro séquentiel (anciennement timestamp)
    pub log_type: LogType, // type d’opération
    pub corr: u32,
    pub s_k_corr: usize,
    pub hash: [u8; 32],
    pub sig: [u8; 64],
    pub msg: String,
}

impl LogEntry {
    /// Sérialise une LogEntry en string formatée
    /// Format: s_k;log_type;corr;hash_hex;sig_hex;msg_base64
    pub fn serialize(log_entry: LogEntry) -> String {
        let log_type_val = match log_entry.log_type {
            LogType::Send => 0u8,
            LogType::Recv => 1u8,
        };

        // Convertir les arrays de bytes en hex
        let hash_hex = hex::encode(log_entry.hash);
        let sig_hex = hex::encode(log_entry.sig);

        // Convertir le message en base64
        let msg_base64 = STANDARD.encode(&log_entry.msg);

        // Format: s_k;log_type;corr;hash_hex;sig_hex;msg_base64
        format!(
            "{};{};{};{};{};{};{}",
            log_entry.s_k,
            log_type_val,
            log_entry.corr,
            log_entry.s_k_corr,
            hash_hex,
            sig_hex,
            msg_base64
        )
    }

    /// Désérialise une string en LogEntry
    /// Format: s_k;log_type;corr;hash_hex;sig_hex;msg_base64
    pub fn deserialize(line: &str) -> std::io::Result<LogEntry> {
        let parts: Vec<&str> = line.split(';').collect();

        if parts.len() != NB_SEMICOL as usize + 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Format invalide : {} champs attendus", NB_SEMICOL),
            ));
        }

        // Parser s_k
        let s_k = parts[0]
            .parse::<usize>()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "s_k invalide"))?;

        // Parser log_type
        let log_type_val = parts[1].parse::<u8>().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "log_type invalide")
        })?;
        let log_type = match log_type_val {
            0 => LogType::Send,
            1 => LogType::Recv,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "log_type doit être 0 ou 1",
                ));
            }
        };

        // Parser corr
        let corr = parts[2]
            .parse::<u32>()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "corr invalide"))?;

        //Parser s_k_corr
        let s_k_corr = parts[3].parse::<usize>().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "s_k_corr invalide")
        })?;

        // Parser hash (hex -> [u8; 32])
        let hash_hex = parts[4].trim();
        let hash_bytes = hex::decode(hash_hex).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "hash hex invalide")
        })?;
        if hash_bytes.len() != 32 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "hash doit faire 32 bytes",
            ));
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hash_bytes);

        // Parser sig (hex -> [u8; 64])
        let sig_hex = parts[5].trim();
        let sig_bytes = hex::decode(sig_hex).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "sig hex invalide")
        })?;
        if sig_bytes.len() != 64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "sig doit faire 64 bytes",
            ));
        }
        let mut sig = [0u8; 64];
        sig.copy_from_slice(&sig_bytes);

        // Parser msg (base64 -> String)
        let msg = STANDARD.decode(parts[6].trim()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "msg base64 invalide")
        })?;
        let msg_string = String::from_utf8(msg).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "msg n'est pas une string UTF-8 valide",
            )
        })?;

        Ok(LogEntry {
            s_k,
            log_type,
            corr,
            s_k_corr,
            hash,
            sig,
            msg: msg_string,
        })
    }
}

impl fmt::Display for LogEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hash_hex = hex::encode(self.hash);
        let sig_hex = hex::encode(self.sig);

        write!(
            f,
            "LogEntry:\n  s_k: {}\n  log_type: {:?}\n  corr: {}\n  s_k_corr: {}\n hash: {}\n  sig: {}\n  msg: {}",
            self.s_k, self.log_type, self.corr, self.s_k_corr, hash_hex, sig_hex, self.msg
        )
    }
}
