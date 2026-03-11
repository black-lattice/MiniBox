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
        Self {
            namespace: "minibox",
            exposition_path: "/metrics",
            descriptors: DEFAULT_METRICS,
        }
    }
}

pub const DEFAULT_METRICS: &[MetricDescriptor] = &[
    MetricDescriptor {
        name: "connections.accepted",
        kind: MetricKind::Counter,
        description: "accepted downstream TCP sessions",
        labels: &["listener", "protocol"],
    },
    MetricDescriptor {
        name: "connections.active",
        kind: MetricKind::Gauge,
        description: "currently active downstream sessions",
        labels: &["listener", "protocol"],
    },
    MetricDescriptor {
        name: "upstream.dial_failures",
        kind: MetricKind::Counter,
        description: "upstream dial failures by coarse reason",
        labels: &["listener", "protocol", "reason"],
    },
    MetricDescriptor {
        name: "runtime.readiness",
        kind: MetricKind::Gauge,
        description: "runtime readiness state exposed through future readiness probes",
        labels: &["status"],
    },
    MetricDescriptor {
        name: "subscription.translation_failures",
        kind: MetricKind::Counter,
        description: "subscription translation failures",
        labels: &["source"],
    },
    MetricDescriptor {
        name: "provider.rollback_activations",
        kind: MetricKind::Counter,
        description: "activations of a last-known-good provider cache",
        labels: &["provider"],
    },
];

#[cfg(test)]
mod tests {
    use super::{MetricKind, MetricsPlan};

    #[test]
    fn metrics_plan_stays_low_cardinality_and_namespaced() {
        let plan = MetricsPlan::default();

        assert_eq!(plan.namespace, "minibox");
        assert_eq!(plan.exposition_path, "/metrics");
        assert!(
            plan.descriptors
                .iter()
                .any(|metric| metric.name == "runtime.readiness")
        );
        assert!(
            plan.descriptors
                .iter()
                .any(|metric| metric.kind == MetricKind::Gauge)
        );
    }
}
