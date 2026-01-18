use peerreview_protocol::journal::Logger;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn find_first_file_depth2(root: &Path) -> PathBuf {
    // 1) fichiers directement dans root
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                return p;
            }
        }
    }

    // 2) sinon, on cherche 1 niveau plus bas
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Ok(rd2) = fs::read_dir(&p) {
                    for e2 in rd2.flatten() {
                        let p2 = e2.path();
                        if p2.is_file() {
                            return p2;
                        }
                    }
                }
            }
        }
    }

    panic!("No log file found in {:?} (depth<=2)", root);
}

#[test]
fn log_verify_ok_and_tamper_detected() {
    let dir = tempdir().unwrap();

    // Clés déterministes (ok pour test)
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
    let vk = signing_key.verifying_key();

    // Écrit quelques entrées
    {
        let mut lg = Logger::open(1, dir.path(), signing_key).unwrap();

        // Logs applicatifs (compatibles avec ton Logger actuel)
        lg.log_app("SEND", Some(2), "m1".to_string(), [7u8; 32], 1234)
            .unwrap();
        lg.log_app("RECV", Some(2), "m2".to_string(), [8u8; 32], 1235)
            .unwrap();

        // Optionnel : si ton Logger expose ces méthodes, décommente
        // lg.log_pr_in("PR_IN hello").unwrap();
        // lg.log_pr_out("PR_OUT ping").unwrap();
    }

    // Le logger choisit lui-même le nom du fichier -> on le retrouve
    let path = find_first_file_depth2(dir.path());

    // Vérification OK
    Logger::verify_log_file(&path, &vk, true).unwrap();

    // --- Tamper robuste : parse 1ère ligne JSON, modifie hash[0] ---
    let content = fs::read_to_string(&path).unwrap();
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    assert!(!lines.is_empty(), "empty log file: {:?}", path);

    let mut v: Value = serde_json::from_str(&lines[0]).unwrap();

    let hash_arr = v
        .get_mut("hash")
        .and_then(|x| x.as_array_mut())
        .expect("hash field not found or not an array");

    let first = hash_arr[0].as_u64().expect("hash[0] not u64");
    let tampered = (first + 1) % 256;
    hash_arr[0] = Value::from(tampered);

    lines[0] = serde_json::to_string(&v).unwrap();
    fs::write(&path, lines.join("\n") + "\n").unwrap();

    // Maintenant ça DOIT échouer
    let err = Logger::verify_log_file(&path, &vk, true)
        .err()
        .expect("tamper not detected");

    eprintln!("tamper correctly detected: {err}");
}
