use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

/// Remplace une ligne à une position donnée dans le fichier
///
/// # Parameters
/// - `file`: Fichier ouvert en lecture/écriture
/// - `line_number`: Numéro de la ligne à remplacer (0-indexed)
/// - `new_line`: Le nouveau contenu (sans \n)
///
/// # Returns
/// Ok(()) si succès, Err sinon
pub fn replace_line_at_position(
    file: &mut File,
    line_number: usize,
    new_line: &str,
) -> std::io::Result<()> {
    // Étape 1 : Trouver la position de la ligne
    let line_start_pos = find_line_start(file, line_number)?;

    // Étape 2 : Trouver la fin de la ligne (position du \n)
    file.seek(SeekFrom::Start(line_start_pos as u64))?;
    let line_end_pos = find_next_newline(file, line_start_pos)?;
    let old_line_length = line_end_pos - line_start_pos; // Inclut le \n

    // Étape 3 : Calculer la nouvelle taille (SANS \n pour l'instant)
    let new_line_length = new_line.len();

    // Étape 4 : Gérer l'espace
    if new_line_length < old_line_length - 1 {
        // -1 pour le \n
        // Cas 1 : Le nouveau message est plus court
        // On écrit, padding, puis \n
        write_line_with_padding(file, line_start_pos, new_line, old_line_length)?;
    } else if new_line_length == old_line_length - 1 {
        // Cas 2 : Exact fit (nouveau message + \n = ancienne longueur)
        file.seek(SeekFrom::Start(line_start_pos as u64))?;
        file.write_all(new_line.as_bytes())?;
        file.write_all(b"\n")?;
    } else {
        // Cas 3 : Le nouveau message est plus long
        // On décale tout ce qui suit
        write_line_with_shift(file, line_start_pos, line_end_pos, new_line)?;
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
fn write_line_with_padding(
    file: &mut File,
    position: usize,
    new_line: &str,
    old_length: usize,
) -> std::io::Result<()> {
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
fn write_line_with_shift(
    file: &mut File,
    line_start: usize,
    line_end: usize,
    new_line: &str,
) -> std::io::Result<()> {
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
