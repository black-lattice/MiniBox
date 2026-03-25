use std::path::{Path, PathBuf};
use std::pin::pin;
use std::time::Duration;

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
    health::ProbeStatus,
    listener::ListenerTaskHandle,
    logging::emit_log_event,
    runtime::RuntimeState,
};

const LISTENER_TASK_POLL_INTERVAL: Duration = Duration::from_millis(100);

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
    run_until(options, wait_for_shutdown_signal()).await
}

async fn run_until<F>(options: StartupOptions, shutdown_signal: F) -> Result<(), Error>
where
    F: std::future::Future<Output = Result<(), Error>>,
{
    let startup = prepare_runtime(&options).await?;
    emit_startup_logs(&startup, &options);

    let mut handles = match startup.spawn_accept_loops().await {
        Ok(handles) => handles,
        Err(error) => {
            emit_runtime_readiness_changed(
                &startup.plan.operations,
                ProbeStatus::Degraded,
                format!("listener startup failed: {error}"),
            );
            return Err(error);
        }
    };
    for handle in &handles {
        emit_listener_bound(&startup.plan.operations, handle);
    }
    emit_runtime_readiness_changed(
        &startup.plan.operations,
        ProbeStatus::Ready,
        format!("{} listener(s) bound", handles.len()),
    );

    match wait_for_runtime_stop(&startup.plan.operations, &mut handles, shutdown_signal).await {
        Ok(shutdown_reason) => {
            emit_runtime_readiness_changed(
                &startup.plan.operations,
                ProbeStatus::Degraded,
                shutdown_reason,
            );
            shutdown_listener_tasks(&mut handles).await
        }
        Err(error) => {
            let shutdown_result = shutdown_listener_tasks(&mut handles).await;
            shutdown_result?;
            Err(error)
        }
    }
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

    if startup.activation.source == crate::subscription::ConfigActivationSource::LastKnownGoodCache
    {
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

fn emit_runtime_readiness_changed(
    operations: &OperationsPlan,
    status: ProbeStatus,
    reason: String,
) {
    if let Some(event) = operations.logging.event("runtime.readiness_changed") {
        emit_log_event(
            event,
            &[("status", status.as_str().to_string()), ("reason", reason)],
        );
    }
}

async fn wait_for_runtime_stop<F>(
    operations: &OperationsPlan,
    handles: &mut Vec<ListenerTaskHandle>,
    shutdown_signal: F,
) -> Result<String, Error>
where
    F: std::future::Future<Output = Result<(), Error>>,
{
    let mut shutdown_signal = pin!(shutdown_signal);
    let mut poll_interval = tokio::time::interval(LISTENER_TASK_POLL_INTERVAL);
    poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            signal = &mut shutdown_signal => {
                signal?;
                return Ok("shutdown signal received".to_string());
            }
            _ = poll_interval.tick() => {
                if let Some(index) = handles.iter().position(ListenerTaskHandle::is_finished) {
                    let handle = handles.swap_remove(index);
                    let listener_name = handle.plan().name.clone();
                    let local_addr = handle.local_addr();
                    let failure = match handle.join().await {
                        Ok(Ok(())) => Error::io(format!(
                            "listener '{}' on '{}' stopped without an error, which should not happen",
                            listener_name,
                            local_addr
                        )),
                        Ok(Err(error)) => error,
                        Err(join_error) if join_error.is_cancelled() => Error::io(format!(
                            "listener '{}' on '{}' was cancelled unexpectedly",
                            listener_name,
                            local_addr
                        )),
                        Err(join_error) => Error::io(format!(
                            "listener '{}' on '{}' panicked: {join_error}",
                            listener_name,
                            local_addr
                        )),
                    };
                    emit_runtime_readiness_changed(
                        operations,
                        ProbeStatus::Degraded,
                        format!("listener '{}' failed: {failure}", listener_name),
                    );
                    return Err(failure);
                }
            }
        }
    }
}

async fn shutdown_listener_tasks(handles: &mut Vec<ListenerTaskHandle>) -> Result<(), Error> {
    for handle in handles.iter() {
        handle.abort();
    }

    while let Some(handle) = handles.pop() {
        let listener_name = handle.plan().name.clone();
        let local_addr = handle.local_addr();
        match handle.join().await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(join_error) if join_error.is_cancelled() => {}
            Err(join_error) => {
                return Err(Error::io(format!(
                    "listener '{}' on '{}' panicked during shutdown: {join_error}",
                    listener_name, local_addr
                )));
            }
        }
    }

    Ok(())
}

async fn wait_for_shutdown_signal() -> Result<(), Error> {
    tokio::signal::ctrl_c()
        .await
        .map_err(|error| Error::io(format!("failed to listen for shutdown signal: {error}")))
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
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        StartupConfigInput, StartupOptions, build_startup_plan, default_cache_path,
        load_startup_config, prepare_runtime, run_until, shutdown_listener_tasks,
        startup_source_from_arg, wait_for_runtime_stop,
    };
    use crate::config::external::{
        ExternalConfig, ExternalConfigSource, ExternalDocument, ListenerInput,
        ListenerProtocolInput, NodeInput, TargetRefInput,
    };
    use crate::config::internal::{ProtocolKind, TargetRef};
    use crate::listener::{
        ListenerAdmissionPlan, ListenerHandler, ListenerPlan, ListenerTaskHandle,
    };
    use crate::operations::OperationsPlan;
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

    #[tokio::test]
    async fn startup_prepare_runtime_accepts_repository_example_config() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/example.yaml");

        let startup = prepare_runtime(&StartupOptions {
            source: ExternalConfigSource::LocalFile {
                path: path.display().to_string(),
            },
            cache_path: None,
        })
        .await
        .expect("repository example config should prepare a runtime");

        assert_eq!(startup.activation.source, ConfigActivationSource::LocalFile);
        assert_eq!(startup.runtime.listeners().listeners().len(), 1);
        assert_eq!(
            startup.runtime.active_config().listeners()[0].name,
            "local-socks"
        );
        assert_eq!(
            startup.runtime.active_config().nodes()[0].name,
            "default-upstream"
        );
    }

    #[tokio::test]
    async fn startup_prepare_runtime_rejects_configs_without_listeners() {
        let path = temp_path("runtime-no-listeners");
        let config = ExternalConfig {
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

        let error = prepare_runtime(&StartupOptions {
            source: ExternalConfigSource::LocalFile {
                path: path.display().to_string(),
            },
            cache_path: None,
        })
        .await
        .expect_err("startup should reject configs that do not serve listeners");

        assert_eq!(
            error,
            crate::error::Error::validation(format!(
                "startup source 'local:{}' did not yield any listeners to serve",
                path.display()
            ))
        );

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
    fn startup_plan_defaults_to_repository_example_config() {
        assert_eq!(
            build_startup_plan().subscription.source,
            ExternalConfigSource::LocalFile {
                path: "config/example.yaml".to_string()
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

    #[tokio::test]
    async fn runtime_wait_returns_shutdown_reason_when_signal_arrives() {
        let mut handles = vec![spawn_idle_listener_task("local-socks", 1080)];
        let reason =
            wait_for_runtime_stop(&OperationsPlan::default(), &mut handles, async { Ok(()) })
                .await
                .expect("shutdown signal should stop runtime");

        assert_eq!(reason, "shutdown signal received");
        shutdown_listener_tasks(&mut handles)
            .await
            .expect("listener shutdown should succeed");
    }

    #[tokio::test]
    async fn runtime_wait_propagates_listener_failure() {
        let mut handles = vec![spawn_failed_listener_task("local-socks", 1080)];
        let error = wait_for_runtime_stop(
            &OperationsPlan::default(),
            &mut handles,
            std::future::pending::<Result<(), crate::error::Error>>(),
        )
        .await
        .expect_err("listener failure should stop runtime");

        assert_eq!(
            error,
            crate::error::Error::io(
                "listener 'local-socks' (Socks5) on '127.0.0.1:1080' failed while accepting downstream connections: boom"
            )
        );
        shutdown_listener_tasks(&mut handles)
            .await
            .expect("shutdown of remaining listeners should succeed");
    }

    #[tokio::test]
    async fn runtime_run_until_honors_shutdown_signal() {
        let path = temp_path("runtime-run-until");
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

        match run_until(
            StartupOptions {
                source: ExternalConfigSource::LocalFile {
                    path: path.display().to_string(),
                },
                cache_path: None,
            },
            async { Ok(()) },
        )
        .await
        {
            Ok(()) => {}
            Err(crate::error::Error::Io(message))
                if message.contains("Operation not permitted") =>
            {
                let _ = fs::remove_file(path);
                return;
            }
            Err(error) => panic!("runtime should exit cleanly when shutdown is requested: {error}"),
        }

        let _ = fs::remove_file(path);
    }

    fn spawn_idle_listener_task(name: &str, port: u16) -> ListenerTaskHandle {
        let plan = test_listener_plan(name, port);
        let local_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let task =
            tokio::spawn(async { std::future::pending::<Result<(), crate::error::Error>>().await });
        ListenerTaskHandle::new(plan, local_addr, task)
    }

    fn spawn_failed_listener_task(name: &str, port: u16) -> ListenerTaskHandle {
        let plan = test_listener_plan(name, port);
        let local_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let error = crate::error::Error::io(format!(
            "listener '{}' ({:?}) on '{}' failed while accepting downstream connections: boom",
            name,
            ProtocolKind::Socks5,
            local_addr
        ));
        let task = tokio::spawn(async move { Err(error) });
        ListenerTaskHandle::new(plan, local_addr, task)
    }

    fn test_listener_plan(name: &str, port: u16) -> ListenerPlan {
        ListenerPlan {
            name: name.to_string(),
            bind: format!("127.0.0.1:{port}"),
            protocol: ProtocolKind::Socks5,
            target: TargetRef::Node("node-a".to_string()),
            handler: ListenerHandler::Socks5,
            admission: ListenerAdmissionPlan { shared_limit: 64 },
        }
    }
}
