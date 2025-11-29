use ed25519_dalek::{Signature, Signer, SigningKey};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use peerreview::journal::{entry::MessageType, errors::LogError, logger::Logger};

// ============================================================================
// Test Helpers
// ============================================================================

/// Generate a deterministic signing key for testing
fn test_signing_key() -> SigningKey {
    let seed = [42u8; 32];
    SigningKey::from_bytes(&seed)
}

/// Create a test signature for a given seq and hash
fn test_signature(seq: u64, hash: &[u8; 32]) -> Signature {
    let signing_key = test_signing_key();
    let mut payload = Vec::with_capacity(8 + 32);
    payload.extend_from_slice(&seq.to_be_bytes());
    payload.extend_from_slice(hash);
    signing_key.sign(&payload)
}

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
fn compute_expected_hash(
    prev_hash: [u8; 32],
    seq: u64,
    kind: MessageType,
    content: &[u8],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash);
    hasher.update(seq.to_be_bytes());
    hasher.update([kind as u8]);
    hasher.update(Sha256::digest(content));
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
        let hash_a = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"a");
        let sig_a = test_signature(0, &hash_a);
        logger.log(MessageType::Send, 1, b"a", sig_a).unwrap(); // seq = 0

        let hash_b = compute_expected_hash(hash_a, 1, MessageType::Recv, b"b");
        let sig_b = test_signature(1, &hash_b);
        logger.log(MessageType::Recv, 2, b"b", sig_b).unwrap(); // seq = 1
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
        let hash = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"test");
        let sig = test_signature(0, &hash);
        logger.log(MessageType::Send, 1, b"test", sig).unwrap();
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

    let hash0 = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"hello");
    let sig0 = test_signature(0, &hash0);
    let s0 = logger.log(MessageType::Send, 5, b"hello", sig0).unwrap();

    let hash1 = compute_expected_hash(hash0, 1, MessageType::Recv, b"world");
    let sig1 = test_signature(1, &hash1);
    let s1 = logger.log(MessageType::Recv, 6, b"world", sig1).unwrap();

    assert_eq!(s0, 0);
    assert_eq!(s1, 1);

    let entries = logger.read_range(0, 1).unwrap();
    assert_eq!(entries.len(), 2);

    assert_eq!(entries[0].seq, 0);
    assert_eq!(entries[0].kind, MessageType::Send);
    assert_eq!(entries[0].dest, 5);
    assert_eq!(entries[0].content, b"hello");

    assert_eq!(entries[1].seq, 1);
    assert_eq!(entries[1].kind, MessageType::Recv);
    assert_eq!(entries[1].dest, 6);
    assert_eq!(entries[1].content, b"world");
}

#[test]
fn test_log_empty_message() {
    let mut logger = make_logger(10);
    let hash = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"");
    let sig = test_signature(0, &hash);
    let seq = logger.log(MessageType::Send, 1, b"", sig).unwrap();

    let entries = logger.read_range(seq, seq).unwrap();
    assert_eq!(entries[0].content, b"");
}

#[test]
fn test_log_large_message() {
    let mut logger = make_logger(10);
    let large_msg = vec![0x42u8; 10_000];

    let hash = compute_expected_hash([0u8; 32], 0, MessageType::Send, &large_msg);
    let sig = test_signature(0, &hash);
    let seq = logger.log(MessageType::Send, 1, &large_msg, sig).unwrap();
    let entries = logger.read_range(seq, seq).unwrap();

    assert_eq!(entries[0].content, large_msg);
}

// ============================================================================
// Range Reading Tests
// ============================================================================

#[test]
fn test_read_empty_range() {
    let mut logger = make_logger(10);
    let hash = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"x");
    let sig = test_signature(0, &hash);
    logger.log(MessageType::Send, 1, b"x", sig).unwrap();

    let entries = logger.read_range(5, 3).unwrap(); // start > end
    assert!(entries.is_empty());
}

#[test]
fn test_read_single_entry() {
    let mut logger = make_logger(10);
    let hash = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"only");
    let sig = test_signature(0, &hash);
    logger.log(MessageType::Send, 1, b"only", sig).unwrap();

    let entries = logger.read_range(0, 0).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, b"only");
}

#[test]
fn test_read_nonexistent_range() {
    let mut logger = make_logger(10);
    let hash = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"x");
    let sig = test_signature(0, &hash);
    logger.log(MessageType::Send, 1, b"x", sig).unwrap();

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
    let hash0 = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"a");
    let sig0 = test_signature(0, &hash0);
    logger.log(MessageType::Send, 1, b"a", sig0).unwrap(); // seq 0

    let hash1 = compute_expected_hash(hash0, 1, MessageType::Send, b"b");
    let sig1 = test_signature(1, &hash1);
    logger.log(MessageType::Send, 1, b"b", sig1).unwrap(); // seq 1
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

    let hash0 = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"a");
    let sig0 = test_signature(0, &hash0);
    logger.log(MessageType::Send, 1, b"a", sig0).unwrap(); // seq 0, pos 0

    let hash1 = compute_expected_hash(hash0, 1, MessageType::Send, b"b");
    let sig1 = test_signature(1, &hash1);
    logger.log(MessageType::Send, 1, b"b", sig1).unwrap(); // seq 1, pos 1

    let hash2 = compute_expected_hash(hash1, 2, MessageType::Send, b"c");
    let sig2 = test_signature(2, &hash2);
    logger.log(MessageType::Send, 1, b"c", sig2).unwrap(); // seq 2, pos 2

    let hash3 = compute_expected_hash(hash2, 3, MessageType::Send, b"d");
    let sig3 = test_signature(3, &hash3);
    logger.log(MessageType::Send, 1, b"d", sig3).unwrap(); // seq 3, pos 0 (overwrites a)

    // Seq 0 has been overwritten
    let result = logger.read_range(0, 3);
    assert!(matches!(result, Err(LogError::MissingRows { .. })));

    // But we can read sequences 1-3
    let entries = logger.read_range(1, 3).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].content, b"b");
    assert_eq!(entries[1].content, b"c");
    assert_eq!(entries[2].content, b"d");
}

#[test]
fn test_wraparound_multiple_cycles() {
    let mut logger = make_logger(2);

    // Write 5 entries with max_lines = 2
    let mut prev_hash = [0u8; 32];
    for (i, msg) in [b"a", b"b", b"c", b"d", b"e"].iter().enumerate() {
        let hash = compute_expected_hash(prev_hash, i as u64, MessageType::Send, *msg);
        let sig = test_signature(i as u64, &hash);
        logger.log(MessageType::Send, 1, *msg, sig).unwrap();
        prev_hash = hash;
    }

    // Only the last 2 entries should be readable
    let entries = logger.read_range(3, 4).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].content, b"d");
    assert_eq!(entries[1].content, b"e");
}

// ============================================================================
// Hash Integrity Tests
// ============================================================================

#[test]
fn test_log_hash_computation() {
    let mut logger = make_logger(10);
    let expected = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"hello");
    let sig = test_signature(0, &expected);
    let seq = logger.log(MessageType::Send, 1, b"hello", sig).unwrap();

    let last_hash = logger.last_hash();
    assert_eq!(last_hash, expected);
    assert_eq!(seq, 0);
}

#[test]
fn test_recursive_hashing() {
    let mut logger = make_logger(10);

    let h1_expected = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"a");
    let sig1 = test_signature(0, &h1_expected);
    logger.log(MessageType::Send, 1, b"a", sig1).unwrap();
    let h1 = logger.last_hash();

    let h2_expected = compute_expected_hash(h1, 1, MessageType::Recv, b"b");
    let sig2 = test_signature(1, &h2_expected);
    logger.log(MessageType::Recv, 2, b"b", sig2).unwrap();
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

    let hash = compute_expected_hash(initial_hash, 0, MessageType::Send, b"test");
    let sig = test_signature(0, &hash);
    logger.log(MessageType::Send, 1, b"test", sig).unwrap();

    let entries = logger.read_range(0, 0).unwrap();
    assert!(logger.verify_entry(&entries[0], &initial_hash));
}

#[test]
fn test_verify_range_success() {
    let mut logger = make_logger(10);

    let hash0 = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"msg1");
    let sig0 = test_signature(0, &hash0);
    logger.log(MessageType::Send, 1, b"msg1", sig0).unwrap();

    let hash1 = compute_expected_hash(hash0, 1, MessageType::Send, b"msg2");
    let sig1 = test_signature(1, &hash1);
    logger.log(MessageType::Send, 2, b"msg2", sig1).unwrap();

    let hash2 = compute_expected_hash(hash1, 2, MessageType::Recv, b"msg3");
    let sig2 = test_signature(2, &hash2);
    logger.log(MessageType::Recv, 3, b"msg3", sig2).unwrap();

    let entries = logger.read_range(0, 2).unwrap();
    assert!(logger.verify_range(&entries, [0u8; 32]).is_ok());
}

#[test]
fn test_verify_range_tampered_entry() {
    let mut logger = make_logger(10);

    let hash0 = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"msg1");
    let sig0 = test_signature(0, &hash0);
    logger.log(MessageType::Send, 1, b"msg1", sig0).unwrap();

    let hash1 = compute_expected_hash(hash0, 1, MessageType::Send, b"msg2");
    let sig1 = test_signature(1, &hash1);
    logger.log(MessageType::Send, 2, b"msg2", sig1).unwrap();

    let mut entries = logger.read_range(0, 1).unwrap();

    // Tamper with the second entry's message
    entries[1].content = b"TAMPERED".to_vec();

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

    // Insert invalid kind value manually (with a valid 64-byte signature)
    let dummy_sig = vec![0u8; 64];
    logger.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, content)
         VALUES (0, 0, 99, 1, x'0000000000000000000000000000000000000000000000000000000000000000', ?1, x'00')",
        [dummy_sig],
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
        let dummy_sig = vec![0u8; 64];
        logger.exec_raw(
            "INSERT INTO logs (pos, seq, kind, dest, hash, sig, content)
             VALUES (0, -1, 0, 1, x'0000000000000000000000000000000000000000000000000000000000000000', ?1, x'00')",
            [dummy_sig],
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

    let dummy_sig = vec![0u8; 64];
    logger.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, content)
         VALUES (0, -1, 0, 1, x'0000000000000000000000000000000000000000000000000000000000000000', ?1, x'00')",
        [dummy_sig],
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

    let dummy_sig = vec![0u8; 64];
    logger.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, content)
         VALUES (0, 0, 0, -5, x'0000000000000000000000000000000000000000000000000000000000000000', ?1, x'00')",
        [dummy_sig],
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
    let dummy_sig = vec![0u8; 64];
    logger
        .exec_raw(
            "INSERT INTO logs (pos, seq, kind, dest, hash, sig, content)
         VALUES (0, 0, 0, 1, x'00000000000000000000000000000000', ?1, x'00')",
            [dummy_sig],
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
        let dummy_sig = vec![0u8; 64];
        logger
            .exec_raw(
                "INSERT INTO logs (pos, seq, kind, dest, hash, sig, content)
             VALUES (0, 0, 0, 1, x'FFFF', ?1, x'00')",
                [dummy_sig],
            )
            .unwrap();
    }

    let err = Logger::new(path, 10).unwrap_err();
    assert!(matches!(err, LogError::InvalidHashLength(2)));
}

#[test]
fn test_invalid_signature_length() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let logger = make_persistent_logger(path, 10);

    // Insert entry with wrong signature length (32 bytes instead of 64)
    let bad_sig = vec![0u8; 32];
    logger
        .exec_raw(
            "INSERT INTO logs (pos, seq, kind, dest, hash, sig, content)
         VALUES (0, 0, 0, 1, x'0000000000000000000000000000000000000000000000000000000000000000', ?1, x'00')",
            [bad_sig],
        )
        .unwrap();

    let err = logger.read_range(0, 0).unwrap_err();
    assert!(matches!(err, LogError::InvalidSignatureLength(32)));
}

// ============================================================================
// Edge Cases & Stress Tests
// ============================================================================

#[test]
fn test_max_lines_of_one() {
    let mut logger = make_logger(1);

    let hash0 = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"first");
    let sig0 = test_signature(0, &hash0);
    logger.log(MessageType::Send, 1, b"first", sig0).unwrap();

    let hash1 = compute_expected_hash(hash0, 1, MessageType::Send, b"second");
    let sig1 = test_signature(1, &hash1);
    logger.log(MessageType::Send, 1, b"second", sig1).unwrap();

    // Only the second entry should exist
    let result = logger.read_range(0, 1);
    assert!(matches!(result, Err(LogError::MissingRows { .. })));

    let entries = logger.read_range(1, 1).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, b"second");
}

#[test]
fn test_sequential_logging_performance() {
    let mut logger = make_logger(1000);

    let mut prev_hash = [0u8; 32];
    for i in 0..100 {
        let msg = format!("message_{}", i);
        let hash = compute_expected_hash(prev_hash, i, MessageType::Send, msg.as_bytes());
        let sig = test_signature(i, &hash);
        let seq = logger
            .log(MessageType::Send, i as u32, msg.as_bytes(), sig)
            .unwrap();
        assert_eq!(seq, i);
        prev_hash = hash;
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
        let hash0 = compute_expected_hash([0u8; 32], 0, MessageType::Send, b"a");
        let sig0 = test_signature(0, &hash0);
        logger.log(MessageType::Send, 1, b"a", sig0).unwrap();

        let hash1 = compute_expected_hash(hash0, 1, MessageType::Send, b"b");
        let sig1 = test_signature(1, &hash1);
        logger.log(MessageType::Send, 2, b"b", sig1).unwrap();

        let hash2 = compute_expected_hash(hash1, 2, MessageType::Recv, b"c");
        let sig2 = test_signature(2, &hash2);
        logger.log(MessageType::Recv, 3, b"c", sig2).unwrap();
    }

    // Second session: write 2 more
    {
        let mut logger = make_persistent_logger(path, 10);
        assert_eq!(logger.next_sequence(), 3);
        let prev = logger.last_hash();

        let hash3 = compute_expected_hash(prev, 3, MessageType::Send, b"d");
        let sig3 = test_signature(3, &hash3);
        logger.log(MessageType::Send, 4, b"d", sig3).unwrap();

        let hash4 = compute_expected_hash(hash3, 4, MessageType::Recv, b"e");
        let sig4 = test_signature(4, &hash4);
        logger.log(MessageType::Recv, 5, b"e", sig4).unwrap();
    }

    // Third session: verify all 5 entries
    {
        let logger = make_persistent_logger(path, 10);
        let entries = logger.read_range(0, 4).unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[4].content, b"e");
    }
}
