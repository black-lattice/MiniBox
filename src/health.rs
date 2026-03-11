#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    Liveness,
    Readiness,
}

impl ProbeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Liveness => "liveness",
            Self::Readiness => "readiness",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeStatus {
    Starting,
    Ready,
    Degraded,
}

impl ProbeStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeDescriptor {
    pub kind: ProbeKind,
    pub path: &'static str,
    pub success_status: ProbeStatus,
    pub dependencies: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeSnapshot {
    pub kind: ProbeKind,
    pub status: ProbeStatus,
    pub detail: &'static str,
}

impl ProbeSnapshot {
    pub const fn planned(kind: ProbeKind) -> Self {
        let detail = match kind {
            ProbeKind::Liveness => "process event loop initialized",
            ProbeKind::Readiness => "listeners bound and active config loaded",
        };

        Self {
            kind,
            status: ProbeStatus::Starting,
            detail,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HealthPlan {
    pub liveness: ProbeDescriptor,
    pub readiness: ProbeDescriptor,
}

impl Default for HealthPlan {
    fn default() -> Self {
        Self {
            liveness: LIVENESS_PROBE,
            readiness: READINESS_PROBE,
        }
    }
}

pub const LIVENESS_PROBE: ProbeDescriptor = ProbeDescriptor {
    kind: ProbeKind::Liveness,
    path: "/healthz",
    success_status: ProbeStatus::Ready,
    dependencies: &["process.runtime"],
};

pub const READINESS_PROBE: ProbeDescriptor = ProbeDescriptor {
    kind: ProbeKind::Readiness,
    path: "/readyz",
    success_status: ProbeStatus::Ready,
    dependencies: &["active_config", "listeners.bound"],
};

#[cfg(test)]
mod tests {
    use super::{HealthPlan, ProbeKind, ProbeSnapshot, ProbeStatus};

    #[test]
    fn health_plan_exposes_separate_probe_intents() {
        let plan = HealthPlan::default();

        assert_eq!(plan.liveness.path, "/healthz");
        assert_eq!(plan.readiness.path, "/readyz");
        assert_ne!(plan.liveness.dependencies, plan.readiness.dependencies);
    }

    #[test]
    fn probe_snapshots_start_in_planned_state() {
        let snapshot = ProbeSnapshot::planned(ProbeKind::Readiness);

        assert_eq!(snapshot.status, ProbeStatus::Starting);
        assert_eq!(snapshot.detail, "listeners bound and active config loaded");
    }
}
