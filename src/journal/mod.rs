//use core::hash;
//use std::fmt::format;
use std::fs::{File};
use std::io::{Write};
use std::path::Path;
use std::io::{BufRead, BufReader};
use std::io;
use std::io::{Seek, SeekFrom, Read};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;


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
    pub s_k: usize,          // numéro séquentiel (anciennement timestamp)
    pub log_type: LogType,  // type d’opération
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
            log_entry.s_k,
            log_type_val,
            log_entry.dest,
            hash_hex,
            sig_hex,
            msg_base64
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
        let s_k = parts[0].parse::<usize>().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "s_k invalide",
            )
        })?;
        
        // Parser log_type
        let log_type_val = parts[1].parse::<u8>().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "log_type invalide",
            )
        })?;
        let log_type = match log_type_val {
            0 => LogType::Send,
            1 => LogType::Recv,
            _ => return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "log_type doit être 0 ou 1",
            )),
        };
        
        // Parser dest
        let dest = parts[2].parse::<u32>().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "dest invalide",
            )
        })?;
        
        // Parser hash (hex -> [u8; 32])
        let hash_hex = parts[3].trim();
        let hash_bytes = hex::decode(hash_hex).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "hash hex invalide",
            )
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
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "sig hex invalide",
            )
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
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "msg base64 invalide",
            )
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


/// Journaliseur : écrit les entrées dans un fichier texte
pub struct Logger {
    s_k: usize,
    line_max: usize,
    line_current: usize,
    file: File,
}

impl Logger {

    fn log_integrity(reader: &mut BufReader<File>, line_max: usize, nb_semicol: u8) -> std::io::Result<()> {
        let mut nb_line_reader: usize = 0;

        for line_result in reader.lines() {
            nb_line_reader += 1;
            if line_result.unwrap().matches(';').count() as u8 != nb_semicol {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Mauvais nombre de ';'",
                ));
            }
        }

        if nb_line_reader != line_max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Le nombre de ligne du fichier de log existant et le nombre de ligne indiqué ne correspondent pas"
            ));
        }
        
        Ok(())
    }

    /// Remplit le fichier avec des lignes fictives (format standard)
    fn initialize_file(path: &str, line_max: usize, min_line_size: usize, nb_semicol: u8) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        
        let base_line: String = ";".repeat(nb_semicol as usize);        
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

    /// Crée un nouveau logger, le fichier est créé s’il n’existe pas    
    pub fn new(path: &str, line_max: usize, min_line_size: usize) -> std::io::Result<Self> {
        let nb_semicol: u8 = 5;
        let mut s_k: usize = 0;
        let mut line_current: usize = 0;
        let mut file: File;

        if Path::new(path).exists() {
            file = File::open(path)?;
            let mut reader = BufReader::new(file);

            Self::log_integrity(&mut reader, line_max, nb_semicol)?;
            reader.seek(SeekFrom::Start(0))?;
            
            let mut old_s_k: usize;

            
            for line_result in reader.lines() {
                let line = line_result?;

                old_s_k = s_k;

                s_k = line
                    .split(';')
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())   // parse réussi → Some(valeur)
                    .unwrap_or(0); 
                
                // On cherche le s_k max en comparant avec old_s_k
                if s_k < old_s_k {
                    s_k = old_s_k;
                    break;
                }
                line_current += 1;
            }
        } else {
            Self::initialize_file(path, line_max, min_line_size, nb_semicol)?;
        }
        file = File::options()
                .read(true)
                .write(true)
                .open(path)?;

        Ok(Self { 
            s_k,
            line_max,
            line_current,
            file,
        })
    }

    /// Ajoute une entrée au journal
    pub fn log(&mut self, entry_type: LogType, destinataire: u32, msg: &str) -> std::io::Result<()> {
        self.s_k += 1;

        if self.line_current >= self.line_max {
            self.line_current = 0;
        }

        let hash: [u8; 32] = [
            0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8,
            0x5F, 0xA2, 0x39, 0x6C, 0x00, 0x4D, 0x8B, 0x17,
            0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE,
            0x29, 0x74, 0x66, 0x01, 0xB8, 0x42, 0xDA, 0x10
        ];

        let sig: [u8; 32] = [
            0xAA, 0x19, 0xE3, 0x4F, 0x0C, 0xB2, 0x7D, 0x33,
            0x91, 0x60, 0x18, 0x72, 0xBE, 0x05, 0xD9, 0x27,
            0x48, 0x9A, 0xF1, 0xC3, 0x14, 0x26, 0xE0, 0x8F,
            0x55, 0x31, 0xB4, 0x7A, 0x02, 0x63, 0xD5, 0xC0
        ];

        let logentry = LogEntry {
            s_k: self.s_k,
            log_type: entry_type,
            dest: destinataire,
            hash,
            sig,
            msg: msg.to_string(),            
        };

        let entry = LogEntry::serialize(logentry);

        Self::replace_line_at_position(&mut self.file,self.line_current,&entry)?;

        let out: LogEntry = LogEntry::deserialize(&entry)?;

        
        println!(
            "LogEntry:\n  s_k: {}\n  log_type: {:?}\n  dest: {}\n  hash: {:?}\n  sig: {:?}\n  msg: {}",
            out.s_k,
            out.log_type,
            out.dest,
            out.hash,
            out.sig,
            out.msg
        );

        
        
        self.line_current += 1;
        Ok(())
    }

    /// Remplace une ligne à une position donnée dans le fichier
    /// 
    /// # Parameters
    /// - `file`: Fichier ouvert en lecture/écriture
    /// - `line_number`: Numéro de la ligne à remplacer (0-indexed)
    /// - `new_line`: Le nouveau contenu (sans \n)
    ///
    /// # Returns
    /// Ok(()) si succès, Err sinon
    pub fn replace_line_at_position(file: &mut File, line_number: usize, new_line: &str) -> std::io::Result<()> {
        // Étape 1 : Trouver la position de la ligne
        let line_start_pos = Self::find_line_start(file, line_number)?;
        
        // Étape 2 : Trouver la fin de la ligne (position du \n)
        file.seek(SeekFrom::Start(line_start_pos as u64))?;
        let line_end_pos = Self::find_next_newline(file, line_start_pos)?;
        let old_line_length = line_end_pos - line_start_pos; // Inclut le \n
        
        // Étape 3 : Calculer la nouvelle taille (SANS \n pour l'instant)
        let new_line_length = new_line.len();
        
        // Étape 4 : Gérer l'espace
        if new_line_length < old_line_length - 1 {  // -1 pour le \n
            // Cas 1 : Le nouveau message est plus court
            // On écrit, padding, puis \n
            Self::write_line_with_padding(file, line_start_pos, new_line, old_line_length)?;
        } else if new_line_length == old_line_length - 1 {
            // Cas 2 : Exact fit (nouveau message + \n = ancienne longueur)
            file.seek(SeekFrom::Start(line_start_pos as u64))?;
            file.write_all(new_line.as_bytes())?;
            file.write_all(b"\n")?;
        } else {
            // Cas 3 : Le nouveau message est plus long
            // On décale tout ce qui suit
            Self::write_line_with_shift(file, line_start_pos, line_end_pos, new_line)?;
        }
        
        Ok(())
    }


    /// Trouve la position (en bytes) du début d'une ligne
    fn find_line_start(file: &mut File, line_number: usize) -> std::io::Result<usize> {
        file.seek(SeekFrom::Start(0))?;
        let mut position = 0;
        let mut current_line = 0;
        let mut buffer = [0u8; 1];
        
        if line_number == 0 {
            return Ok(0);
        }
        
        while file.read_exact(&mut buffer).is_ok() {
            if buffer[0] == b'\n' {
                current_line += 1;
                if current_line == line_number {
                    return Ok(position + 1);
                }
            }
            position += 1;
        }
        
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Ligne {} n'existe pas", line_number),
        ))
    }

    /// Trouve la position du prochain \n à partir de la position actuelle
    fn find_next_newline(file: &mut File, start_pos: usize) -> std::io::Result<usize> {
        file.seek(SeekFrom::Start(start_pos as u64))?;
        let mut position = start_pos;
        let mut buffer = [0u8; 1];
        
        while file.read_exact(&mut buffer).is_ok() {
            if buffer[0] == b'\n' {
                return Ok(position + 1); // Inclut le \n
            }
            position += 1;
        }
        
        // Si on atteint la fin du fichier sans \n
        Ok(position)
    }


    /// Écrit une ligne avec padding quand l'espace est suffisant
    fn write_line_with_padding(file: &mut File, position: usize, new_line: &str, old_length: usize) -> std::io::Result<()> {
        file.seek(SeekFrom::Start(position as u64))?;
        file.write_all(new_line.as_bytes())?;
        
        // Remplir le reste avec des espaces (sauf le dernier byte qui est \n)
        let padding_size = old_length - new_line.len() - 1; // -1 pour le \n
        if padding_size > 0 {
            let padding = vec![b' '; padding_size];
            file.write_all(&padding)?;
        }
        
        // Écrire le \n à la fin
        file.write_all(b"\n")?;
        
        Ok(())
    }

    /// Écrit une ligne en décalant tout ce qui suit
    fn write_line_with_shift(file: &mut File, line_start: usize, line_end: usize, new_line: &str) -> std::io::Result<()> {
        // Lire la partie après la ligne
        file.seek(SeekFrom::Start(line_end as u64))?;
        let mut after = Vec::new();
        file.read_to_end(&mut after)?;
        
        // Réécrire : seek à la ligne, écrire la nouvelle ligne, \n, puis tout ce qui suit
        file.seek(SeekFrom::Start(line_start as u64))?;
        
        file.write_all(new_line.as_bytes())?;
        file.write_all(b"\n")?;
        file.write_all(&after)?;
        
        Ok(())
    }
}