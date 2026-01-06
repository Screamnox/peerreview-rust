use std::time::Duration;

/// Scheduler minimal pour déclencher des audits périodiques.
/// (On enrichira ensuite en suivant l’article : choix aléatoire, fenêtres de logs, etc.)
pub struct AuditScheduler {
    period: Duration,
}

impl AuditScheduler {
    pub fn new(period: Duration) -> Self {
        Self { period }
    }

    pub fn period(&self) -> Duration {
        self.period
    }
}
