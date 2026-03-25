use std::path::{Path, PathBuf};
use std::pin::pin;
use std::time::Duration;

use crate::admin::{AdminTaskHandle, spawn_admin_server};
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
    logging::{emit_log_event_by_name, install_logging_plan},
    runtime::RuntimeState,
};
use crate::{
    config::internal::{ConfigOrigin, NodeKind, TargetRef},
    subscription::ConfigActivationSource,
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
        current_phase: "subscription source + local listener template bootstrap",
        clash_support_boundary: "level B: nodes + groups, without full rule compatibility",
        steps: &[
            "validate internal config snapshot",
            "load subscription or local file source",
            "merge startup listener template with translated subscription content when needed",
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
        Self { cache_path: default_cache_path(&source), source }
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

    Ok(StartupRuntime { plan: build_startup_plan(), activation, runtime })
}

pub async fn run(options: StartupOptions) -> Result<(), Error> {
    run_until(options, wait_for_shutdown_signal()).await
}

async fn run_until<F>(options: StartupOptions, shutdown_signal: F) -> Result<(), Error>
where
    F: std::future::Future<Output = Result<(), Error>>,
{
    install_logging_plan(build_startup_plan().operations.logging.clone());
    let startup = match prepare_runtime(&options).await {
        Ok(startup) => startup,
        Err(error) => {
            emit_fatal_startup_failure("prepare_runtime", &options.source, &error);
            return Err(error);
        }
    };
    emit_startup_logs(&startup, &options);
    update_runtime_readiness(
        &startup.runtime,
        &startup.plan.operations,
        ProbeStatus::Starting,
        "startup accepted; listeners not yet bound".to_string(),
    );
    let mut admin_handle = match spawn_admin_server(startup.runtime.clone()).await {
        Ok(handle) => handle,
        Err(error) => {
            emit_fatal_startup_failure("spawn_admin_server", &options.source, &error);
            update_runtime_readiness(
                &startup.runtime,
                &startup.plan.operations,
                ProbeStatus::Degraded,
                format!("admin listener startup failed: {error}"),
            );
            return Err(error);
        }
    };
    emit_admin_bound(&startup.plan.operations, admin_handle.as_ref());

    let mut handles = match startup.spawn_accept_loops().await {
        Ok(handles) => handles,
        Err(error) => {
            emit_fatal_startup_failure("spawn_accept_loops", &options.source, &error);
            update_runtime_readiness(
                &startup.runtime,
                &startup.plan.operations,
                ProbeStatus::Degraded,
                format!("listener startup failed: {error}"),
            );
            shutdown_admin_task(&mut admin_handle).await?;
            return Err(error);
        }
    };
    emit_startup_activated(&startup, &options, admin_handle.as_ref(), &handles);
    for handle in &handles {
        emit_listener_bound(handle);
    }
    update_runtime_readiness(
        &startup.runtime,
        &startup.plan.operations,
        ProbeStatus::Ready,
        format!("{} listener(s) bound", handles.len()),
    );

    match wait_for_runtime_stop(&options.source, &mut handles, shutdown_signal).await {
        Ok(shutdown_reason) => {
            update_runtime_readiness(
                &startup.runtime,
                &startup.plan.operations,
                ProbeStatus::Degraded,
                shutdown_reason,
            );
            shutdown_listener_tasks(&mut handles).await?;
            shutdown_admin_task(&mut admin_handle).await
        }
        Err(error) => {
            emit_runtime_failure("listener_failure", &options.source, &error);
            let shutdown_result = shutdown_listener_tasks(&mut handles).await;
            let admin_shutdown = shutdown_admin_task(&mut admin_handle).await;
            shutdown_result?;
            admin_shutdown?;
            Err(error)
        }
    }
}

pub fn startup_source_from_arg(value: &str) -> ExternalConfigSource {
    if value.starts_with("http://") || value.starts_with("https://") {
        ExternalConfigSource::ClashSubscription { url: value.to_string() }
    } else {
        ExternalConfigSource::LocalFile { path: value.to_string() }
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
        } else if !label.ends_with('-') {
            label.push('-');
        }
    }
    let trimmed = label.trim_matches('-');
    if trimmed.is_empty() { "subscription".to_string() } else { trimmed.to_string() }
}

fn emit_startup_logs(startup: &StartupRuntime, options: &StartupOptions) {
    let _ = emit_log_event_by_name(
        "startup.begin",
        &[
            ("phase", startup.plan.current_phase.to_string()),
            ("config_source", describe_source(&options.source)),
        ],
    );

    if let Some(error) = &startup.activation.translation_error {
        let _ = emit_log_event_by_name(
            "subscription.translate_failed",
            &[("source", describe_source(&options.source)), ("reason", error.to_string())],
        );
    }

    if startup.activation.source == crate::subscription::ConfigActivationSource::LastKnownGoodCache
    {
        let _ = emit_log_event_by_name(
            "provider.cache_rollback_used",
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

fn emit_startup_activated(
    startup: &StartupRuntime,
    options: &StartupOptions,
    admin_handle: Option<&AdminTaskHandle>,
    handles: &[ListenerTaskHandle],
) {
    let _ = emit_log_event_by_name(
        "startup.activated",
        &[
            ("source", describe_source(&options.source)),
            ("activated", describe_activation_source(startup.activation.source).to_string()),
            (
                "cache_rollback",
                if startup.activation.source == ConfigActivationSource::LastKnownGoodCache {
                    "used".to_string()
                } else {
                    "not-used".to_string()
                },
            ),
            ("listeners", handles.len().to_string()),
            ("admin_enabled", admin_handle.is_some().to_string()),
            (
                "admin_bind",
                admin_handle
                    .map(|admin| admin.local_addr().to_string())
                    .unwrap_or_else(|| "disabled".to_string()),
            ),
        ],
    );
}

fn emit_admin_bound(operations: &OperationsPlan, admin_handle: Option<&AdminTaskHandle>) {
    let Some(admin) = admin_handle else {
        return;
    };
    let _ = emit_log_event_by_name(
        "admin.bound",
        &[
            ("bind", admin.local_addr().to_string()),
            ("healthz", operations.health.liveness.path.to_string()),
            ("readyz", operations.health.readiness.path.to_string()),
            ("metrics", operations.metrics.exposition_path.to_string()),
        ],
    );
}

fn emit_fatal_startup_failure(phase: &str, source: &ExternalConfigSource, error: &Error) {
    let _ = emit_log_event_by_name(
        "startup.failed",
        &[
            ("phase", phase.to_string()),
            ("source", describe_source(source)),
            ("reason", error.to_string()),
        ],
    );
}

fn emit_runtime_failure(phase: &str, source: &ExternalConfigSource, error: &Error) {
    let _ = emit_log_event_by_name(
        "runtime.failed",
        &[
            ("phase", phase.to_string()),
            ("source", describe_source(source)),
            ("reason", error.to_string()),
        ],
    );
}

fn emit_listener_bound(handle: &ListenerTaskHandle) {
    let _ = emit_log_event_by_name(
        "listener.bound",
        &[
            ("listener", handle.plan().name.clone()),
            ("protocol", format!("{:?}", handle.plan().protocol)),
            ("bind", handle.local_addr().to_string()),
            ("target", describe_listener_target(handle.plan())),
        ],
    );
}

fn emit_runtime_readiness_changed(status: ProbeStatus, reason: String) {
    let _ = emit_log_event_by_name(
        "runtime.readiness_changed",
        &[("status", status.as_str().to_string()), ("reason", reason)],
    );
}

fn update_runtime_readiness(
    runtime: &RuntimeState,
    _operations: &OperationsPlan,
    status: ProbeStatus,
    reason: String,
) {
    runtime.update_readiness(status, reason.clone());
    emit_runtime_readiness_changed(status, reason);
}

async fn wait_for_runtime_stop<F>(
    source: &ExternalConfigSource,
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
                        ProbeStatus::Degraded,
                        format!("listener '{}' failed: {failure}", listener_name),
                    );
                    emit_runtime_failure("listener_task", source, &failure);
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

async fn shutdown_admin_task(handle: &mut Option<AdminTaskHandle>) -> Result<(), Error> {
    let Some(handle) = handle.take() else {
        return Ok(());
    };

    let local_addr = handle.local_addr();
    handle.abort();
    match handle.join().await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error),
        Err(join_error) if join_error.is_cancelled() => Ok(()),
        Err(join_error) => Err(Error::io(format!(
            "admin listener on '{}' panicked during shutdown: {join_error}",
            local_addr
        ))),
    }
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

fn describe_activation_source(source: ConfigActivationSource) -> &'static str {
    match source {
        ConfigActivationSource::LocalFile => "local_file",
        ConfigActivationSource::FreshTranslation => "fresh_translation",
        ConfigActivationSource::LastKnownGoodCache => "last_known_good_cache",
    }
}

fn describe_listener_target(plan: &crate::listener::ListenerPlan) -> String {
    format!(
        "{} -> node:{} ({}, {})",
        describe_target_ref(&plan.target),
        plan.resolved_target.name,
        describe_node_kind(plan.resolved_target.kind),
        describe_origin(&plan.resolved_target.origin)
    )
}

fn describe_target_ref(target: &TargetRef) -> String {
    match target {
        TargetRef::Node(name) => format!("node:{name}"),
        TargetRef::Group(name) => format!("group:{name}"),
    }
}

fn describe_node_kind(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::DirectTcp => "DirectTcp",
        NodeKind::Trojan => "Trojan",
    }
}

fn describe_origin(origin: &ConfigOrigin) -> String {
    match origin {
        ConfigOrigin::Inline => "inline".to_string(),
        ConfigOrigin::Provider { provider, subscription } => {
            format!("provider:{provider}@{subscription}")
        }
        ConfigOrigin::Subscription { subscription } => {
            format!("subscription:{subscription}")
        }
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
                kind: crate::config::external::NodeKindInput::DirectTcp,
                address: None,
                server: None,
                port: None,
                password: None,
                sni: None,
                skip_cert_verify: false,
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
        assert_eq!(activation.active_config.listeners()[0].bind, "127.0.0.1:1080");
        assert_eq!(activation.translation_error, None);

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn startup_uses_fresh_clash_translation_when_document_is_valid() {
        let cache_path = temp_path("fresh-cache");
        let cache = CacheStore::new(cache_path.clone());

        let activation =
            load_startup_config(StartupConfigInput::Document(valid_clash_document()), Some(&cache))
                .await
                .expect("valid Clash document should activate");

        assert_eq!(activation.source, ConfigActivationSource::FreshTranslation);
        assert_eq!(activation.active_config.listeners().len(), 2);
        assert_eq!(activation.active_config.nodes().len(), 1);
        assert_eq!(activation.active_config.groups().len(), 2);
        assert_eq!(activation.active_config.subscriptions().len(), 1);
        assert_eq!(activation.translation_error, None);
        assert_eq!(activation.active_config.listeners()[0].name, "local-socks");
        assert_eq!(activation.active_config.listeners()[1].name, "local-connect");
        assert_eq!(
            activation
                .active_config
                .resolve_target_node(&activation.active_config.listeners()[0].target)
                .expect("listener target should resolve")
                .name,
            "trojan-a"
        );
        assert!(cache.path().exists());

        let _ = fs::remove_file(cache_path);
    }

    #[tokio::test]
    async fn startup_falls_back_to_last_known_good_cache_when_translation_fails() {
        let cache_path = temp_path("rollback-cache");
        let cache = CacheStore::new(cache_path.clone());

        let fresh =
            load_startup_config(StartupConfigInput::Document(valid_clash_document()), Some(&cache))
                .await
                .expect("seed translation should persist cache");
        assert_eq!(fresh.source, ConfigActivationSource::FreshTranslation);

        let activation = load_startup_config(
            StartupConfigInput::Document(invalid_clash_document()),
            Some(&cache),
        )
        .await
        .expect("startup should fall back to cache");

        assert_eq!(activation.source, ConfigActivationSource::LastKnownGoodCache);
        assert_eq!(activation.active_config.listeners().len(), 2);
        assert_eq!(activation.active_config.nodes().len(), 1);
        assert_eq!(activation.active_config.subscriptions().len(), 1);
        assert_eq!(activation.active_config.groups().len(), 2);
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
  - name: trojan-a
    type: trojan
    server: 127.0.0.1
    port: 443
    password: secret
proxy-groups:
  - name: proxy
    type: select
    proxies:
      - trojan-a
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
                kind: crate::config::external::NodeKindInput::DirectTcp,
                address: None,
                server: None,
                port: None,
                password: None,
                sni: None,
                skip_cert_verify: false,
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        };
        fs::write(&path, serde_yaml::to_string(&config).expect("config should serialize"))
            .expect("config file should be written");

        let startup = prepare_runtime(&StartupOptions {
            source: ExternalConfigSource::LocalFile { path: path.display().to_string() },
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
            source: ExternalConfigSource::LocalFile { path: path.display().to_string() },
            cache_path: None,
        })
        .await
        .expect("repository example config should prepare a runtime");

        assert_eq!(startup.activation.source, ConfigActivationSource::LocalFile);
        assert_eq!(startup.runtime.listeners().listeners().len(), 2);
        assert_eq!(startup.runtime.active_config().listeners()[0].name, "local-socks");
        assert_eq!(startup.runtime.active_config().listeners()[1].name, "local-connect");
        assert_eq!(startup.runtime.active_config().nodes()[0].name, "default-upstream");
    }

    #[tokio::test]
    async fn startup_prepare_runtime_rejects_configs_without_listeners() {
        let path = temp_path("runtime-no-listeners");
        let config = ExternalConfig {
            nodes: vec![NodeInput {
                name: "node-a".to_string(),
                kind: crate::config::external::NodeKindInput::DirectTcp,
                address: None,
                server: None,
                port: None,
                password: None,
                sni: None,
                skip_cert_verify: false,
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        };
        fs::write(&path, serde_yaml::to_string(&config).expect("config should serialize"))
            .expect("config file should be written");

        let error = prepare_runtime(&StartupOptions {
            source: ExternalConfigSource::LocalFile { path: path.display().to_string() },
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
            ExternalConfigSource::LocalFile { path: "/tmp/minibox.yaml".to_string() }
        );
        assert_eq!(
            startup_source_from_arg("http://example.com/subscription"),
            ExternalConfigSource::ClashSubscription {
                url: "http://example.com/subscription".to_string()
            }
        );
        assert_eq!(
            startup_source_from_arg("https://example.com/subscription"),
            ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string()
            }
        );
    }

    #[test]
    fn startup_plan_defaults_to_repository_example_config() {
        assert_eq!(
            build_startup_plan().subscription.source,
            ExternalConfigSource::LocalFile { path: "config/example.yaml".to_string() }
        );
    }

    #[test]
    fn remote_sources_get_a_stable_default_cache_path() {
        let cache_path = default_cache_path(&ExternalConfigSource::ClashSubscription {
            url: "http://example.com/a?b=c".to_string(),
        })
        .expect("remote source should get a cache path");

        assert_eq!(cache_path, PathBuf::from(".minibox/cache/http-example-com-a-b-c.yaml"));
    }

    #[tokio::test]
    async fn runtime_wait_returns_shutdown_reason_when_signal_arrives() {
        let mut handles = vec![spawn_idle_listener_task("local-socks", 1080)];
        let source = ExternalConfigSource::LocalFile { path: "config/example.yaml".to_string() };
        let reason = wait_for_runtime_stop(&source, &mut handles, async { Ok(()) })
            .await
            .expect("shutdown signal should stop runtime");

        assert_eq!(reason, "shutdown signal received");
        shutdown_listener_tasks(&mut handles).await.expect("listener shutdown should succeed");
    }

    #[tokio::test]
    async fn runtime_wait_propagates_listener_failure() {
        let mut handles = vec![spawn_failed_listener_task("local-socks", 1080)];
        let source = ExternalConfigSource::LocalFile { path: "config/example.yaml".to_string() };
        let error = wait_for_runtime_stop(
            &source,
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
                kind: crate::config::external::NodeKindInput::DirectTcp,
                address: None,
                server: None,
                port: None,
                password: None,
                sni: None,
                skip_cert_verify: false,
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        };
        fs::write(&path, serde_yaml::to_string(&config).expect("config should serialize"))
            .expect("config file should be written");

        match run_until(
            StartupOptions {
                source: ExternalConfigSource::LocalFile { path: path.display().to_string() },
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
            resolved_target: crate::config::internal::NodeConfig {
                name: "node-a".to_string(),
                kind: crate::config::internal::NodeKind::DirectTcp,
                trojan: None,
                origin: crate::config::internal::ConfigOrigin::Inline,
            },
            handler: ListenerHandler::Socks5,
            admission: ListenerAdmissionPlan { shared_limit: 64 },
        }
    }
}
