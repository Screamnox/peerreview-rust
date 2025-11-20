use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use peerreview::journal::{LogError, Logger, MessageType};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a new logger with in-memory temporary database
fn make_logger(max: u64) -> Logger {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    Logger::new(path.to_str().unwrap(), max).unwrap()
}

/// Create a persistent logger for tests that need to reopen the same DB
fn make_persistent_logger(path: &str, max: u64) -> Logger {
    Logger::new(path, max).unwrap()
}

/// Compute expected hash for a log entry
fn compute_expected_hash(prev_hash: [u8; 32], seq: u64, kind: MessageType, msg: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash);
    hasher.update(seq.to_be_bytes());
    hasher.update([kind as u8]);
    hasher.update(Sha256::digest(msg));
    let result = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&result);
    arr
}

// ============================================================================
// Initialization & Recovery Tests
// ============================================================================

#[test]
fn test_new_logger_empty_db() {
    let logger = make_logger(10);
    assert_eq!(logger.next_sequence(), 0);
    assert_eq!(logger.last_hash(), [0u8; 32]);
}

#[test]
fn test_recover_sequence_nonempty_db() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();

    // First logger logs some entries
    {
        let mut logger = make_persistent_logger(path, 10);
        logger.log(MessageType::Send, 1, b"a").unwrap(); // seq = 0
        logger.log(MessageType::Recv, 2, b"b").unwrap(); // seq = 1
    }

    // Second logger should detect next_seq = 2
    let log2 = make_persistent_logger(path, 10);
    assert_eq!(log2.next_sequence(), 2);
}

#[test]
fn test_recover_last_hash_nonempty_db() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();

    let expected_hash = {
        let mut logger = make_persistent_logger(path, 10);
        logger.log(MessageType::Send, 1, b"test").unwrap();
        logger.last_hash()
    };

    // Reopen and verify hash is recovered
    let log2 = make_persistent_logger(path, 10);
    assert_eq!(log2.last_hash(), expected_hash);
}

// ============================================================================
// Basic Logging Tests
// ============================================================================

#[test]
fn test_log_and_read_simple() {
    let mut logger = make_logger(10);

    let s0 = logger.log(MessageType::Send, 5, b"hello").unwrap();
    let s1 = logger.log(MessageType::Recv, 6, b"world").unwrap();

    assert_eq!(s0, 0);
    assert_eq!(s1, 1);

    let entries = logger.read_range(0, 1).unwrap();
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
fn test_log_empty_message() {
    let mut logger = make_logger(10);
    let seq = logger.log(MessageType::Send, 1, b"").unwrap();

    let entries = logger.read_range(seq, seq).unwrap();
    assert_eq!(entries[0].msg, b"");
}

#[test]
fn test_log_large_message() {
    let mut logger = make_logger(10);
    let large_msg = vec![0x42u8; 10_000];

    let seq = logger.log(MessageType::Send, 1, &large_msg).unwrap();
    let entries = logger.read_range(seq, seq).unwrap();

    assert_eq!(entries[0].msg, large_msg);
}

// ============================================================================
// Range Reading Tests
// ============================================================================

#[test]
fn test_read_empty_range() {
    let mut logger = make_logger(10);
    logger.log(MessageType::Send, 1, b"x").unwrap();

    let entries = logger.read_range(5, 3).unwrap(); // start > end
    assert!(entries.is_empty());
}

#[test]
fn test_read_single_entry() {
    let mut logger = make_logger(10);
    logger.log(MessageType::Send, 1, b"only").unwrap();

    let entries = logger.read_range(0, 0).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].msg, b"only");
}

#[test]
fn test_read_nonexistent_range() {
    let mut logger = make_logger(10);
    logger.log(MessageType::Send, 1, b"x").unwrap();

    // Try to read entries that don't exist
    let result = logger.read_range(10, 20);
    assert!(matches!(
        result,
        Err(LogError::MissingRows {
            requested: 11,
            found: 0
        })
    ));
}

#[test]
fn test_read_partial_range() {
    let mut logger = make_logger(10);
    logger.log(MessageType::Send, 1, b"a").unwrap(); // seq 0
    logger.log(MessageType::Send, 1, b"b").unwrap(); // seq 1
    // gap - no seq 2

    // Reading 0-2 should fail due to missing seq 2
    let result = logger.read_range(0, 2);
    assert!(matches!(
        result,
        Err(LogError::MissingRows {
            requested: 3,
            found: 2
        })
    ));
}

// ============================================================================
// Circular Buffer Tests
// ============================================================================

#[test]
fn test_log_wraparound() {
    let mut logger = make_logger(3); // max_lines = 3

    logger.log(MessageType::Send, 1, b"a").unwrap(); // seq 0, pos 0
    logger.log(MessageType::Send, 1, b"b").unwrap(); // seq 1, pos 1
    logger.log(MessageType::Send, 1, b"c").unwrap(); // seq 2, pos 2
    logger.log(MessageType::Send, 1, b"d").unwrap(); // seq 3, pos 0 (overwrites a)

    // Seq 0 has been overwritten
    let result = logger.read_range(0, 3);
    assert!(matches!(result, Err(LogError::MissingRows { .. })));

    // But we can read sequences 1-3
    let entries = logger.read_range(1, 3).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].msg, b"b");
    assert_eq!(entries[1].msg, b"c");
    assert_eq!(entries[2].msg, b"d");
}

#[test]
fn test_wraparound_multiple_cycles() {
    let mut logger = make_logger(2);

    // Write 5 entries with max_lines = 2
    logger.log(MessageType::Send, 1, b"a").unwrap(); // pos 0
    logger.log(MessageType::Send, 1, b"b").unwrap(); // pos 1
    logger.log(MessageType::Send, 1, b"c").unwrap(); // pos 0 (overwrites a)
    logger.log(MessageType::Send, 1, b"d").unwrap(); // pos 1 (overwrites b)
    logger.log(MessageType::Send, 1, b"e").unwrap(); // pos 0 (overwrites c)

    // Only the last 2 entries should be readable
    let entries = logger.read_range(3, 4).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].msg, b"d");
    assert_eq!(entries[1].msg, b"e");
}

// ============================================================================
// Hash Integrity Tests
// ============================================================================

#[test]
fn test_log_hash_computation() {
    let mut logger = make_logger(10);
    let seq = logger.log(MessageType::Send, 1, b"hello").unwrap();

    let last_hash = logger.last_hash();
    let expected = compute_expected_hash([0u8; 32], seq, MessageType::Send, b"hello");

    assert_eq!(last_hash, expected);
}

#[test]
fn test_recursive_hashing() {
    let mut logger = make_logger(10);

    logger.log(MessageType::Send, 1, b"a").unwrap();
    let h1 = logger.last_hash();

    logger.log(MessageType::Recv, 2, b"b").unwrap();
    let h2 = logger.last_hash();

    assert_ne!(h1, h2);

    // Verify manual computation matches
    let manual_h1 = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"a");
    let manual_h2 = compute_expected_hash(manual_h1, 1, MessageType::Recv, b"b");

    assert_eq!(h1, manual_h1);
    assert_eq!(h2, manual_h2);
}

#[test]
fn test_verify_single_entry() {
    let mut logger = make_logger(10);
    let initial_hash = logger.last_hash();

    logger.log(MessageType::Send, 1, b"test").unwrap();

    let entries = logger.read_range(0, 0).unwrap();
    assert!(logger.verify_entry(&entries[0], &initial_hash));
}

#[test]
fn test_verify_range_success() {
    let mut logger = make_logger(10);

    logger.log(MessageType::Send, 1, b"msg1").unwrap();
    logger.log(MessageType::Send, 2, b"msg2").unwrap();
    logger.log(MessageType::Recv, 3, b"msg3").unwrap();

    let entries = logger.read_range(0, 2).unwrap();
    assert!(logger.verify_range(&entries, [0u8; 32]).is_ok());
}

#[test]
fn test_verify_range_tampered_entry() {
    let mut logger = make_logger(10);

    logger.log(MessageType::Send, 1, b"msg1").unwrap();
    logger.log(MessageType::Send, 2, b"msg2").unwrap();

    let mut entries = logger.read_range(0, 1).unwrap();

    // Tamper with the second entry's message
    entries[1].msg = b"TAMPERED".to_vec();

    let result = logger.verify_range(&entries, [0u8; 32]);
    assert!(matches!(
        result,
        Err(LogError::HashVerificationFailed { seq: 1 })
    ));
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_invalid_message_type() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let logger = make_persistent_logger(path, 10);

    // Insert invalid kind value manually
    logger.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
         VALUES (0, 0, 99, 1, x'0000000000000000000000000000000000000000000000000000000000000000', '0', x'00')",
        [],
    )
    .unwrap();

    let err = logger.read_range(0, 0).unwrap_err();
    assert!(matches!(err, LogError::InvalidMessageType(99)));
}

#[test]
fn test_negative_seq_triggers_on_recover() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();

    {
        let logger = make_persistent_logger(path, 10);
        logger.exec_raw(
            "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
             VALUES (0, -1, 0, 1, x'0000000000000000000000000000000000000000000000000000000000000000', '0', x'00')",
            [],
        )
        .unwrap();
    }

    let err = Logger::new(path, 10).unwrap_err();
    assert!(matches!(err, LogError::InvalidSequence(-1)));
}

#[test]
fn test_negative_seq_in_read() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let logger = make_persistent_logger(path, 10);

    logger.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
         VALUES (0, -1, 0, 1, x'0000000000000000000000000000000000000000000000000000000000000000', '0', x'00')",
        [],
    )
    .unwrap();

    let err = logger.read_range(0, 0).unwrap_err();
    // Should get MissingRows because negative seq won't match the WHERE clause
    assert!(matches!(
        err,
        LogError::MissingRows {
            requested: 1,
            found: 0
        }
    ));
}

#[test]
fn test_negative_dest_in_db() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let logger = make_persistent_logger(path, 10);

    logger.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
         VALUES (0, 0, 0, -5, x'0000000000000000000000000000000000000000000000000000000000000000', '0', x'00')",
        [],
    )
    .unwrap();

    let err = logger.read_range(0, 0).unwrap_err();
    assert!(matches!(err, LogError::InvalidDest(-5)));
}

#[test]
fn test_invalid_hash_length_in_db() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let logger = make_persistent_logger(path, 10);

    // Insert entry with wrong hash length (16 bytes instead of 32)
    logger
        .exec_raw(
            "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
         VALUES (0, 0, 0, 1, x'00000000000000000000000000000000', '0', x'00')",
            [],
        )
        .unwrap();

    let err = logger.read_range(0, 0).unwrap_err();
    assert!(matches!(err, LogError::InvalidHashLength(16)));
}

#[test]
fn test_invalid_hash_length_on_recovery() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();

    {
        let logger = make_persistent_logger(path, 10);
        logger
            .exec_raw(
                "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
             VALUES (0, 0, 0, 1, x'FFFF', '0', x'00')",
                [],
            )
            .unwrap();
    }

    let err = Logger::new(path, 10).unwrap_err();
    assert!(matches!(err, LogError::InvalidHashLength(2)));
}

// ============================================================================
// Edge Cases & Stress Tests
// ============================================================================

#[test]
fn test_max_lines_of_one() {
    let mut logger = make_logger(1);

    logger.log(MessageType::Send, 1, b"first").unwrap();
    logger.log(MessageType::Send, 1, b"second").unwrap();

    // Only the second entry should exist
    let result = logger.read_range(0, 1);
    assert!(matches!(result, Err(LogError::MissingRows { .. })));

    let entries = logger.read_range(1, 1).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].msg, b"second");
}

#[test]
fn test_sequential_logging_performance() {
    let mut logger = make_logger(1000);

    for i in 0..100 {
        let msg = format!("message_{}", i);
        let seq = logger
            .log(MessageType::Send, i as u32, msg.as_bytes())
            .unwrap();
        assert_eq!(seq, i);
    }

    let entries = logger.read_range(0, 99).unwrap();
    assert_eq!(entries.len(), 100);
}

#[test]
fn test_persistence_across_multiple_reopens() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();

    // First session: write 3 entries
    {
        let mut logger = make_persistent_logger(path, 10);
        logger.log(MessageType::Send, 1, b"a").unwrap();
        logger.log(MessageType::Send, 2, b"b").unwrap();
        logger.log(MessageType::Recv, 3, b"c").unwrap();
    }

    // Second session: write 2 more
    {
        let mut logger = make_persistent_logger(path, 10);
        assert_eq!(logger.next_sequence(), 3);
        logger.log(MessageType::Send, 4, b"d").unwrap();
        logger.log(MessageType::Recv, 5, b"e").unwrap();
    }

    // Third session: verify all 5 entries
    {
        let logger = make_persistent_logger(path, 10);
        let entries = logger.read_range(0, 4).unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[4].msg, b"e");
    }
}
