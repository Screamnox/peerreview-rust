use std::fmt;

use crate::types::NodeId;

/// Événements applicatifs à auditer (Monde A -> Monde B).
/// On reste volontairement générique + extensible.
#[derive(Debug, Clone)]
pub enum AuditEvent {
    AppSend {
        to: NodeId,
        tree_id: u32,
        msg_id: String,
        kind: String,
        bytes: usize,
    },
    AppRecv {
        from: NodeId,
        tree_id: u32,
        msg_id: String,
        kind: String,
        bytes: usize,
    },
    Heartbeat {
        tree_id: u32,
        counter: u64,
    },
    /// Pour tracer une anomalie locale (ex: chunk incomplet, msg invalide, etc.)
    AppError {
        what: String,
    },
}

impl AuditEvent {
    /// "kind" stable (utile pour agrégation / KPI).
    pub fn kind(&self) -> &'static str {
        match self {
            AuditEvent::AppSend { .. } => "APP_SEND",
            AuditEvent::AppRecv { .. } => "APP_RECV",
            AuditEvent::Heartbeat { .. } => "HEARTBEAT",
            AuditEvent::AppError { .. } => "APP_ERROR",
        }
    }
}

impl fmt::Display for AuditEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Payload humain + stable (loggable)
        match self {
            AuditEvent::AppSend { to, tree_id, msg_id, kind, bytes } => {
                write!(f, "to={} tree={} msg_id={} kind={} bytes={}", to, tree_id, msg_id, kind, bytes)
            }
            AuditEvent::AppRecv { from, tree_id, msg_id, kind, bytes } => {
                write!(f, "from={} tree={} msg_id={} kind={} bytes={}", from, tree_id, msg_id, kind, bytes)
            }
            AuditEvent::Heartbeat { tree_id, counter } => {
                write!(f, "tree={} counter={}", tree_id, counter)
            }
            AuditEvent::AppError { what } => {
                write!(f, "{}", what)
            }
        }
    }
}
