use std::fs::{File, OpenOptions};
use std::io::{Write};
use std::path::Path;
use serde::{Serialize, Deserialize};

/// Type d’action enregistrée : envoi ou réception
#[derive(Debug, Serialize, Deserialize)]
pub enum LogType {
    SEND,
    RECV,
}

/// Structure d’une entrée du journal
#[derive(Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub s_k: u128,          // numéro séquentiel (anciennement timestamp)
    pub log_type: LogType,  // type d’opération
    pub destinataire: String,
    pub message: String,
}

/// Journaliseur : écrit les entrées dans un fichier texte
pub struct Logger {
    path: String,
    counter: u128,
}

impl Logger {
    /// Crée un nouveau logger, le fichier est créé s’il n’existe pas
    pub fn new(path: &str) -> std::io::Result<Self> {
        if !Path::new(path).exists() {
            File::create(path)?; // Crée le fichier vide
        }
        Ok(Self { 
            path: path.to_string(), 
            counter: 0,
        })
    }

    /// Ajoute une entrée au journal
    pub fn log(&mut self, entry_type: &str, destinataire: &str, message: &str) -> std::io::Result<()> {
        self.counter += 1;

        let log_type = match entry_type {
            "SEND" => LogType::SEND,
            "RECV" => LogType::RECV,
            _ => panic!("Type inconnu : utilisez 'SEND' ou 'RECV'"),
        };

        let entry = LogEntry {
            s_k: self.counter,
            log_type,
            destinataire: destinataire.to_string(),
            message: message.to_string(),
        };

        let json_line = serde_json::to_string(&entry).unwrap() + "\n";

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(json_line.as_bytes())?;

        Ok(())
    }
}