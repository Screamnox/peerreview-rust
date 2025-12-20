use ed25519_dalek::Keypair;
use ed25519_dalek::ed25519::signature::SignerMut;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;

// Imports internes :
use super::entry::{LogEntry, LogType};
use super::file_utils::replace_line_at_position;

pub const NB_SEMICOL: u8 = 6;
const HASH_INIT: [u8; 32] = [
    0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8, 0x5F, 0xA2, 0x39, 0x6C, 0x00, 0x4D, 0x8B, 0x17,
    0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE, 0x29, 0x74, 0x66, 0x01, 0xB8, 0x42, 0xDA, 0x10,
];

/// Journaliseur : écrit les entrées dans un fichier texte
pub struct Logger {
    pub s_k: usize,
    line_max: usize,
    line_current: usize,
    file: File,
    hash: [u8; 32],
}

impl Logger {
    /// Remplit le fichier avec des lignes fictives (format standard)
    fn initialize_file(path: &str, line_max: usize, min_line_size: usize) -> std::io::Result<()> {
        let mut file = File::create(path)?;

        let base_line: String = ";".repeat(NB_SEMICOL as usize);
        let padding_size = min_line_size.saturating_sub(base_line.len() + 1); // -1 pour le \n
        let padding = vec![b' '; padding_size];

        // Écrire line_max lignes
        for _ in 0..line_max {
            file.write_all(base_line.as_bytes())?;
            file.write_all(&padding)?;
            file.write_all(b"\n")?;
        }
        Ok(())
    }

    fn log_integrity(
        reader: &mut BufReader<File>,
        line_max: usize,
    ) -> std::io::Result<(usize, usize, [u8; 32])> {
        let lines = reader.lines().collect::<Result<Vec<String>, _>>()?;
        let size: usize = lines.len();

        if size != line_max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Le nombre de ligne du fichier de log existant et le nombre de ligne indiqué ne correspondent pas",
            ));
        }

        let mut s_k: usize = 0;
        let mut s_k_test: usize;

        for line in &lines {
            if line.matches(';').count() as u8 != NB_SEMICOL {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Mauvais nombre de ';'",
                ));
            }
            s_k_test = line
                .split(';')
                .next()
                .and_then(|s| s.parse::<usize>().ok()) // parse réussi → Some(valeur)
                .unwrap_or(0);

            if s_k_test > s_k {
                s_k = s_k_test;
            }
        }
        let line_current: usize = s_k % size; //Récupère la ligne actuelle en se basant sur le module de s_k par size
        Ok((
            s_k,
            line_current,
            LogEntry::deserialize(&lines[(line_current + size - 1) % size])?.hash, /*Récupère le dernier hash*/
        ))
    }

    /// Crée un nouveau logger, le fichier est créé s’il n’existe pas    
    pub fn new(path: &str, line_max: usize, min_line_size: usize) -> std::io::Result<Self> {
        let mut s_k: usize = 0;
        let mut line_current: usize = 0;
        let mut file: File;
        let hash: [u8; 32];

        if Path::new(path).exists() {
            file = File::open(path)?;
            let mut reader = BufReader::new(file);

            (s_k, line_current, hash) = Self::log_integrity(&mut reader, line_max)?;
        } else {
            Self::initialize_file(path, line_max, min_line_size)?;
            hash = HASH_INIT;
        }
        file = File::options().read(true).write(true).open(path)?;

        Ok(Self {
            s_k,
            line_max,
            line_current,
            file,
            hash,
        })
    }

    /// Ajoute une entrée recv au journal
    pub fn log_recv(
        &mut self,
        correspondent: u32,
        s_k_corr: usize,
        sig: [u8; 64],
        msg: &str,
    ) -> std::io::Result<()> {
        self.s_k += 1;

        if self.line_current >= self.line_max {
            self.line_current = 0;
        }

        let mut hasher = Sha256::new();
        hasher.update(correspondent.to_be_bytes());
        hasher.update(s_k_corr.to_be_bytes());
        hasher.update(msg.as_bytes());

        let mut c_k = [0u8; 32];
        c_k.copy_from_slice(&hasher.finalize());

        hasher = Sha256::new();
        hasher.update(self.hash);
        hasher.update(self.s_k.to_be_bytes());
        hasher.update((LogType::Recv as u8).to_be_bytes());
        hasher.update(c_k);

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hasher.finalize());

        let logentry = LogEntry {
            s_k: self.s_k,
            log_type: LogType::Recv,
            corr: correspondent,
            s_k_corr,
            hash,
            sig,
            msg: msg.to_string(),
        };

        let entry = LogEntry::serialize(logentry);

        replace_line_at_position(&mut self.file, self.line_current, &entry)?;

        self.hash = hash;
        self.line_current += 1;
        Ok(())
    }

    /// Ajoute une entrée send au journal
    pub fn log_send(
        &mut self,
        correspondent: u32,
        msg: &str,
        key: &mut Keypair,
    ) -> std::io::Result<[u8; 64]> {
        self.s_k += 1;

        if self.line_current >= self.line_max {
            self.line_current = 0;
        }

        let mut hasher = Sha256::new();
        hasher.update(correspondent.to_be_bytes());
        hasher.update(msg.as_bytes());

        let mut c_k = [0u8; 32];
        c_k.copy_from_slice(&hasher.finalize());

        hasher = Sha256::new();
        hasher.update(self.hash);
        hasher.update(self.s_k.to_be_bytes());
        hasher.update((LogType::Send as u8).to_be_bytes());
        hasher.update(c_k);

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hasher.finalize());

        let mut buf = [0u8; 40];
        buf[..8].copy_from_slice(&self.s_k.to_be_bytes());
        buf[8..].copy_from_slice(&hash);

        let mut sig = [0u8; 64];
        sig.copy_from_slice(&key.sign(&buf).to_bytes());

        let s_k_corr: usize = 0;
        let logentry = LogEntry {
            s_k: self.s_k,
            log_type: LogType::Send,
            corr: correspondent,
            s_k_corr,
            hash,
            sig,
            msg: msg.to_string(),
        };

        let entry = LogEntry::serialize(logentry);

        replace_line_at_position(&mut self.file, self.line_current, &entry)?;

        self.hash = hash;
        self.line_current += 1;
        Ok(sig)
    }

    ///Cette fonction a pour objectif de renvoyer le nombre de log demandé passé en paramètre du plus récent au plus ancien (trié par s_k)
    pub fn get_log(&mut self, mut nb_log: usize) -> std::io::Result<Vec<LogEntry>> {
        self.file.seek(SeekFrom::Start(0))?;
        let reader = BufReader::new(&self.file);
        let mut count: usize = 0;
        let lines = reader.lines().collect::<Result<Vec<String>, _>>()?;

        // Si aucun log n'a été écrit, retourner un vecteur vide
        if self.line_current == 0 {
            return Ok(Vec::new());
        }

        let mut id: usize = self.line_current - 1;
        nb_log = nb_log.min(lines.len());
        let mut result = Vec::with_capacity(nb_log);

        while count < nb_log {
            match LogEntry::deserialize(&lines[id]) {
                Ok(entry) => result.push(entry),
                Err(_) => {
                    eprintln!(
                        "get_log : Format de ligne incorrect !, Vec<LogEntry> retourné avec les précédentes valeurs"
                    );
                    break;
                }
            }
            if id == 0 {
                id += lines.len() - 1;
            } else {
                id -= 1;
            }
            count += 1;
        }
        Ok(result)
    }

    /// Renvoie le hash actuel de la chaine de hachage
    pub fn get_current_hash(&self) -> [u8; 32] {
        self.hash
    }
}
