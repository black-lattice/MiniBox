use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::internal::{ActiveConfig, ConfigSnapshot};
use crate::error::Error;

const SNAPSHOT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct CacheStore {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheActivationSource {
    FreshTranslation,
    LastKnownGoodCache,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheActivation {
    pub active_config: ActiveConfig,
    pub source: CacheActivationSource,
    pub translation_error: Option<Error>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSnapshot {
    version: u32,
    snapshot: ConfigSnapshot,
}

impl CacheStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_last_known_good(&self) -> Result<Option<ActiveConfig>, Error> {
        if !self.path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(&self.path)?;
        let persisted: PersistedSnapshot = serde_yaml::from_str(raw.as_str()).map_err(|error| {
            Error::validation(format!(
                "failed to parse last-known-good snapshot '{}': {error}",
                self.path.display()
            ))
        })?;

        if persisted.version != SNAPSHOT_FORMAT_VERSION {
            return Err(Error::validation(format!(
                "unsupported snapshot format version {} in '{}'",
                persisted.version,
                self.path.display()
            )));
        }

        ActiveConfig::new(persisted.snapshot).map(Some)
    }

    pub fn store_validated_snapshot(&self, config: &ActiveConfig) -> Result<(), Error> {
        config.validate()?;

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let persisted = PersistedSnapshot {
            version: SNAPSHOT_FORMAT_VERSION,
            snapshot: config.snapshot().clone(),
        };
        let encoded = serde_yaml::to_string(&persisted).map_err(|error| {
            Error::validation(format!(
                "failed to serialize last-known-good snapshot '{}': {error}",
                self.path.display()
            ))
        })?;
        let temp_path = self.temp_path();
        fs::write(&temp_path, encoded)?;
        fs::rename(&temp_path, &self.path)?;
        Ok(())
    }

    pub fn activate_candidate(
        &self,
        candidate: Result<ActiveConfig, Error>,
    ) -> Result<CacheActivation, Error> {
        match candidate {
            Ok(active_config) => {
                self.store_validated_snapshot(&active_config)?;
                Ok(CacheActivation {
                    active_config,
                    source: CacheActivationSource::FreshTranslation,
                    translation_error: None,
                })
            }
            Err(translation_error) => match self.load_last_known_good() {
                Ok(Some(active_config)) => Ok(CacheActivation {
                    active_config,
                    source: CacheActivationSource::LastKnownGoodCache,
                    translation_error: Some(translation_error),
                }),
                Ok(None) => Err(translation_error),
                Err(cache_error) => Err(Error::validation(format!(
                    "subscription translation failed and last-known-good cache could not be loaded: {cache_error}; original error: {translation_error}",
                ))),
            },
        }
    }

    fn temp_path(&self) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("minibox-cache");
        self.path
            .with_file_name(format!("{file_name}.{}.tmp", std::process::id()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{CacheActivationSource, CacheStore};
    use crate::config::external::{
        ExternalConfig, GroupInput, GroupStrategyInput, NodeInput, SubscriptionInput,
        TargetRefInput,
    };
    use crate::config::internal::{ActiveConfig, TargetRef};
    use crate::error::Error;

    #[test]
    fn cache_round_trips_validated_snapshot() {
        let path = temp_cache_path("round-trip");
        let store = CacheStore::new(path.clone());
        let active = sample_active_config();

        store
            .store_validated_snapshot(&active)
            .expect("validated snapshot should persist");

        let restored = store
            .load_last_known_good()
            .expect("persisted snapshot should load")
            .expect("snapshot should exist");

        assert_eq!(restored, active);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn cache_rolls_back_to_last_known_good_when_translation_fails() {
        let path = temp_cache_path("rollback");
        let store = CacheStore::new(path.clone());
        let active = sample_active_config();
        let translation_error = Error::unsupported(
            "Clash rule-level semantics are not supported at level B: found top-level 'rules'",
        );

        store
            .store_validated_snapshot(&active)
            .expect("validated snapshot should persist");

        let activation = store
            .activate_candidate(Err(translation_error.clone()))
            .expect("rollback should activate cached snapshot");

        assert_eq!(activation.source, CacheActivationSource::LastKnownGoodCache);
        assert_eq!(activation.active_config, active);
        assert_eq!(activation.translation_error, Some(translation_error));

        let _ = fs::remove_file(path);
    }

    fn sample_active_config() -> ActiveConfig {
        ActiveConfig::from_external(ExternalConfig {
            nodes: vec![NodeInput {
                name: "node-a".to_string(),
                address: "1.1.1.1:443".to_string(),
                provider: None,
                subscription: Some("clash-subscription".to_string()),
            }],
            groups: vec![GroupInput {
                name: "primary".to_string(),
                strategy: GroupStrategyInput::Select,
                members: vec![TargetRefInput::node("node-a")],
                provider: None,
                subscription: Some("clash-subscription".to_string()),
            }],
            subscriptions: vec![SubscriptionInput {
                name: "clash-subscription".to_string(),
                source: crate::config::external::ExternalConfigSource::ClashSubscription {
                    url: "https://example.com/subscription".to_string(),
                },
            }],
            ..ExternalConfig::default()
        })
        .expect("sample config should normalize")
    }

    fn temp_cache_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be monotonic enough for tests")
            .as_nanos();
        std::env::temp_dir().join(format!("minibox-{label}-{nonce}.yaml"))
    }

    #[test]
    fn cache_fresh_activation_persists_snapshot() {
        let path = temp_cache_path("fresh");
        let store = CacheStore::new(path.clone());
        let active = sample_active_config();

        let activation = store
            .activate_candidate(Ok(active.clone()))
            .expect("fresh translation should activate");

        assert_eq!(activation.source, CacheActivationSource::FreshTranslation);
        assert_eq!(activation.active_config, active);
        assert_eq!(activation.translation_error, None);

        let restored = store
            .load_last_known_good()
            .expect("fresh activation should persist cache")
            .expect("snapshot should exist");
        assert_eq!(restored.groups()[0].members, vec![TargetRef::Node("node-a".to_string())]);

        let _ = fs::remove_file(path);
    }
}
