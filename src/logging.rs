use std::sync::OnceLock;

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

static ACTIVE_LOGGING_PLAN: OnceLock<LoggingPlan> = OnceLock::new();

impl Default for LoggingPlan {
    fn default() -> Self {
        Self { default_level: LogLevel::Info, events: DEFAULT_LOG_EVENTS }
    }
}

impl LoggingPlan {
    pub fn event(&self, name: &str) -> Option<LogEvent> {
        self.events.iter().copied().find(|event| event.name == name)
    }
}

pub fn install_logging_plan(plan: LoggingPlan) -> &'static LoggingPlan {
    ACTIVE_LOGGING_PLAN.get_or_init(|| plan)
}

pub fn logging_plan() -> &'static LoggingPlan {
    ACTIVE_LOGGING_PLAN.get_or_init(LoggingPlan::default)
}

pub fn logging_event(name: &str) -> Option<LogEvent> {
    logging_plan().event(name)
}

pub fn default_log_event(name: &str) -> Option<LogEvent> {
    logging_event(name)
}

pub fn emit_log_event(event: LogEvent, fields: &[(&str, String)]) {
    eprintln!("{}", format_log_line(event, fields));
}

pub fn emit_log_event_by_name(name: &str, fields: &[(&str, String)]) -> bool {
    let Some(event) = logging_event(name) else {
        return false;
    };
    emit_log_event(event, fields);
    true
}

pub fn format_log_line(event: LogEvent, fields: &[(&str, String)]) -> String {
    let mut line =
        format!("level={} event={} message={:?}", event.level.as_str(), event.name, event.message);
    for (key, value) in fields {
        line.push(' ');
        line.push_str(key);
        line.push('=');
        line.push_str(&format!("{value:?}"));
    }
    line
}

pub const DEFAULT_LOG_EVENTS: &[LogEvent] = &[
    LogEvent {
        name: "startup.begin",
        level: LogLevel::Info,
        message: "MiniBox startup plan accepted",
        fields: &["phase", "config_source"],
    },
    LogEvent {
        name: "startup.activated",
        level: LogLevel::Info,
        message: "startup activation completed and listeners are ready",
        fields: &[
            "source",
            "activated",
            "cache_rollback",
            "listeners",
            "admin_enabled",
            "admin_bind",
        ],
    },
    LogEvent {
        name: "startup.failed",
        level: LogLevel::Error,
        message: "startup or runtime exited before a clean shutdown",
        fields: &["phase", "source", "reason"],
    },
    LogEvent {
        name: "listener.bound",
        level: LogLevel::Info,
        message: "listener bound and accepting downstream sessions",
        fields: &["listener", "protocol", "bind", "target"],
    },
    LogEvent {
        name: "admin.bound",
        level: LogLevel::Info,
        message: "admin listener bound and serving probe endpoints",
        fields: &["bind", "healthz", "readyz", "metrics"],
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
        name: "runtime.failed",
        level: LogLevel::Error,
        message: "runtime exited because a listener failed or shutdown could not complete",
        fields: &["phase", "source", "reason"],
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
    use super::{
        LogLevel, LoggingPlan, emit_log_event_by_name, format_log_line, install_logging_plan,
        logging_event,
    };

    #[test]
    fn logging_plan_exposes_structured_event_descriptors() {
        let plan = LoggingPlan::default();

        assert_eq!(plan.default_level, LogLevel::Info);
        assert!(plan.events.iter().any(|event| event.name == "runtime.readiness_changed"));
        assert!(plan.events.iter().all(|event| !event.fields.is_empty()));
    }

    #[test]
    fn format_log_line_renders_level_event_message_and_fields() {
        let event = LoggingPlan::default().event("startup.begin").expect("event should exist");
        let line = format_log_line(
            event,
            &[("phase", "bootstrap".to_string()), ("config_source", "local".to_string())],
        );

        assert!(line.contains("level=info"));
        assert!(line.contains("event=startup.begin"));
        assert!(line.contains("message=\"MiniBox startup plan accepted\""));
        assert!(line.contains("phase=\"bootstrap\""));
        assert!(line.contains("config_source=\"local\""));
    }

    #[test]
    fn install_logging_plan_exposes_event_lookup_via_global_state() {
        let plan = LoggingPlan::default();
        let installed = install_logging_plan(plan);
        assert_eq!(installed.default_level, LogLevel::Info);

        let event =
            logging_event("admin.bound").expect("installed plan should expose admin events");
        assert_eq!(event.name, "admin.bound");
        assert!(emit_log_event_by_name("admin.bound", &[("bind", "127.0.0.1:9090".to_string())]));
        assert!(!emit_log_event_by_name("missing.event", &[]));
    }
}
