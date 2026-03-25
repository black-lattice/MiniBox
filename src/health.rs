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

        Self { kind, status: ProbeStatus::Starting, detail }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub kind: ProbeKind,
    pub status: ProbeStatus,
    pub detail: String,
}

impl ProbeReport {
    pub fn http_status_code(&self) -> u16 {
        match (self.kind, self.status) {
            (ProbeKind::Liveness, _) => 200,
            (_, ProbeStatus::Ready) => 200,
            _ => 503,
        }
    }

    pub fn render_text_body(&self) -> String {
        format!(
            "kind={}\nstatus={}\ndetail={}\n",
            self.kind.as_str(),
            self.status.as_str(),
            self.detail
        )
    }
}

#[derive(Debug, Clone)]
pub struct HealthPlan {
    pub liveness: ProbeDescriptor,
    pub readiness: ProbeDescriptor,
}

impl Default for HealthPlan {
    fn default() -> Self {
        Self { liveness: LIVENESS_PROBE, readiness: READINESS_PROBE }
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
    use super::{HealthPlan, ProbeKind, ProbeReport, ProbeSnapshot, ProbeStatus};

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

    #[test]
    fn readiness_report_maps_starting_state_to_unavailable_http_status() {
        let report = ProbeReport {
            kind: ProbeKind::Readiness,
            status: ProbeStatus::Starting,
            detail: "listeners not yet bound".to_string(),
        };

        assert_eq!(report.http_status_code(), 503);
        assert!(report.render_text_body().contains("status=starting"));
    }
}
