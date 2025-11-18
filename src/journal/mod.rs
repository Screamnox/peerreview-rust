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
    pub hash: Vec<u8>, // Future: Recursive Hash (4.4 - PeerReview)
    pub sig: String,   // Future: Log Signature
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

    #[error("Requested rows missing: requested={requested}, found={found}")]
    MissingRows { requested: u64, found: usize },
}

pub type Result<T> = std::result::Result<T, LogError>;

/// Circular logger using SQLite
#[derive(Debug)]
pub struct Logger {
    conn: Connection,
    max_lines: u64,
    next_seq: u64,
    last_hash: [u8; 32],
}

impl Logger {
    /// Create or open a logger
    pub fn new(db_path: &str, max_lines: u64) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create table if not exists
        // Future: integer would be also BLOB
        // Note: INTEGER corresponds to i64
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

        /*
        // Add index to speed up seq-range queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_logs_seq ON logs(seq)",
            [],
        )?;
        */

        let next_seq = Self::recover_sequence(&conn)?;
        let last_hash = Self::recover_last_hash(&conn)?;

        Ok(Self {
            conn,
            max_lines,
            next_seq,
            last_hash,
        })
    }

    /// Recover the next sequence number in log table
    fn recover_sequence(conn: &Connection) -> Result<u64> {
        // MAX(seq) returns NULL when no rows exist, which maps to Option<i64>
        let max_seq_opt: Option<i64> =
            conn.query_row("SELECT MAX(seq) FROM logs", [], |row| row.get(0))?;

        let next = match max_seq_opt {
            None => 0u64,
            Some(s) if s < 0 => return Err(LogError::InvalidSequence(s)),
            Some(s) => (s as u64).wrapping_add(1),
        };

        Ok(next)
    }

    /// Recover the last hash in log table
    fn recover_last_hash(conn: &Connection) -> Result<[u8; 32]> {
        let hash_opt: Option<Vec<u8>> = conn
            .query_row(
                "SELECT hash FROM logs ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(h) = hash_opt {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            Ok(arr)
        } else {
            // Future: Change default hash by a well-known value
            Ok([0u8; 32]) // h_{-1}
        }
    }

    // TODO: Method to check integrity of each row

    /// Log a new message
    pub fn log(&mut self, kind: MessageType, dest: u32, msg: &[u8]) -> Result<u64> {
        // position and sequence are stored as i64 in SQLite
        let pos_i64 = (self.next_seq % self.max_lines) as i64;
        let seq_i64 = self.next_seq as i64;
        let kind_i64 = kind as i64;
        let dest_i64 = dest as i64;

        // TODO: Maybe rename msg -> content
        // Hash content
        let content_hash = Sha256::digest(msg);

        // TODO: Check if H(msg) ?= H(c_k)
        // Recursive hash: H(last_hash || seq || kind || H(msg))
        let mut hasher = Sha256::new();
        hasher.update(self.last_hash);
        hasher.update(seq_i64.to_be_bytes());
        hasher.update([kind_i64 as u8]);
        hasher.update(content_hash);
        let new_hash = hasher.finalize();

        // Future: Calculate signature
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
        self.next_seq = self.next_seq.wrapping_add(1);
        Ok(ret)
    }

    /// Read a range of logs inclusive [start_seq, end_seq]
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

        let mut rows = stmt.query(params![start_seq as i64, end_seq as i64])?;

        let mut entries = Vec::new();

        while let Some(row) = rows.next()? {
            let seq_i64: i64 = row.get(0)?;
            let kind_i64: i64 = row.get(1)?;
            let dest_i64: i64 = row.get(2)?;
            let hash: Vec<u8> = row.get(3)?;
            let sig: String = row.get(4)?;
            let msg: Vec<u8> = row.get(5)?;

            let kind = MessageType::try_from(kind_i64)?;

            let seq = if seq_i64 < 0 {
                return Err(LogError::InvalidSequence(seq_i64));
            } else {
                seq_i64 as u64
            };

            let dest = if dest_i64 < 0 {
                return Err(LogError::InvalidDest(dest_i64));
            } else {
                dest_i64 as u32
            };

            entries.push(LogEntry {
                seq,
                kind,
                dest,
                hash,
                sig,
                msg,
            });
        }

        Ok(entries)
    }

    /// Get the next sequence number
    pub fn next_sequence(&self) -> u64 {
        self.next_seq
    }

    /// Get the last hash
    pub fn last_hash(&self) -> [u8; 32] {
        self.last_hash
    }

    /// Execute a raw SQL request -- TEST ONLY
    pub fn exec_raw<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<()> {
        self.conn.execute(sql, params)?;
        Ok(())
    }
}
