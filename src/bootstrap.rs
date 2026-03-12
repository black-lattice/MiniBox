use std::path::{Path, PathBuf};

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
    listener::ListenerTaskHandle,
    runtime::RuntimeState,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupOptions {
    pub source: ExternalConfigSource,
    pub cache_path: Option<PathBuf>,
}

impl StartupOptions {
    pub fn from_source(source: ExternalConfigSource) -> Self {
        Self {
            cache_path: default_cache_path(&source),
            source,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StartupRuntime {
    pub plan: StartupPlan,
    pub activation: ConfigActivation,
    pub runtime: RuntimeState,
}

impl StartupRuntime {
    pub async fn spawn_accept_loops(&self) -> Result<Vec<ListenerTaskHandle>, Error> {
        self.runtime.spawn_accept_loops().await
    }
}

pub async fn load_startup_config(
    input: StartupConfigInput,
    cache: Option<&CacheStore>,
) -> Result<ConfigActivation, Error> {
    let adapter = ClashLevelBAdapter;

    match input {
        StartupConfigInput::Source(source) => {
            load_active_config_from_source(&adapter, cache, &source).await
        }
        StartupConfigInput::Document(document) => {
            load_active_config_from_document(&adapter, cache, &document)
        }
    }
}

pub async fn prepare_runtime(options: &StartupOptions) -> Result<StartupRuntime, Error> {
    let cache_store = options.cache_path.as_ref().map(CacheStore::new);
    let activation = load_startup_config(
        StartupConfigInput::Source(options.source.clone()),
        cache_store.as_ref(),
    )
    .await?;
    let runtime = RuntimeState::new(activation.active_config.clone());

    if runtime.listeners().listeners().is_empty() {
        return Err(Error::validation(format!(
            "startup source '{}' did not yield any listeners to serve",
            describe_source(&options.source)
        )));
    }

    Ok(StartupRuntime {
        plan: build_startup_plan(),
        activation,
        runtime,
    })
}

pub async fn run(options: StartupOptions) -> Result<(), Error> {
    let startup = prepare_runtime(&options).await?;
    emit_startup_logs(&startup, &options);

    let handles = startup.spawn_accept_loops().await?;
    for handle in &handles {
        emit_listener_bound(&startup.plan.operations, handle);
    }

    let _handles = handles;
    std::future::pending::<Result<(), Error>>().await
}

pub fn startup_source_from_arg(value: &str) -> ExternalConfigSource {
    if value.starts_with("http://") || value.starts_with("https://") {
        ExternalConfigSource::ClashSubscription {
            url: value.to_string(),
        }
    } else {
        ExternalConfigSource::LocalFile {
            path: value.to_string(),
        }
    }
}

pub fn default_cache_path(source: &ExternalConfigSource) -> Option<PathBuf> {
    match source {
        ExternalConfigSource::LocalFile { .. } => None,
        ExternalConfigSource::ClashSubscription { url } => Some(
            Path::new(".minibox")
                .join("cache")
                .join(format!("{}.yaml", sanitize_source_label(url))),
        ),
    }
}

fn sanitize_source_label(raw: &str) -> String {
    let mut label = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            label.push(ch.to_ascii_lowercase());
        } else if label.chars().last() != Some('-') {
            label.push('-');
        }
    }
    let trimmed = label.trim_matches('-');
    if trimmed.is_empty() {
        "subscription".to_string()
    } else {
        trimmed.to_string()
    }
}

fn emit_startup_logs(startup: &StartupRuntime, options: &StartupOptions) {
    let operations = &startup.plan.operations;
    if let Some(event) = operations.logging.event("startup.begin") {
        emit_log_event(
            event,
            &[
                ("phase", startup.plan.current_phase.to_string()),
                ("config_source", describe_source(&options.source)),
            ],
        );
    }

    if let Some(error) = &startup.activation.translation_error {
        if let Some(event) = operations.logging.event("subscription.translate_failed") {
            emit_log_event(
                event,
                &[
                    ("source", describe_source(&options.source)),
                    ("reason", error.to_string()),
                ],
            );
        }
    }

    if startup.activation.source == crate::subscription::ConfigActivationSource::LastKnownGoodCache {
        if let Some(event) = operations.logging.event("provider.cache_rollback_used") {
            emit_log_event(
                event,
                &[
                    ("provider", "startup-cache".to_string()),
                    (
                        "reason",
                        startup
                            .activation
                            .translation_error
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "translation fallback requested".to_string()),
                    ),
                ],
            );
        }
    }
}

fn emit_listener_bound(operations: &OperationsPlan, handle: &ListenerTaskHandle) {
    if let Some(event) = operations.logging.event("listener.bound") {
        emit_log_event(
            event,
            &[
                ("listener", handle.plan().name.clone()),
                ("protocol", format!("{:?}", handle.plan().protocol)),
                ("bind", handle.local_addr().to_string()),
            ],
        );
    }
}

fn emit_log_event(event: crate::logging::LogEvent, fields: &[(&str, String)]) {
    let mut line = format!(
        "level={} event={} message={:?}",
        event.level.as_str(),
        event.name,
        event.message
    );
    for (key, value) in fields {
        line.push(' ');
        line.push_str(key);
        line.push('=');
        line.push_str(&format!("{value:?}"));
    }
    eprintln!("{line}");
}

fn describe_source(source: &ExternalConfigSource) -> String {
    match source {
        ExternalConfigSource::LocalFile { path } => format!("local:{path}"),
        ExternalConfigSource::ClashSubscription { url } => format!("clash:{url}"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        StartupConfigInput, StartupOptions, default_cache_path, load_startup_config,
        prepare_runtime, startup_source_from_arg,
    };
    use crate::config::external::{
        ExternalConfig, ExternalConfigSource, ExternalDocument, ListenerInput,
        ListenerProtocolInput, NodeInput, TargetRefInput,
    };
    use crate::provider::cache::CacheStore;
    use crate::subscription::ConfigActivationSource;

    #[tokio::test]
    async fn startup_loads_local_file_source_directly() {
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
        .await
        .expect("local startup source should load");

        assert_eq!(activation.source, ConfigActivationSource::LocalFile);
        assert_eq!(
            activation.active_config.listeners()[0].bind,
            "127.0.0.1:1080"
        );
        assert_eq!(activation.translation_error, None);

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn startup_uses_fresh_clash_translation_when_document_is_valid() {
        let cache_path = temp_path("fresh-cache");
        let cache = CacheStore::new(cache_path.clone());

        let activation = load_startup_config(
            StartupConfigInput::Document(valid_clash_document()),
            Some(&cache),
        )
        .await
        .expect("valid Clash document should activate");

        assert_eq!(activation.source, ConfigActivationSource::FreshTranslation);
        assert_eq!(activation.active_config.nodes()[0].name, "edge-a");
        assert_eq!(activation.translation_error, None);
        assert!(cache.path().exists());

        let _ = fs::remove_file(cache_path);
    }

    #[tokio::test]
    async fn startup_falls_back_to_last_known_good_cache_when_translation_fails() {
        let cache_path = temp_path("rollback-cache");
        let cache = CacheStore::new(cache_path.clone());

        let fresh = load_startup_config(
            StartupConfigInput::Document(valid_clash_document()),
            Some(&cache),
        )
        .await
        .expect("seed translation should persist cache");
        assert_eq!(fresh.source, ConfigActivationSource::FreshTranslation);

        let activation = load_startup_config(
            StartupConfigInput::Document(invalid_clash_document()),
            Some(&cache),
        )
        .await
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

    #[tokio::test]
    async fn startup_uses_explicit_error_for_unsupported_https_source_loading() {
        let error = load_startup_config(
            StartupConfigInput::Source(ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string(),
            }),
            None,
        )
        .await
        .expect_err("https Clash source loading should stay outside this minimal stage");

        assert_eq!(
            error,
            crate::error::Error::unsupported(
                "https Clash subscription loading is not supported in this stage for 'https://example.com/subscription'; use http:// or preload the document",
            )
        );
    }

    #[tokio::test]
    async fn startup_prepare_runtime_builds_runtime_state_from_local_source() {
        let path = temp_path("runtime-startup");
        let config = ExternalConfig {
            listeners: vec![ListenerInput {
                name: "local-socks".to_string(),
                bind: "127.0.0.1:0".to_string(),
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
        fs::write(
            &path,
            serde_yaml::to_string(&config).expect("config should serialize"),
        )
        .expect("config file should be written");

        let startup = prepare_runtime(&StartupOptions {
            source: ExternalConfigSource::LocalFile {
                path: path.display().to_string(),
            },
            cache_path: None,
        })
        .await
        .expect("runtime should prepare");

        assert_eq!(startup.runtime.listeners().listeners().len(), 1);
        assert_eq!(startup.activation.source, ConfigActivationSource::LocalFile);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn startup_source_parser_keeps_boundary_simple() {
        assert_eq!(
            startup_source_from_arg("/tmp/minibox.yaml"),
            ExternalConfigSource::LocalFile {
                path: "/tmp/minibox.yaml".to_string()
            }
        );
        assert_eq!(
            startup_source_from_arg("http://example.com/subscription"),
            ExternalConfigSource::ClashSubscription {
                url: "http://example.com/subscription".to_string()
            }
        );
    }

    #[test]
    fn remote_sources_get_a_stable_default_cache_path() {
        let cache_path = default_cache_path(&ExternalConfigSource::ClashSubscription {
            url: "http://example.com/a?b=c".to_string(),
        })
        .expect("remote source should get a cache path");

        assert_eq!(
            cache_path,
            PathBuf::from(".minibox/cache/http-example-com-a-b-c.yaml")
        );
    }
}
