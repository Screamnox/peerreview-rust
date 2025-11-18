use base64::{Engine as _, engine::general_purpose};
use rusqlite::{Connection, params};
use thiserror::Error;

/// Message type: SEND or RECV
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Send = 0,
    Recv = 1,
}

/// Log entry
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub seq: u64,
    pub kind: MessageType,
    pub dest: u32,
    pub hash: String, // Recursive Hash (4.4 - PeerReview)
    pub sig: String,  // Log Signature
    pub msg: String,  // Data (decoded)
}

/// Error type for Logger
#[derive(Error, Debug)]
pub enum LogError {
    #[error("SQLite error: {0}")]
    Sql(#[from] rusqlite::Error),

    #[error("Base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("UTF8 decode error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("Invalid message type: {0}")]
    InvalidMessageType(u8),
}

pub type Result<T> = std::result::Result<T, LogError>;

/// Circular logger using SQLite
pub struct Logger {
    conn: Connection,
    max_lines: u64,
    next_seq: u64,
}

impl Logger {
    /// Create or open a logger
    pub fn new(db_path: &str, max_lines: u64) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create table if not exists
        // Use i64-compatible INTEGER columns for seq and dest
        conn.execute(
            "CREATE TABLE IF NOT EXISTS logs (
                pos        INTEGER PRIMARY KEY,
                seq        INTEGER NOT NULL,
                kind       INTEGER NOT NULL,
                dest       INTEGER NOT NULL,
                hash       TEXT NOT NULL,
                sig        TEXT NOT NULL,
                msg        TEXT NOT NULL
            )",
            [],
        )?;

        let next_seq = Self::recover_sequence(&conn)?;

        Ok(Self {
            conn,
            max_lines,
            next_seq,
        })
    }

    /// Recover the next sequence number in log table
    ///
    /// Important: MAX(seq) returns NULL if the table is empty.
    /// Request Option<i64> from the row to differentiate NULL vs value.
    fn recover_sequence(conn: &Connection) -> Result<u64> {
        // Get Option<i64> because MAX(seq) returns NULL when no rows exist.
        let max_seq_opt: Option<i64> =
            conn.query_row("SELECT MAX(seq) FROM logs", [], |row| row.get(0))?;

        let next = match max_seq_opt {
            None => 0u64,
            Some(s) if s < 0 => 0u64, // defensive: negative sequences are invalid
            Some(s) => (s as u64).saturating_add(1),
        };

        Ok(next)
    }

    /// Log a new message with base64 encoding
    pub fn log(&mut self, kind: MessageType, dest: u32, msg: &str) -> Result<u64> {
        // Encode message as base64 for storage
        let encoded = general_purpose::STANDARD.encode(msg.as_bytes());

        // position and sequence are stored as i64 in SQLite
        let position = (self.next_seq % self.max_lines) as i64;
        let sequence = self.next_seq as i64;
        let kind_i64 = kind as i64;
        let dest_i64 = dest as i64;

        // Use column names that exist in CREATE TABLE and excluded.<col> names matching those columns
        self.conn.execute(
            "INSERT INTO logs (pos, seq, kind, dest, hash, sig, msg)
             VALUES (?1, ?2, ?3, ?4, '0', '0', ?5)
             ON CONFLICT(pos) DO UPDATE SET
                seq  = excluded.seq,
                kind = excluded.kind,
                dest = excluded.dest,
                hash = excluded.hash,
                sig  = excluded.sig,
                msg  = excluded.msg;",
            params![position, sequence, kind_i64, dest_i64, encoded],
        )?;

        let ret = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        Ok(ret)
    }

    /// Read a range of logs inclusive [start_seq, end_seq]
    pub fn read_range(&self, start_seq: u64, end_seq: u64) -> Result<Vec<LogEntry>> {
        // If start > end, return empty (no rows)
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
            let hash: String = row.get(3)?;
            let sig: String = row.get(4)?;
            let encoded_message: String = row.get(5)?;

            let decoded_bytes = general_purpose::STANDARD.decode(encoded_message.as_bytes())?;
            let decoded_message = String::from_utf8(decoded_bytes)?;

            let kind_u8 = kind_i64 as u8;
            let kind = match kind_u8 {
                0 => MessageType::Send,
                1 => MessageType::Recv,
                invalid => return Err(LogError::InvalidMessageType(invalid)),
            };

            // Defensive casts (seq and dest came from i64 in DB)
            let seq = if seq_i64 < 0 { 0u64 } else { seq_i64 as u64 };
            let dest = if dest_i64 < 0 { 0u32 } else { dest_i64 as u32 };

            entries.push(LogEntry {
                seq,
                kind,
                dest,
                hash,
                sig,
                msg: decoded_message,
            });
        }

        Ok(entries)
    }

    /// Get the next sequence number
    pub fn next_sequence(&self) -> u64 {
        self.next_seq
    }

    /// Execute a raw SQL request -- TEST ONLY
    pub fn exec_raw<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<()> {
        self.conn.execute(sql, params)?;
        Ok(())
    }
}
