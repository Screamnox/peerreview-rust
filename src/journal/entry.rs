use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Type d’action enregistrée : envoi ou réception
#[derive(Debug)]
#[repr(u8)]
pub enum LogType {
    Send = 0,
    Recv = 1,
}

/// Structure d’une entrée du journal
#[derive(Debug)]
pub struct LogEntry {
    pub s_k: usize,        // numéro séquentiel (anciennement timestamp)
    pub log_type: LogType, // type d’opération
    pub dest: u32,
    pub hash: [u8; 32],
    pub sig: [u8; 32],
    pub msg: String,
}

impl LogEntry {
    /// Sérialise une LogEntry en string formatée
    /// Format: s_k;log_type;dest;hash_hex;sig_hex;msg_base64
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

        // Format: s_k;log_type;dest;hash_hex;sig_hex;msg_base64
        format!(
            "{};{};{};{};{};{}",
            log_entry.s_k, log_type_val, log_entry.dest, hash_hex, sig_hex, msg_base64
        )
    }

    /// Désérialise une string en LogEntry
    /// Format: s_k;log_type;dest;hash_hex;sig_hex;msg_base64
    pub fn deserialize(line: &str) -> std::io::Result<LogEntry> {
        let parts: Vec<&str> = line.split(';').collect();

        if parts.len() != 6 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Format invalide : 6 champs attendus",
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

        // Parser dest
        let dest = parts[2]
            .parse::<u32>()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "dest invalide"))?;

        // Parser hash (hex -> [u8; 32])
        let hash_hex = parts[3].trim();
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

        // Parser sig (hex -> [u8; 32])
        let sig_hex = parts[4].trim();
        let sig_bytes = hex::decode(sig_hex).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "sig hex invalide")
        })?;
        if sig_bytes.len() != 32 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "sig doit faire 32 bytes",
            ));
        }
        let mut sig = [0u8; 32];
        sig.copy_from_slice(&sig_bytes);

        // Parser msg (base64 -> String)
        let msg = STANDARD.decode(parts[5].trim()).map_err(|_| {
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
            dest,
            hash,
            sig,
            msg: msg_string,
        })
    }
}
