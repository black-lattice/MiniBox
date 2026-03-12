use crate::operations::OperationsPlan;
use crate::provider::cache::CacheStore;
use crate::subscription::{
    ConfigActivation, SubscriptionPlan, load_active_config_from_document,
    load_active_config_from_source,
};
use crate::{
    adapter::clash::ClashLevelBAdapter,
    config::external::{ExternalConfigSource, ExternalDocument},
    error::Error,
};

#[derive(Debug, Clone)]
pub struct StartupPlan {
    pub current_phase: &'static str,
    pub clash_support_boundary: &'static str,
    pub steps: &'static [&'static str],
    pub operations: OperationsPlan,
    pub subscription: SubscriptionPlan,
}

pub fn build_startup_plan() -> StartupPlan {
    StartupPlan {
        current_phase: "listener accept-loop + direct CONNECT proxy baseline",
        clash_support_boundary: "level B: nodes + groups, without full rule compatibility",
        steps: &[
            "validate internal config snapshot",
            "prepare listener registry and shared admission control",
            "bind configured listeners and accept downstream TCP sessions",
            "parse SOCKS5 and HTTP CONNECT requests into session targets",
            "add relay pipeline",
            "wire structured logging, metrics, and probe surfaces",
            "add Clash adapter and cache rollback",
        ],
        operations: OperationsPlan::default(),
        subscription: SubscriptionPlan::default(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupConfigInput {
    Source(ExternalConfigSource),
    Document(ExternalDocument),
}

pub fn load_startup_config(
    input: StartupConfigInput,
    cache: Option<&CacheStore>,
) -> Result<ConfigActivation, Error> {
    let adapter = ClashLevelBAdapter;

    match input {
        StartupConfigInput::Source(source) => {
            load_active_config_from_source(&adapter, cache, &source)
        }
        StartupConfigInput::Document(document) => {
            load_active_config_from_document(&adapter, cache, &document)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{StartupConfigInput, load_startup_config};
    use crate::config::external::{
        ExternalConfig, ExternalConfigSource, ExternalDocument, ListenerInput,
        ListenerProtocolInput, NodeInput, TargetRefInput,
    };
    use crate::provider::cache::CacheStore;
    use crate::subscription::ConfigActivationSource;

    #[test]
    fn startup_loads_local_file_source_directly() {
        let path = temp_path("local-startup");
        let config = ExternalConfig {
            listeners: vec![ListenerInput {
                name: "local-socks".to_string(),
                bind: "127.0.0.1:1080".to_string(),
                protocol: ListenerProtocolInput::Socks5,
                target: TargetRefInput::node("node-a"),
            }],
            nodes: vec![NodeInput {
                name: "node-a".to_string(),
                address: "1.1.1.1:443".to_string(),
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        };
        let encoded = serde_yaml::to_string(&config).expect("config should serialize");
        fs::write(&path, encoded).expect("startup config should be written");

        let activation = load_startup_config(
            StartupConfigInput::Source(ExternalConfigSource::LocalFile {
                path: path.display().to_string(),
            }),
            None,
        )
        .expect("local startup source should load");

        assert_eq!(activation.source, ConfigActivationSource::LocalFile);
        assert_eq!(
            activation.active_config.listeners()[0].bind,
            "127.0.0.1:1080"
        );
        assert_eq!(activation.translation_error, None);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn startup_uses_fresh_clash_translation_when_document_is_valid() {
        let cache_path = temp_path("fresh-cache");
        let cache = CacheStore::new(cache_path.clone());

        let activation = load_startup_config(
            StartupConfigInput::Document(valid_clash_document()),
            Some(&cache),
        )
        .expect("valid Clash document should activate");

        assert_eq!(activation.source, ConfigActivationSource::FreshTranslation);
        assert_eq!(activation.active_config.nodes()[0].name, "edge-a");
        assert_eq!(activation.translation_error, None);
        assert!(cache.path().exists());

        let _ = fs::remove_file(cache_path);
    }

    #[test]
    fn startup_falls_back_to_last_known_good_cache_when_translation_fails() {
        let cache_path = temp_path("rollback-cache");
        let cache = CacheStore::new(cache_path.clone());

        let fresh = load_startup_config(
            StartupConfigInput::Document(valid_clash_document()),
            Some(&cache),
        )
        .expect("seed translation should persist cache");
        assert_eq!(fresh.source, ConfigActivationSource::FreshTranslation);

        let activation = load_startup_config(
            StartupConfigInput::Document(invalid_clash_document()),
            Some(&cache),
        )
        .expect("startup should fall back to cache");

        assert_eq!(
            activation.source,
            ConfigActivationSource::LastKnownGoodCache
        );
        assert_eq!(activation.active_config.nodes()[0].name, "edge-a");
        assert!(activation.translation_error.is_some());

        let _ = fs::remove_file(cache_path);
    }

    fn valid_clash_document() -> ExternalDocument {
        ExternalDocument::new(
            ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string(),
            },
            r#"
proxies:
  - name: edge-a
    type: ss
    server: 1.1.1.1
    port: 443
proxy-groups:
  - name: primary
    type: select
    proxies:
      - edge-a
"#,
        )
    }

    fn invalid_clash_document() -> ExternalDocument {
        ExternalDocument::new(
            ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string(),
            },
            r#"
proxies:
  - name: edge-a
    type: ss
    server: 1.1.1.1
    port: 443
rules:
  - MATCH,DIRECT
"#,
        )
    }

    fn temp_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be monotonic enough for tests")
            .as_nanos();
        std::env::temp_dir().join(format!("minibox-{label}-{nonce}.yaml"))
    }

    #[test]
    fn startup_preserves_cache_loading_boundary_for_external_documents_only() {
        let error = load_startup_config(
            StartupConfigInput::Source(ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string(),
            }),
            None,
        )
        .expect_err("remote Clash source loading should stay outside this stage");

        assert_eq!(
            error,
            crate::error::Error::unimplemented(
                "loading Clash subscription source 'https://example.com/subscription' is not implemented yet; provide an ExternalDocument instead",
            )
        );
    }
}
