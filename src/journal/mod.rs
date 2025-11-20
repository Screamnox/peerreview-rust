use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Message type: SEND or RECV
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Send = 0,
    Recv = 1,
}

impl TryFrom<i64> for MessageType {
    type Error = LogError;

    fn try_from(v: i64) -> Result<Self> {
        match v {
            0 => Ok(MessageType::Send),
            1 => Ok(MessageType::Recv),
            invalid => Err(LogError::InvalidMessageType(invalid)),
        }
    }
}

/// Log entry
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub seq: u64,
    pub kind: MessageType,
    pub dest: u32,
    pub hash: Vec<u8>, // Recursive Hash (4.4 - PeerReview)
    pub sig: String,   // Log Signature
    pub msg: Vec<u8>,  // Data (raw bytes)
}

/// Error type for Logger
#[derive(Error, Debug)]
pub enum LogError {
    #[error("SQLite error: {0}")]
    Sql(#[from] rusqlite::Error),

    #[error("Invalid message type: {0}")]
    InvalidMessageType(i64),

    #[error("Invalid sequence value in DB: {0}")]
    InvalidSequence(i64),

    #[error("Invalid destination value in DB: {0}")]
    InvalidDest(i64),

    #[error("Invalid hash length: expected 32, got {0}")]
    InvalidHashLength(usize),

    #[error("Value too large for i64: {0}")]
    ValueTooLarge(u64),

    #[error("Requested rows missing: requested={requested}, found={found}")]
    MissingRows { requested: u64, found: usize },

    #[error("Hash verification failed at seq={seq}")]
    HashVerificationFailed { seq: u64 },

    #[error("Circular buffer overflow: next_seq would exceed u64::MAX")]
    SequenceOverflow,
}

pub type Result<T> = std::result::Result<T, LogError>;

/// Circular logger using SQLite with cryptographic integrity verification.
///
/// # Thread Safety
/// This logger is NOT thread-safe. Wrap in `Arc<Mutex<Logger>>` for multi-threaded use.
///
/// # Example
/// ```no_run
/// use peerreview::journal::{Logger, MessageType, LogError};
///
/// let mut logger = Logger::new("log.db", 1000)?;
/// let seq = logger.log(MessageType::Send, 42, b"Hello, world!")?;
/// let entries = logger.read_range(seq, seq)?;
/// # Ok::<(), LogError>(())
/// ```
#[derive(Debug)]
pub struct Logger {
    conn: Connection,
    max_lines: u64,
    next_seq: u64,
    last_hash: [u8; 32],
}

impl Logger {
    /// Create or open a circular logger.
    ///
    /// # Arguments
    /// * `db_path` - Path to SQLite database file
    /// * `max_lines` - Maximum number of entries before circular overwrite
    ///
    /// # Errors
    /// Returns error if database cannot be opened, initialized, or is corrupted
    pub fn new(db_path: &str, max_lines: u64) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create table if not exists
        // Note: INTEGER corresponds to i64
        // Future: integer would be also BLOB
        conn.execute(
            "CREATE TABLE IF NOT EXISTS logs (
                pos        INTEGER PRIMARY KEY,
                seq        INTEGER NOT NULL,
                kind       INTEGER NOT NULL,
                dest       INTEGER NOT NULL,
                hash       BLOB NOT NULL,
                sig        TEXT NOT NULL,
                msg        BLOB NOT NULL
            )",
            [],
        )?;

        // Add index to speed up seq-range queries
        conn.execute("CREATE INDEX IF NOT EXISTS idx_logs_seq ON logs(seq)", [])?;

        let next_seq = Self::recover_sequence(&conn)?;
        let last_hash = Self::recover_last_hash(&conn)?;

        Ok(Self {
            conn,
            max_lines,
            next_seq,
            last_hash,
        })
    }

    /// Recover the next sequence number from log table
    fn recover_sequence(conn: &Connection) -> Result<u64> {
        // MAX(seq) returns NULL when no rows exist, which maps to Option<i64>
        let max_seq_opt: Option<i64> =
            conn.query_row("SELECT MAX(seq) FROM logs", [], |row| row.get(0))?;

        let next = match max_seq_opt {
            None => 0u64,
            Some(s) if s < 0 => return Err(LogError::InvalidSequence(s)),
            Some(s) => (s as u64)
                .checked_add(1)
                .ok_or(LogError::SequenceOverflow)?,
        };

        Ok(next)
    }

    /// Recover the last hash from log table
    fn recover_last_hash(conn: &Connection) -> Result<[u8; 32]> {
        let hash_opt: Option<Vec<u8>> = conn
            .query_row(
                "SELECT hash FROM logs ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(h) = hash_opt {
            if h.len() != 32 {
                return Err(LogError::InvalidHashLength(h.len()));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            Ok(arr)
        } else {
            // Future: Change default hash by a well-known value
            Ok([0u8; 32]) // h_{-1}
        }
    }

    /// Helper: Convert u64 to i64 with overflow check
    fn to_i64_checked(val: u64) -> Result<i64> {
        i64::try_from(val).map_err(|_| LogError::ValueTooLarge(val))
    }

    /// Log a new message with cryptographic chaining.
    ///
    /// # Arguments
    /// * `kind` - Message type (Send or Recv)
    /// * `dest` - Destination identifier (Peer ID)
    /// * `msg` - Raw message content
    ///
    /// # Returns
    /// The sequence number assigned to this log entry
    ///
    /// # Errors
    /// Returns error if database write fails or sequence number would overflow
    pub fn log(&mut self, kind: MessageType, dest: u32, msg: &[u8]) -> Result<u64> {
        // Check for sequence overflow before logging
        if self.next_seq == u64::MAX {
            return Err(LogError::SequenceOverflow);
        }

        // position and sequence are stored as i64 in SQLite
        let pos_i64 = Self::to_i64_checked(self.next_seq % self.max_lines)?;
        let seq_i64 = Self::to_i64_checked(self.next_seq)?;
        let kind_i64 = kind as i64;
        let dest_i64 = dest as i64;

        // Hash content
        // TODO: Maybe rename msg -> content
        let content_hash = Sha256::digest(msg);

        // Recursive hash: H(last_hash || seq || kind || H(msg))
        // TODO: Check if H(msg) ?= H(c_k)
        let mut hasher = Sha256::new();
        hasher.update(self.last_hash);
        hasher.update(seq_i64.to_be_bytes());
        hasher.update([kind_i64 as u8]);
        hasher.update(content_hash);
        let new_hash = hasher.finalize();

        // TODO: Calculate actual signature
        let sig = "0";

        self.conn.execute(
            "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(pos) DO UPDATE SET
                seq  = excluded.seq,
                kind = excluded.kind,
                dest = excluded.dest,
                hash = excluded.hash,
                sig  = excluded.sig,
                msg  = excluded.msg;",
            params![
                pos_i64,
                seq_i64,
                kind_i64,
                dest_i64,
                new_hash.as_slice(),
                sig,
                msg
            ],
        )?;

        self.last_hash.copy_from_slice(&new_hash);

        let ret = self.next_seq;
        self.next_seq = self
            .next_seq
            .checked_add(1)
            .ok_or(LogError::SequenceOverflow)?;
        Ok(ret)
    }

    /// Read a range of logs [start_seq, end_seq].
    ///
    /// # Arguments
    /// * `start_seq` - Starting sequence number (inclusive)
    /// * `end_seq` - Ending sequence number (inclusive)
    ///
    /// # Returns
    /// Vector of log entries in sequence order
    ///
    /// # Errors
    /// * Returns `MissingRows` if not all requested entries are found
    /// * Returns database errors if query fails
    pub fn read_range(&self, start_seq: u64, end_seq: u64) -> Result<Vec<LogEntry>> {
        if start_seq > end_seq {
            return Ok(Vec::new());
        }

        let mut stmt = self.conn.prepare(
            "SELECT seq, kind, dest, hash, sig, msg
             FROM logs
             WHERE seq BETWEEN ?1 AND ?2
             ORDER BY seq",
        )?;

        let start_i64 = Self::to_i64_checked(start_seq)?;
        let end_i64 = Self::to_i64_checked(end_seq)?;
        let mut rows = stmt.query(params![start_i64, end_i64])?;

        let mut entries = Vec::new();

        while let Some(row) = rows.next()? {
            let seq_i64: i64 = row.get(0)?;
            let kind_i64: i64 = row.get(1)?;
            let dest_i64: i64 = row.get(2)?;
            let hash: Vec<u8> = row.get(3)?;
            let sig: String = row.get(4)?;
            let msg: Vec<u8> = row.get(5)?;

            let seq = if seq_i64 < 0 {
                return Err(LogError::InvalidSequence(seq_i64));
            } else {
                seq_i64 as u64
            };

            let kind = MessageType::try_from(kind_i64)?;

            let dest = if dest_i64 < 0 {
                return Err(LogError::InvalidDest(dest_i64));
            } else {
                dest_i64 as u32
            };

            if hash.len() != 32 {
                return Err(LogError::InvalidHashLength(hash.len()));
            }

            entries.push(LogEntry {
                seq,
                kind,
                dest,
                hash,
                sig,
                msg,
            });
        }

        let expected_count = (end_seq - start_seq + 1) as usize;
        if entries.len() != expected_count {
            return Err(LogError::MissingRows {
                requested: expected_count as u64,
                found: entries.len(),
            });
        }

        Ok(entries)
    }

    /// Verify cryptographic integrity of a log entry against its predecessor.
    ///
    /// # Arguments
    /// * `entry` - The log entry to verify
    /// * `prev_hash` - Hash from the previous entry in the chain
    ///
    /// # Returns
    /// `true` if the entry's hash is valid, `false` otherwise
    pub fn verify_entry(&self, entry: &LogEntry, prev_hash: &[u8; 32]) -> bool {
        let content_hash = Sha256::digest(&entry.msg);

        let mut hasher = Sha256::new();
        hasher.update(prev_hash);
        hasher.update(entry.seq.to_be_bytes());
        hasher.update([entry.kind as u8]);
        hasher.update(content_hash);
        let computed = hasher.finalize();

        computed.as_slice() == entry.hash
    }

    /// Verify integrity of a range of log entries.
    ///
    /// This checks that each entry's hash correctly chains to the previous entry.
    ///
    /// # Arguments
    /// * `entries` - Slice of consecutive log entries to verify
    /// * `initial_hash` - Hash of the entry before the first entry in the slice
    ///
    /// # Returns
    /// `Ok(())` if all entries are valid, or an error indicating which entry failed
    pub fn verify_range(&self, entries: &[LogEntry], initial_hash: [u8; 32]) -> Result<()> {
        let mut prev_hash = initial_hash;

        for entry in entries {
            if !self.verify_entry(entry, &prev_hash) {
                return Err(LogError::HashVerificationFailed { seq: entry.seq });
            }

            // Update prev_hash for next iteration
            if entry.hash.len() != 32 {
                return Err(LogError::InvalidHashLength(entry.hash.len()));
            }
            prev_hash.copy_from_slice(&entry.hash);
        }

        Ok(())
    }

    /// Future Implementation:
    /// Verify integrity of the entire log chain from a starting sequence.
    ///
    /// # Arguments
    /// * `start_seq` - Sequence number to start verification from
    ///
    /// # Returns
    /// `Ok(())` if the chain is valid, or an error indicating where verification failed
    ///
    pub fn verify_from(&self, start_seq: u64) -> Result<()> {
        unimplemented!("{}", start_seq)
    }

    /// Get the next sequence number that will be assigned
    pub fn next_sequence(&self) -> u64 {
        self.next_seq
    }

    /// Get the last hash in the chain
    pub fn last_hash(&self) -> [u8; 32] {
        self.last_hash
    }

    /// Execute a raw SQL request -- TEST ONLY
    // #[cfg(test)]
    pub fn exec_raw<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<()> {
        self.conn.execute(sql, params)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_logging() -> Result<()> {
        let mut logger = Logger::new(":memory:", 100)?;

        let seq1 = logger.log(MessageType::Send, 1, b"Hello")?;
        assert_eq!(seq1, 0);

        let seq2 = logger.log(MessageType::Recv, 2, b"World")?;
        assert_eq!(seq2, 1);

        Ok(())
    }

    #[test]
    fn test_read_range() -> Result<()> {
        let mut logger = Logger::new(":memory:", 100)?;

        logger.log(MessageType::Send, 1, b"msg1")?;
        logger.log(MessageType::Send, 2, b"msg2")?;
        logger.log(MessageType::Recv, 3, b"msg3")?;

        let entries = logger.read_range(0, 2)?;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].msg, b"msg1");
        assert_eq!(entries[2].msg, b"msg3");

        Ok(())
    }

    #[test]
    fn test_missing_rows_error() -> Result<()> {
        let mut logger = Logger::new(":memory:", 100)?;

        logger.log(MessageType::Send, 1, b"msg1")?;
        logger.log(MessageType::Send, 2, b"msg2")?;

        // Try to read non-existent range
        let result = logger.read_range(5, 10);
        assert!(matches!(result, Err(LogError::MissingRows { .. })));

        Ok(())
    }

    #[test]
    fn test_verify_entry() -> Result<()> {
        let mut logger = Logger::new(":memory:", 100)?;

        let initial_hash = logger.last_hash();
        logger.log(MessageType::Send, 1, b"test message")?;

        let entries = logger.read_range(0, 0)?;
        assert_eq!(entries.len(), 1);

        // Should verify correctly
        assert!(logger.verify_entry(&entries[0], &initial_hash));

        Ok(())
    }

    #[test]
    fn test_verify_range() -> Result<()> {
        let mut logger = Logger::new(":memory:", 100)?;

        logger.log(MessageType::Send, 1, b"msg1")?;
        logger.log(MessageType::Send, 2, b"msg2")?;
        logger.log(MessageType::Recv, 3, b"msg3")?;

        let entries = logger.read_range(0, 2)?;
        logger.verify_range(&entries, [0u8; 32])?;

        Ok(())
    }

    #[test]
    fn test_circular_overwrite() -> Result<()> {
        let mut logger = Logger::new(":memory:", 3)?;

        logger.log(MessageType::Send, 1, b"msg1")?;
        logger.log(MessageType::Send, 2, b"msg2")?;
        logger.log(MessageType::Send, 3, b"msg3")?;
        logger.log(MessageType::Send, 4, b"msg4")?; // Overwrites first entry

        // Original entry 0 should be missing
        let result = logger.read_range(0, 3);
        assert!(matches!(result, Err(LogError::MissingRows { .. })));

        // But we can read the newer entries
        let entries = logger.read_range(1, 3)?;
        assert_eq!(entries.len(), 3);

        Ok(())
    }
}
