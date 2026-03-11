use crate::adapter::clash::ClashLevelBAdapter;
use crate::config::external::ExternalConfigSource;
use crate::config::external::ExternalDocument;
use crate::error::Error;
use crate::provider::cache::{CacheActivation, CacheStore};

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

pub fn ingest_clash_document(
    adapter: &ClashLevelBAdapter,
    document: &ExternalDocument,
) -> Result<crate::config::internal::ActiveConfig, Error> {
    adapter.translate(document)
}

pub fn ingest_clash_document_with_cache(
    adapter: &ClashLevelBAdapter,
    cache: &CacheStore,
    document: &ExternalDocument,
) -> Result<CacheActivation, Error> {
    cache.activate_candidate(adapter.translate(document))
}
