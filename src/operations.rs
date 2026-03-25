use crate::health::HealthPlan;
use crate::logging::LoggingPlan;
use crate::metrics::MetricsPlan;

#[derive(Debug, Clone, Default)]
pub struct OperationsPlan {
    pub logging: LoggingPlan,
    pub metrics: MetricsPlan,
    pub health: HealthPlan,
}

impl OperationsPlan {
    pub fn summary(&self) -> &'static str {
        "typed log events, a minimal Prometheus surface, and health/readiness probes are available."
    }
}

#[cfg(test)]
mod tests {
    use super::OperationsPlan;

    #[test]
    fn operations_plan_groups_observability_surfaces() {
        let plan = OperationsPlan::default();

        assert_eq!(plan.metrics.exposition_path, "/metrics");
        assert_eq!(plan.health.readiness.path, "/readyz");
        assert!(!plan.logging.events.is_empty());
    }
}
