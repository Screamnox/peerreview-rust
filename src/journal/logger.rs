use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;

// Imports internes :
use super::entry::{LogEntry, LogType};
use super::file_utils::replace_line_at_position;

/// Journaliseur : écrit les entrées dans un fichier texte
pub struct Logger {
    s_k: usize,
    line_max: usize,
    line_current: usize,
    file: File,
}

impl Logger {
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

    fn log_integrity(
        reader: &mut BufReader<File>,
        line_max: usize,
        nb_semicol: u8,
    ) -> std::io::Result<(usize, usize)> {
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

        for line in lines {
            if line.matches(';').count() as u8 != nb_semicol {
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
        let line_current: usize = s_k % size;
        Ok((s_k, line_current))
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

            (s_k, line_current) = Self::log_integrity(&mut reader, line_max, nb_semicol)?;
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

    /// Ajoute une entrée au journal
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

        self.line_current += 1;
        Ok(())
    }

    ///Cette fonction a pour objectif de renvoyer le nombre de log demandé passé en paramètre du plus récent au plus ancien (trié par s_k)
    pub fn get_log(self, mut nb_log: usize) -> std::io::Result<Vec<LogEntry>> {
        let mut reader = BufReader::new(self.file);
        reader.seek(SeekFrom::Start(0))?;
        let mut count: usize = 0;
        let mut id: usize = self.line_current - 1;
        let lines = reader.lines().collect::<Result<Vec<String>, _>>()?;
        nb_log = nb_log.min(lines.len());
        let mut result = Vec::with_capacity(nb_log);

        while count < nb_log {
            match LogEntry::deserialize(&lines[id]) {
                Ok(entry) => result.push(entry),
                Err(_) => {
                    println!(
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
}
