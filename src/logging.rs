#[derive(Debug, Clone)]
pub struct LoggingPlan {
    pub default_level: &'static str,
    pub events: &'static [&'static str],
}

impl Default for LoggingPlan {
    fn default() -> Self {
        Self {
            default_level: "info",
            events: &[
                "startup.begin",
                "listener.bound",
                "session.closed",
                "subscription.translate_failed",
                "provider.cache_rollback_used",
            ],
        }
    }
}
