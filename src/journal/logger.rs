use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;

// Imports internes :
use super::entry::{LogEntry, LogType};
use super::file_utils::replace_line_at_position;

/// Journaliseur : écrit les entrées dans un fichier texte
pub struct Logger {
    pub s_k: usize,
    line_max: usize,
    line_current: usize,
    file: File,
}

impl Logger {
    fn log_integrity(
        reader: &mut BufReader<File>,
        line_max: usize,
        nb_semicol: u8,
    ) -> std::io::Result<()> {
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
                "Le nombre de ligne du fichier de log existant et le nombre de ligne indiqué ne correspondent pas",
            ));
        }

        Ok(())
    }

    /// Remplit le fichier avec des lignes fictives (format standard)
    fn initialize_file(
        path: &str,
        line_max: usize,
        min_line_size: usize,
        nb_semicol: u8,
    ) -> std::io::Result<()> {
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
                    .and_then(|s| s.parse::<usize>().ok()) // parse réussi → Some(valeur)
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
        file = File::options().read(true).write(true).open(path)?;

        Ok(Self {
            s_k,
            line_max,
            line_current,
            file,
        })
    }

    /// Ajoute une entrée au journal et retourne la ligne sérialisée
    pub fn log(
        &mut self,
        entry_type: LogType,
        destinataire: u32,
        msg: &str,
    ) -> std::io::Result<()> {
        self.s_k += 1;

        if self.line_current >= self.line_max {
            self.line_current = 0;
        }

        let hash: [u8; 32] = [
            0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8, 0x5F, 0xA2, 0x39, 0x6C, 0x00, 0x4D,
            0x8B, 0x17, 0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE, 0x29, 0x74, 0x66, 0x01,
            0xB8, 0x42, 0xDA, 0x10,
        ];

        let sig: [u8; 32] = [
            0xAA, 0x19, 0xE3, 0x4F, 0x0C, 0xB2, 0x7D, 0x33, 0x91, 0x60, 0x18, 0x72, 0xBE, 0x05,
            0xD9, 0x27, 0x48, 0x9A, 0xF1, 0xC3, 0x14, 0x26, 0xE0, 0x8F, 0x55, 0x31, 0xB4, 0x7A,
            0x02, 0x63, 0xD5, 0xC0,
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

        replace_line_at_position(&mut self.file, self.line_current, &entry)?;

        println!(
            "LogEntry:\n  s_k: {}\n  log_type: {:?}\n  dest: {}\n  hash: {:?}\n  sig: {:?}\n  msg: {}",
            self.s_k, entry_type, destinataire, hash, sig, msg
        );

        self.line_current += 1;
        Ok(())
    }

    /// Récupère les n dernières entrées de log
    /// Retourne un Vec<LogEntry> avec les n dernières entrées
    pub fn get_log(&mut self, n: usize) -> std::io::Result<Vec<LogEntry>> {
        let mut logs = Vec::new();
        
        // Se positionner au début du fichier
        self.file.seek(SeekFrom::Start(0))?;
        let reader = BufReader::new(&self.file);
        
        // Lire toutes les lignes
        let mut all_entries = Vec::new();
        for line_result in reader.lines() {
            let line = line_result?;
            // Ignorer les lignes vides ou qui contiennent uniquement des séparateurs
            if line.trim().is_empty() || line.trim().chars().all(|c| c == ';' || c == ' ') {
                continue;
            }
            
            // Désérialiser l'entrée
            match LogEntry::deserialize(&line) {
                Ok(entry) => all_entries.push(entry),
                Err(_) => continue, // Ignorer les lignes invalides
            }
        }
        
        // Prendre les n dernières entrées
        let start_index = if all_entries.len() > n {
            all_entries.len() - n
        } else {
            0
        };
        
        for entry in all_entries.into_iter().skip(start_index) {
            logs.push(entry);
        }
        
        if logs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Aucune entrée de log trouvée",
            ));
        }
        
        Ok(logs)
    }
}
