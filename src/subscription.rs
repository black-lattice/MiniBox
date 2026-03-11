use crate::config::external::ExternalConfigSource;

#[derive(Debug, Clone)]
pub struct SubscriptionPlan {
    pub source: ExternalConfigSource,
    pub cache_enabled: bool,
    pub rollback_enabled: bool,
}

impl Default for SubscriptionPlan {
    fn default() -> Self {
        Self {
            source: ExternalConfigSource::LocalFile {
                path: "config/example.yaml".to_string(),
            },
            cache_enabled: true,
            rollback_enabled: true,
        }
    }
}

pub fn describe_update_flow() -> &'static [&'static str] {
    &[
        "read external input",
        "translate into internal config",
        "validate internal snapshot",
        "persist last-known-good cache",
        "activate validated snapshot",
    ]
}
