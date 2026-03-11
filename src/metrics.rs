#[derive(Debug, Clone)]
pub struct MetricsPlan {
    pub counters: &'static [&'static str],
}

impl Default for MetricsPlan {
    fn default() -> Self {
        Self {
            counters: &[
                "connections.accepted",
                "connections.active",
                "upstream.dial_failures",
                "subscription.translation_failures",
                "provider.rollback_activations",
            ],
        }
    }
}
