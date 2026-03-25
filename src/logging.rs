#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogEvent {
    pub name: &'static str,
    pub level: LogLevel,
    pub message: &'static str,
    pub fields: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct LoggingPlan {
    pub default_level: LogLevel,
    pub events: &'static [LogEvent],
}

impl Default for LoggingPlan {
    fn default() -> Self {
        Self {
            default_level: LogLevel::Info,
            events: DEFAULT_LOG_EVENTS,
        }
    }
}

impl LoggingPlan {
    pub fn event(&self, name: &str) -> Option<LogEvent> {
        self.events.iter().copied().find(|event| event.name == name)
    }
}

pub fn default_log_event(name: &str) -> Option<LogEvent> {
    DEFAULT_LOG_EVENTS
        .iter()
        .copied()
        .find(|event| event.name == name)
}

pub fn emit_log_event(event: LogEvent, fields: &[(&str, String)]) {
    let mut line = format!(
        "level={} event={} message={:?}",
        event.level.as_str(),
        event.name,
        event.message
    );
    for (key, value) in fields {
        line.push(' ');
        line.push_str(key);
        line.push('=');
        line.push_str(&format!("{value:?}"));
    }
    eprintln!("{line}");
}

pub const DEFAULT_LOG_EVENTS: &[LogEvent] = &[
    LogEvent {
        name: "startup.begin",
        level: LogLevel::Info,
        message: "MiniBox startup plan accepted",
        fields: &["phase", "config_source"],
    },
    LogEvent {
        name: "listener.bound",
        level: LogLevel::Info,
        message: "listener bound and accepting downstream sessions",
        fields: &["listener", "protocol", "bind"],
    },
    LogEvent {
        name: "session.closed",
        level: LogLevel::Info,
        message: "session relay finished",
        fields: &["listener", "protocol", "result"],
    },
    LogEvent {
        name: "runtime.readiness_changed",
        level: LogLevel::Warn,
        message: "runtime readiness status changed",
        fields: &["status", "reason"],
    },
    LogEvent {
        name: "subscription.translate_failed",
        level: LogLevel::Error,
        message: "external subscription translation failed",
        fields: &["source", "reason"],
    },
    LogEvent {
        name: "provider.cache_rollback_used",
        level: LogLevel::Warn,
        message: "last-known-good provider cache activated",
        fields: &["provider", "reason"],
    },
];

#[cfg(test)]
mod tests {
    use super::{LogLevel, LoggingPlan};

    #[test]
    fn logging_plan_exposes_structured_event_descriptors() {
        let plan = LoggingPlan::default();

        assert_eq!(plan.default_level, LogLevel::Info);
        assert!(
            plan.events
                .iter()
                .any(|event| event.name == "runtime.readiness_changed")
        );
        assert!(
            plan.events
                .iter()
                .all(|event| !event.fields.is_empty() || event.name == "startup.begin")
        );
    }
}
