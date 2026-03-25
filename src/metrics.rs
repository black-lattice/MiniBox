use crate::health::ProbeStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    Counter,
    Gauge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricDescriptor {
    pub name: &'static str,
    pub kind: MetricKind,
    pub description: &'static str,
    pub labels: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct MetricsPlan {
    pub namespace: &'static str,
    pub exposition_path: &'static str,
    pub descriptors: &'static [MetricDescriptor],
}

impl Default for MetricsPlan {
    fn default() -> Self {
        Self { namespace: "minibox", exposition_path: "/metrics", descriptors: DEFAULT_METRICS }
    }
}

pub const DEFAULT_METRICS: &[MetricDescriptor] = &[
    MetricDescriptor {
        name: "connections.active",
        kind: MetricKind::Gauge,
        description: "currently active downstream TCP sessions",
        labels: &[],
    },
    MetricDescriptor {
        name: "connections.capacity",
        kind: MetricKind::Gauge,
        description: "configured downstream connection capacity",
        labels: &[],
    },
    MetricDescriptor {
        name: "listeners.bound",
        kind: MetricKind::Gauge,
        description: "currently bound downstream listeners",
        labels: &[],
    },
    MetricDescriptor {
        name: "runtime.readiness",
        kind: MetricKind::Gauge,
        description: "runtime readiness state exposed through readiness probes",
        labels: &["status"],
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeMetricsSnapshot {
    pub active_connections: usize,
    pub max_connections: usize,
    pub bound_listeners: usize,
    pub readiness: ProbeStatus,
}

pub fn render_prometheus_text(snapshot: &RuntimeMetricsSnapshot) -> String {
    format!(
        concat!(
            "# HELP minibox_connections_active currently active downstream TCP sessions\n",
            "# TYPE minibox_connections_active gauge\n",
            "minibox_connections_active {}\n",
            "# HELP minibox_connections_capacity configured downstream connection capacity\n",
            "# TYPE minibox_connections_capacity gauge\n",
            "minibox_connections_capacity {}\n",
            "# HELP minibox_listeners_bound currently bound downstream listeners\n",
            "# TYPE minibox_listeners_bound gauge\n",
            "minibox_listeners_bound {}\n",
            "# HELP minibox_runtime_readiness runtime readiness state exposed through readiness probes\n",
            "# TYPE minibox_runtime_readiness gauge\n",
            "minibox_runtime_readiness{{status=\"starting\"}} {}\n",
            "minibox_runtime_readiness{{status=\"ready\"}} {}\n",
            "minibox_runtime_readiness{{status=\"degraded\"}} {}\n"
        ),
        snapshot.active_connections,
        snapshot.max_connections,
        snapshot.bound_listeners,
        usize::from(snapshot.readiness == ProbeStatus::Starting),
        usize::from(snapshot.readiness == ProbeStatus::Ready),
        usize::from(snapshot.readiness == ProbeStatus::Degraded),
    )
}

#[cfg(test)]
mod tests {
    use super::{MetricKind, MetricsPlan, RuntimeMetricsSnapshot, render_prometheus_text};
    use crate::health::ProbeStatus;

    #[test]
    fn metrics_plan_stays_low_cardinality_and_namespaced() {
        let plan = MetricsPlan::default();

        assert_eq!(plan.namespace, "minibox");
        assert_eq!(plan.exposition_path, "/metrics");
        assert!(plan.descriptors.iter().any(|metric| metric.name == "runtime.readiness"));
        assert!(plan.descriptors.iter().any(|metric| metric.kind == MetricKind::Gauge));
    }

    #[test]
    fn renders_prometheus_text_for_runtime_snapshot() {
        let rendered = render_prometheus_text(&RuntimeMetricsSnapshot {
            active_connections: 3,
            max_connections: 64,
            bound_listeners: 2,
            readiness: ProbeStatus::Ready,
        });

        assert!(rendered.contains("minibox_connections_active 3"));
        assert!(rendered.contains("minibox_connections_capacity 64"));
        assert!(rendered.contains("minibox_listeners_bound 2"));
        assert!(rendered.contains("minibox_runtime_readiness{status=\"ready\"} 1"));
    }
}
