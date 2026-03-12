use crate::adapter::clash::ClashLevelBAdapter;
use crate::config::external::ExternalConfigSource;
use crate::config::external::ExternalDocument;
use crate::config::internal::ActiveConfig;
use crate::config::load::{load_local_document, read_source_document};
use crate::error::Error;
use crate::provider::cache::{CacheActivation, CacheActivationSource, CacheStore};

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
) -> Result<ActiveConfig, Error> {
    adapter.translate(document)
}

pub fn ingest_clash_document_with_cache(
    adapter: &ClashLevelBAdapter,
    cache: &CacheStore,
    document: &ExternalDocument,
) -> Result<CacheActivation, Error> {
    cache.activate_candidate(adapter.translate(document))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigActivationSource {
    LocalFile,
    FreshTranslation,
    LastKnownGoodCache,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigActivation {
    pub active_config: ActiveConfig,
    pub source: ConfigActivationSource,
    pub translation_error: Option<Error>,
}

pub async fn load_active_config_from_source(
    adapter: &ClashLevelBAdapter,
    cache: Option<&CacheStore>,
    source: &ExternalConfigSource,
) -> Result<ConfigActivation, Error> {
    let document = read_source_document(source).await?;
    load_active_config_from_document(adapter, cache, &document)
}

pub fn load_active_config_from_document(
    adapter: &ClashLevelBAdapter,
    cache: Option<&CacheStore>,
    document: &ExternalDocument,
) -> Result<ConfigActivation, Error> {
    match &document.source {
        ExternalConfigSource::LocalFile { .. } => Ok(ConfigActivation {
            active_config: load_local_document(document)?,
            source: ConfigActivationSource::LocalFile,
            translation_error: None,
        }),
        ExternalConfigSource::ClashSubscription { .. } => {
            if let Some(cache) = cache {
                map_cache_activation(ingest_clash_document_with_cache(adapter, cache, document))
            } else {
                Ok(ConfigActivation {
                    active_config: ingest_clash_document(adapter, document)?,
                    source: ConfigActivationSource::FreshTranslation,
                    translation_error: None,
                })
            }
        }
    }
}

fn map_cache_activation(
    activation: Result<CacheActivation, Error>,
) -> Result<ConfigActivation, Error> {
    activation.map(|activation| ConfigActivation {
        active_config: activation.active_config,
        source: match activation.source {
            CacheActivationSource::FreshTranslation => ConfigActivationSource::FreshTranslation,
            CacheActivationSource::LastKnownGoodCache => ConfigActivationSource::LastKnownGoodCache,
        },
        translation_error: activation.translation_error,
    })
}
