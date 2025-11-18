use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use peerreview::journal::{LogError, Logger, MessageType};

fn make_logger(max: u64) -> Logger {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    Logger::new(path.to_str().unwrap(), max).unwrap()
}

#[test]
fn test_new_logger_empty_db() {
    let log = make_logger(10);
    assert_eq!(log.next_sequence(), 0);
}

#[test]
fn test_recover_sequence_nonempty_db() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();

    // First logger logs some entries
    {
        let mut log = Logger::new(path, 10).unwrap();
        log.log(MessageType::Send, 1, b"a").unwrap(); // seq = 0
        log.log(MessageType::Recv, 2, b"b").unwrap(); // seq = 1
    }

    // Second logger should detect next_seq = 2
    let log2 = Logger::new(path, 10).unwrap();
    assert_eq!(log2.next_sequence(), 2);
}

#[test]
fn test_log_and_read_simple() {
    let mut log = make_logger(10);

    let s0 = log.log(MessageType::Send, 5, b"hello").unwrap();
    let s1 = log.log(MessageType::Recv, 6, b"world").unwrap();

    assert_eq!(s0, 0);
    assert_eq!(s1, 1);

    let entries = log.read_range(0, 1).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].seq, 0);
    assert_eq!(entries[0].kind, MessageType::Send);
    assert_eq!(entries[0].dest, 5);
    assert_eq!(entries[0].msg, b"hello");

    assert_eq!(entries[1].seq, 1);
    assert_eq!(entries[1].kind, MessageType::Recv);
    assert_eq!(entries[1].dest, 6);
    assert_eq!(entries[1].msg, b"world");
}

#[test]
fn test_log_wraparound() {
    let mut log = make_logger(3); // max_lines = 3

    log.log(MessageType::Send, 1, b"a").unwrap(); // seq 0, pos 0
    log.log(MessageType::Send, 1, b"b").unwrap(); // seq 1, pos 1
    log.log(MessageType::Send, 1, b"c").unwrap(); // seq 2, pos 2
    log.log(MessageType::Send, 1, b"d").unwrap(); // seq 3, pos 0 overwrite

    let entries = log.read_range(0, 3).unwrap();

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].msg, b"b");
    assert_eq!(entries[1].msg, b"c");
    assert_eq!(entries[2].msg, b"d");
}

#[test]
fn test_read_empty_range() {
    let mut log = make_logger(10);

    log.log(MessageType::Send, 1, b"x").unwrap();
    let entries = log.read_range(5, 3).unwrap(); // start > end
    assert!(entries.is_empty());
}

#[test]
fn test_invalid_message_type() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let log = Logger::new(path, 10).unwrap();

    // Insert invalid kind value manually
    log.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
         VALUES (0, 0, 99, 1, x'00', '0', x'00')",
        [],
    )
    .unwrap();

    let err = log.read_range(0, 0).unwrap_err();
    match err {
        LogError::InvalidMessageType(v) => assert_eq!(v, 99),
        _ => panic!("wrong error type"),
    }
}

#[test]
fn test_negative_seq_triggers_on_recover() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();

    {
        let log = Logger::new(path, 10).unwrap();
        log.exec_raw(
            "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
             VALUES (0, -1, 0, 1, '0', '0', x'00')",
            [],
        )
        .unwrap();
    }

    let err = Logger::new(path, 10).unwrap_err();
    match err {
        LogError::InvalidSequence(v) => assert_eq!(v, -1),
        _ => panic!("Expected InvalidSequence error, got {:?}", err),
    }
}

#[test]
fn test_negative_seq_in_db() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let log = Logger::new(path, 10).unwrap();

    log.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
         VALUES (0, -1, 0, 1, x'00', '0', x'00')",
        [],
    )
    .unwrap();

    let results = log.read_range(0, 0).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_negative_dest_in_db() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let log = Logger::new(path, 10).unwrap();

    log.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
         VALUES (0, 0, 0, -5, x'00', '0', x'00')",
        [],
    )
    .unwrap();

    let err = log.read_range(0, 0).unwrap_err();
    match err {
        LogError::InvalidDest(v) => assert_eq!(v, -5),
        _ => panic!("wrong error type"),
    }
}

#[test]
fn test_recover_last_hash_empty_logger() {
    let log = make_logger(10);
    assert_eq!(log.last_hash(), [0u8; 32]);
}

#[test]
fn test_log_hash_computation() {
    let mut log = make_logger(10);
    let seq = log.log(MessageType::Send, 1, b"hello").unwrap();

    let last_hash = log.last_hash();
    let expected = {
        let mut hasher = Sha256::new();
        hasher.update([0u8; 32]);
        hasher.update(seq.to_be_bytes());
        hasher.update([MessageType::Send as u8]);
        hasher.update(Sha256::digest(b"hello"));
        hasher.finalize()
    };
    assert_eq!(last_hash.as_slice(), expected.as_slice());
}

#[test]
fn test_recursive_hashing() {
    let mut log = make_logger(10);
    let _ = log.log(MessageType::Send, 1, b"a").unwrap();
    let h1 = log.last_hash();
    let _ = log.log(MessageType::Recv, 2, b"b").unwrap();
    let h2 = log.last_hash();

    assert_ne!(h1, h2);

    // Compute manually to verify recursion
    let mut hasher = Sha256::new();
    hasher.update([0u8; 32]);
    hasher.update(0u64.to_be_bytes());
    hasher.update([MessageType::Send as u8]);
    hasher.update(Sha256::digest(b"a"));
    let manual_h1 = hasher.finalize();

    let mut hasher2 = Sha256::new();
    hasher2.update(manual_h1);
    hasher2.update(1u64.to_be_bytes());
    hasher2.update([MessageType::Recv as u8]);
    hasher2.update(Sha256::digest(b"b"));
    let manual_h2 = hasher2.finalize();

    assert_eq!(h1.as_slice(), manual_h1.as_slice());
    assert_eq!(h2.as_slice(), manual_h2.as_slice());
}
