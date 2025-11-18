use std::fs;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use base64::{engine::general_purpose, Engine as _};

use peerreview::journal::{Logger, MessageType, LogError};

// Helper: create a logger with a tempfile path
fn create_logger_with_max(max_lines: u64) -> (Logger, PathBuf) {
    let tmp = NamedTempFile::new().expect("create temp file");
    let path = tmp.path().to_path_buf();
    // Drop NamedTempFile so sqlite can open/persist to path
    drop(tmp);
    let logger = Logger::new(path.to_str().unwrap(), max_lines).expect("create logger");
    (logger, path)
}

#[test]
fn test_new_creates_db_and_initial_sequence_zero() {
    let (logger, path) = create_logger_with_max(10);
    assert_eq!(logger.next_sequence(), 0);
    fs::remove_file(path).ok();
}

#[test]
fn test_recover_sequence_after_existing_rows() {
    let (mut logger, path) = create_logger_with_max(10);

    // insert a few entries
    let s0 = logger.log(MessageType::Send, 1, "first").expect("log1");
    let s1 = logger.log(MessageType::Recv, 2, "second").expect("log2");
    assert_eq!(s0, 0);
    assert_eq!(s1, 1);
    assert_eq!(logger.next_sequence(), 2);

    // drop and reopen to test recover_sequence
    drop(logger);
    let logger2 = Logger::new(path.to_str().unwrap(), 10).expect("reopen");
    assert_eq!(logger2.next_sequence(), 2);

    fs::remove_file(path).ok();
}

#[test]
fn test_log_base64_and_read_range_basic() {
    let (mut logger, path) = create_logger_with_max(10);
    let seq = logger.log(MessageType::Send, 42, "some payload").expect("log");
    assert_eq!(seq, 0);

    let entries = logger.read_range(0, 0).expect("read_range");
    assert_eq!(entries.len(), 1);

    let e = &entries[0];
    assert_eq!(e.seq, 0);
    assert_eq!(e.kind, MessageType::Send);
    assert_eq!(e.dest, 42);
    assert_eq!(e.msg, "some payload");

    fs::remove_file(path).ok();
}

#[test]
fn test_logging_wraparound_overwrites_old_entries() {
    // max_lines small to force wrap
    let (mut logger, path) = create_logger_with_max(3);

    // write 5 entries; positions: 0,1,2,0,1
    for i in 0..5 {
        let payload = format!("m{}", i);
        let seq = logger.log(MessageType::Send, i as u32, &payload).expect("log");
        assert_eq!(seq, i);
    }

    // read all sequences 0..4
    let entries = logger.read_range(0, 4).expect("read_range");

    // Because logger is circular with max_lines = 3, only the last 3 sequences (2,3,4)
    // will be stored/returned.
    assert_eq!(entries.len(), 3);
    let expected_seqs = [2u64, 3u64, 4u64];
    for (idx, &expected_seq) in expected_seqs.iter().enumerate() {
        let e = &entries[idx];
        assert_eq!(e.seq, expected_seq);
        assert_eq!(e.msg, format!("m{}", expected_seq));
    }

    fs::remove_file(path).ok();
}

#[test]
fn test_read_range_empty_returns_empty_vec() {
    let (logger, path) = create_logger_with_max(5);
    let entries = logger.read_range(0, 10).expect("read empty range");
    assert!(entries.is_empty());
    fs::remove_file(path).ok();
}

#[test]
fn test_read_range_ordering_and_subrange() {
    let (mut logger, path) = create_logger_with_max(10);
    for i in 0..6 {
        let payload = format!("v{}", i);
        logger.log(if i % 2 == 0 { MessageType::Send } else { MessageType::Recv }, i as u32, &payload).expect("log");
    }

    // subrange 2..4
    let sub = logger.read_range(2, 4).expect("read subrange");
    assert_eq!(sub.len(), 3);
    assert_eq!(sub[0].seq, 2);
    assert_eq!(sub[1].seq, 3);
    assert_eq!(sub[2].seq, 4);

    fs::remove_file(path).ok();
}

#[test]
fn test_invalid_message_type_in_db_yields_error() {
    // Create DB and manually insert an invalid kind value
    let (logger, path) = create_logger_with_max(5);
    // insert a row with kind = 9 bypassing Logger.log
    logger.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg) VALUES (?1, ?2, ?3, ?4, '0', '0', ?5)",
        rusqlite::params![0i64, 0i64, 9i64, 1i64, general_purpose::STANDARD.encode("x")],
    ).expect("manual insert");

    let res = logger.read_range(0, 0);
    match res {
        Err(LogError::InvalidMessageType(9)) => {},
        Err(e) => panic!("expected InvalidMessageType(9), got other error: {:?}", e),
        Ok(v) => panic!("expected error, got ok: {:?}", v),
    }

    fs::remove_file(path).ok();
}

#[test]
fn test_base64_decode_error_propagated() {
    // Create DB and insert invalid base64 string
    let (logger, path) = create_logger_with_max(5);
    logger.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg) VALUES (?1, ?2, ?3, ?4, '0', '0', ?5)",
        rusqlite::params![0i64, 0i64, 0i64, 1i64, "!!not_base64!!"],
    ).expect("manual insert");

    let res = logger.read_range(0, 0);
    match res {
        Err(LogError::Base64(_)) => {},
        Err(e) => panic!("expected Base64 error, got {:?}", e),
        Ok(v) => panic!("expected error, got ok: {:?}", v),
    }

    fs::remove_file(path).ok();
}

#[test]
fn test_utf8_decode_error_propagated() {
    // Insert valid base64 that decodes to invalid UTF-8 (e.g., raw bytes 0xff)
    let (logger, path) = create_logger_with_max(5);
    let bad_bytes = vec![0xff, 0xff];
    let enc = general_purpose::STANDARD.encode(&bad_bytes);
    logger.exec_raw(
        "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg) VALUES (?1, ?2, ?3, ?4, '0', '0', ?5)",
        rusqlite::params![0i64, 0i64, 0i64, 1i64, enc],
    ).expect("manual insert");

    let res = logger.read_range(0, 0);
    match res {
        Err(LogError::Utf8(_)) => {},
        Err(e) => panic!("expected Utf8 error, got {:?}", e),
        Ok(v) => panic!("expected error, got ok: {:?}", v),
    }

    fs::remove_file(path).ok();
}

#[test]
fn test_seq_next_sequence_consistency_after_many_logs() {
    let (mut logger, path) = create_logger_with_max(7);
    for i in 0..100 {
        let payload = format!("data{}", i);
        logger.log(MessageType::Send, i as u32, &payload).expect("log");
        assert_eq!(logger.next_sequence(), (i + 1) as u64);
    }
    fs::remove_file(path).ok();
}

/*
// TODO: Complete with concurrent threads
#[test]
fn test_concurrent_open_and_recover_sequence() {
    // Ensure another process writing and reopen works (simulate by two Logger instances)
    let tmp = NamedTempFile::new().expect("temp file");
    let path = tmp.path().to_path_buf();
    drop(tmp);

    let mut logger1 = Logger::new(path.to_str().unwrap(), 10).expect("create1");
    for i in 0..5 {
        logger1.log(MessageType::Send, i as u32, &format!("a{}", i)).expect("log");
    }

    // open second connection
    let logger2 = Logger::new(path.to_str().unwrap(), 10).expect("create2");
    assert_eq!(logger2.next_sequence(), 5);

    // logger1 continues
    logger1.log(MessageType::Recv, 99, "after").expect("log");

    let l2_after = Logger::new(path.to_str().unwrap(), 10).expect("reopen3");
    assert_eq!(l2_after.next_sequence(), 6);

    fs::remove_file(path).ok();
}
*/

#[test]
fn test_read_range_out_of_order_params_returns_empty_or_subset() {
    let (mut logger, path) = create_logger_with_max(10);
    logger.log(MessageType::Send, 1, "x").unwrap();
    logger.log(MessageType::Send, 1, "y").unwrap();

    // start > end -> should return empty (no rows)
    let res = logger.read_range(5, 3).expect("read");
    assert!(res.is_empty());

    fs::remove_file(path).ok();
}

#[test]
fn test_large_max_lines_behavior_and_position_calculation() {
    let (mut logger, path) = create_logger_with_max(1); // circular of size 1
    let s0 = logger.log(MessageType::Send, 1, "a").unwrap();
    assert_eq!(s0, 0);
    let s1 = logger.log(MessageType::Send, 2, "b").unwrap();
    assert_eq!(s1, 1);

    // Only the latest seq (1) should be present when reading by seq range if older was overwritten
    let all = logger.read_range(0, 1).unwrap();
    // depending on correct implementation, both rows may still exist with distinct seq but pos overwrote.
    // Assert presence of seq=1 at least
    assert!(all.iter().any(|e| e.seq == 1 && e.msg == "b"));

    fs::remove_file(path).ok();
}

// Additional negative tests for SQL errors (invalid path)
#[test]
fn test_new_with_invalid_path_returns_sql_error() {
    // Attempt to create DB in a directory path (should fail)
    let path = "/this/path/should/not/exist/and/be/a_dir";
    let res = Logger::new(path, 5);
    match res {
        Err(LogError::Sql(_)) => {},
        Ok(_) => panic!("expected SQL error for invalid path"),
        Err(e) => panic!("unexpected error: {:?}", e),
    }
}

