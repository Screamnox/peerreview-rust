use thiserror::Error;

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

    #[error("Invalid signature length: expected 64, got {0}")]
    InvalidSignatureLength(usize),

    #[error("Value too large for i64: {0}")]
    ValueTooLarge(u64),

    #[error("Requested rows missing: requested={requested}, found={found}")]
    MissingRows { requested: u64, found: usize },

    #[error("Hash verification failed at seq={seq}")]
    HashVerificationFailed { seq: u64 },

    #[error("Signature verification failed at seq={seq}")]
    SignatureVerificationFailed { seq: u64 },

    #[error("Circular buffer overflow: next_seq would exceed u64::MAX")]
    SequenceOverflow,

    #[error("Invalid authenticator length: expected 108, got {0}")]
    InvalidAuthenticatorLength(usize),

    #[error("No signing key available for this logger")]
    NoSigningKey,

    #[error("Signature error: {0}")]
    SignatureError(#[from] ed25519_dalek::SignatureError),
}

pub type Result<T> = std::result::Result<T, LogError>;
