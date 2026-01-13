use ed25519_dalek::SigningKey;
use peerreview_protocol::journal::logger::Logger;
use tempfile::tempdir;

fn tamper_hash_field_in_json_line(line: &str) -> String {
    // We tamper the first number inside the `"hash":[ ... ]` array
    // while keeping valid JSON. This MUST make signature verification fail,
    // because signature is over the stored hash bytes.

    let needle = "\"hash\":[";
    let start = line
        .find(needle)
        .expect("hash field not found")
        + needle.len();

    // Find end of the first integer (until ',' or ']')
    let rest = &line[start..];
    let end_rel = rest
        .find(|c: char| c == ',' || c == ']')
        .expect("hash array seems malformed");

    let first_num_str = &rest[..end_rel];
    let first_num: i32 = first_num_str
        .trim()
        .parse()
        .expect("failed to parse first hash element as int");

    // Flip by 1 within [0..255] range
    let new_num = if first_num == 255 { 254 } else { first_num + 1 };

    // Rebuild string with replaced first integer
    let mut out = String::new();
    out.push_str(&line[..start]);
    out.push_str(&new_num.to_string());
    out.push_str(&rest[end_rel..]);
    out
}

#[test]
fn log_verify_ok_and_tamper_detected() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("node1.log");

    // Keypair
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let vk = sk.verifying_key();

    // Write a few entries
    {
        let mut lg = Logger::open(1, &path, sk).unwrap();

        lg.log_pr_in("PR_IN hello").unwrap();
        lg.log_pr_out("PR_OUT ping").unwrap();

        lg.log_app("SEND", Some(2), "m1".to_string(), [1u8; 32], 123)
            .unwrap();
        lg.log_app("RECV", Some(2), "m2".to_string(), [2u8; 32], 124)
            .unwrap();
        lg.log_app("DELIVER", None, "m3".to_string(), [3u8; 32], 125)
            .unwrap();
    }

    // Verify should succeed (strict_chain=false for this unit test)
    Logger::verify_log_file(&path, &vk, false).unwrap();

    // Tamper: modify the stored hash in the first non-empty line
    let contents = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<String> = contents.lines().map(|s| s.to_string()).collect();

    // Find first non-empty line and tamper it
    let idx = lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .expect("log is empty");
    lines[idx] = tamper_hash_field_in_json_line(&lines[idx]);

    let tampered = lines.join("\n") + "\n";
    std::fs::write(&path, tampered).unwrap();

    // Now verification MUST fail (signature no longer matches stored hash bytes)
    let err = Logger::verify_log_file(&path, &vk, false)
        .err()
        .expect("tamper not detected");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}
