//! Filesystem déterministe pour PeerReview
//!
//! Ce module implémente un wrapper autour du filesystem natif qui garantit
//! un comportement déterministe en gérant manuellement les métadonnées
//! avec des timestamps logiques (horloge de Lamport) au lieu des timestamps système.
//!
//! Conforme à la Section 6.3.2 du papier PeerReview:
//! "Il permet au processus wrapper au niveau utilisateur de contrôler les horodatages
//!  utilisés par le système de fichiers"

use anyhow::{Context, Result};
use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read as IoRead, Seek, SeekFrom, Write as IoWrite};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Horloge déterministe (Lamport clock)
#[derive(Debug, Clone)]
pub struct DeterministicClock {
    current_time: Arc<AtomicU64>,
}

impl DeterministicClock {
    pub fn new() -> Self {
        Self {
            current_time: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn now(&self) -> u64 {
        self.current_time.load(Ordering::SeqCst)
    }

    pub fn set_time(&self, time: u64) {
        self.current_time.store(time, Ordering::SeqCst);
    }

    pub fn update_time(&self, time: u64) {
        self.current_time.fetch_max(time, Ordering::SeqCst);
    }

    pub fn tick(&self) -> u64 {
        self.current_time.fetch_add(1, Ordering::SeqCst) + 1
    }
}

impl Default for DeterministicClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Métadonnées déterministes d'un fichier
/// Utilise des timestamps Lamport au lieu de timestamps système
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// Taille du fichier en octets
    pub size: u64,
    /// Timestamp de création (horloge Lamport)
    pub created_at: u64,
    /// Timestamp de dernière modification (horloge Lamport)
    pub modified_at: u64,
    /// Timestamp de dernier accès (horloge Lamport)
    pub accessed_at: u64,
    /// Hash du contenu (pour vérification d'intégrité)
    pub content_hash: Option<[u8; 32]>,
}

impl FileMetadata {
    pub fn new(clock_time: u64) -> Self {
        Self {
            size: 0,
            created_at: clock_time,
            modified_at: clock_time,
            accessed_at: clock_time,
            content_hash: None,
        }
    }
}

/// Filesystem déterministe
///
/// Encapsule le filesystem natif et garantit un comportement déterministe:
/// - Tous les timestamps sont gérés via l'horloge de Lamport
/// - Les métadonnées sont stockées séparément du filesystem natif
/// - Les opérations concurrentes sont sérialisées par fichier
pub struct DeterministicFS {
    /// Racine du volume
    root: PathBuf,
    /// Horloge déterministe partagée
    clock: DeterministicClock,
    /// Métadonnées déterministes par fichier
    metadata: Arc<DashMap<String, FileMetadata>>,
    /// Verrous par fichier pour sérialisation
    file_locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
}

impl DeterministicFS {
    /// Crée un nouveau filesystem déterministe
    pub fn new(root: PathBuf, clock: DeterministicClock) -> Result<Self> {
        // Créer le répertoire racine s'il n'existe pas
        fs::create_dir_all(&root)
            .with_context(|| format!("Failed to create root directory: {:?}", root))?;

        let fs = Self {
            root,
            clock,
            metadata: Arc::new(DashMap::new()),
            file_locks: Arc::new(DashMap::new()),
        };

        // Charger les métadonnées existantes si disponibles
        fs.load_metadata()?;

        Ok(fs)
    }

    /// Obtient le verrou pour un fichier
    fn get_file_lock(&self, path: &str) -> Arc<Mutex<()>> {
        self.file_locks
            .entry(path.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Résout un chemin relatif vers le chemin absolu dans le volume
    fn resolve_path(&self, relative_path: &str) -> PathBuf {
        // Nettoyer le chemin pour éviter les traversées de répertoire
        let clean_path = relative_path.trim_start_matches('/').replace("..", "");
        self.root.join(clean_path)
    }

    /// Lit des données d'un fichier
    pub fn read(&self, path: &str, offset: u64, length: u64) -> Result<Vec<u8>> {
        let lock = self.get_file_lock(path);
        let _guard = lock.lock();

        let real_path = self.resolve_path(path);

        // Lire le fichier
        let mut file = fs::File::open(&real_path)
            .with_context(|| format!("Failed to open file: {}", path))?;

        file.seek(SeekFrom::Start(offset))
            .with_context(|| format!("Failed to seek in file: {}", path))?;

        let mut buffer = vec![0u8; length as usize];
        let bytes_read = file
            .read(&mut buffer)
            .with_context(|| format!("Failed to read file: {}", path))?;

        buffer.truncate(bytes_read);

        // Mettre à jour les métadonnées avec horloge déterministe
        if let Some(mut meta) = self.metadata.get_mut(path) {
            meta.accessed_at = self.clock.now();
        } else {
            // Créer métadonnées si elles n'existent pas
            let size = fs::metadata(&real_path)
                .map(|m| m.len())
                .unwrap_or(0);

            let now = self.clock.now();
            self.metadata.insert(
                path.to_string(),
                FileMetadata {
                    size,
                    created_at: now,
                    modified_at: now,
                    accessed_at: now,
                    content_hash: None,
                },
            );
        }

        Ok(buffer)
    }

    /// Écrit des données dans un fichier
    pub fn write(&self, path: &str, offset: u64, data: &[u8]) -> Result<u64> {
        let lock = self.get_file_lock(path);
        let _guard = lock.lock();

        let real_path = self.resolve_path(path);

        // Créer les répertoires parents si nécessaire
        if let Some(parent) = real_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create parent directories for: {}", path))?;
        }

        // Ouvrir ou créer le fichier
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&real_path)
            .with_context(|| format!("Failed to open file for writing: {}", path))?;

        file.seek(SeekFrom::Start(offset))
            .with_context(|| format!("Failed to seek in file: {}", path))?;

        let bytes_written = file
            .write(data)
            .with_context(|| format!("Failed to write to file: {}", path))?;

        // Forcer l'écriture sur disque
        file.sync_all()
            .with_context(|| format!("Failed to sync file: {}", path))?;

        // Mettre à jour les métadonnées déterministes
        let now = self.clock.now();
        let final_size = fs::metadata(&real_path)
            .map(|m| m.len())
            .unwrap_or(bytes_written as u64);

        self.metadata
            .entry(path.to_string())
            .and_modify(|m| {
                m.modified_at = now;
                m.accessed_at = now;
                m.size = final_size;
                m.content_hash = None; // Invalider le hash
            })
            .or_insert_with(|| FileMetadata {
                size: final_size,
                created_at: now,
                modified_at: now,
                accessed_at: now,
                content_hash: None,
            });

        Ok(bytes_written as u64)
    }

    /// Supprime un fichier
    pub fn delete(&self, path: &str) -> Result<()> {
        let lock = self.get_file_lock(path);
        let _guard = lock.lock();

        let real_path = self.resolve_path(path);

        fs::remove_file(&real_path)
            .with_context(|| format!("Failed to delete file: {}", path))?;

        // Supprimer les métadonnées
        self.metadata.remove(path);

        Ok(())
    }

    /// Liste les fichiers d'un répertoire
    pub fn list(&self, path: &str) -> Result<Vec<String>> {
        let lock = self.get_file_lock(path);
        let _guard = lock.lock();

        let real_path = self.resolve_path(path);

        let entries = fs::read_dir(&real_path)
            .with_context(|| format!("Failed to read directory: {}", path))?;

        let mut names: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }

        names.sort();
        Ok(names)
    }

    /// Obtient les métadonnées d'un fichier
    pub fn get_metadata(&self, path: &str) -> Option<FileMetadata> {
        self.metadata.get(path).map(|m| m.clone())
    }

    /// Sauvegarde les métadonnées sur disque
    pub fn save_metadata(&self) -> Result<()> {
        let metadata_path = self.root.join(".metadata.json");

        let metadata_map: HashMap<String, FileMetadata> = self
            .metadata
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        let json = serde_json::to_string_pretty(&metadata_map)
            .context("Failed to serialize metadata")?;

        fs::write(&metadata_path, json)
            .context("Failed to write metadata file")?;

        Ok(())
    }

    /// Charge les métadonnées depuis le disque
    pub fn load_metadata(&self) -> Result<()> {
        let metadata_path = self.root.join(".metadata.json");

        if !metadata_path.exists() {
            return Ok(());
        }

        let json = fs::read_to_string(&metadata_path)
            .context("Failed to read metadata file")?;

        let metadata_map: HashMap<String, FileMetadata> =
            serde_json::from_str(&json).context("Failed to deserialize metadata")?;

        for (path, meta) in metadata_map {
            self.metadata.insert(path, meta);
        }

        Ok(())
    }

    /// Obtient l'horloge déterministe
    pub fn clock(&self) -> &DeterministicClock {
        &self.clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_deterministic_fs_read_write() {
        let temp_dir = env::temp_dir().join("test_det_fs");
        let _ = fs::remove_dir_all(&temp_dir);

        let clock = DeterministicClock::new();
        clock.set_time(100);

        let det_fs = DeterministicFS::new(temp_dir.clone(), clock.clone()).unwrap();

        // Écriture
        let written = det_fs.write("test.txt", 0, b"Hello World").unwrap();
        assert_eq!(written, 11);

        // Lecture
        let data = det_fs.read("test.txt", 0, 100).unwrap();
        assert_eq!(&data, b"Hello World");

        // Vérifier métadonnées
        let meta = det_fs.get_metadata("test.txt").unwrap();
        assert_eq!(meta.size, 11);
        assert_eq!(meta.created_at, 100);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_deterministic_timestamps() {
        let temp_dir = env::temp_dir().join("test_det_ts");
        let _ = fs::remove_dir_all(&temp_dir);

        let clock = DeterministicClock::new();
        clock.set_time(1000);

        let det_fs = DeterministicFS::new(temp_dir.clone(), clock.clone()).unwrap();

        // Écriture à t=1000
        det_fs.write("file.txt", 0, b"Data").unwrap();
        let meta1 = det_fs.get_metadata("file.txt").unwrap();
        assert_eq!(meta1.modified_at, 1000);

        // Avancer le temps
        clock.set_time(2000);

        // Modification à t=2000
        det_fs.write("file.txt", 4, b" More").unwrap();
        let meta2 = det_fs.get_metadata("file.txt").unwrap();
        assert_eq!(meta2.modified_at, 2000);
        assert_eq!(meta2.created_at, 1000); // Created time unchanged

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
