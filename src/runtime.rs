use std::sync::{Arc, RwLock};

use crate::config::internal::ActiveConfig;
use crate::error::Error;
use crate::health::{ProbeKind, ProbeReport, ProbeSnapshot, ProbeStatus};
use crate::listener::{
    AdmissionControl, AdmissionSnapshot, ListenerRegistry, ListenerTaskHandle,
    spawn_registry_accept_loops,
};
use crate::metrics::RuntimeMetricsSnapshot;
use crate::session::SessionPlan;

#[derive(Debug, Clone)]
pub struct RuntimeState {
    active_config: ActiveConfig,
    admission: AdmissionControl,
    listeners: ListenerRegistry,
    session_plan: SessionPlan,
    readiness: Arc<RwLock<RuntimeReadiness>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeReadiness {
    status: ProbeStatus,
    detail: String,
}

impl RuntimeState {
    pub fn new(active_config: ActiveConfig) -> Self {
        let admission = AdmissionControl::new(active_config.limits().max_connections);
        let listeners = ListenerRegistry::from_active_config(&active_config, &admission);
        let session_plan = SessionPlan::from_limits(active_config.limits());
        let planned = ProbeSnapshot::planned(ProbeKind::Readiness);
        let readiness = Arc::new(RwLock::new(RuntimeReadiness {
            status: planned.status,
            detail: planned.detail.to_string(),
        }));

        Self { active_config, admission, listeners, session_plan, readiness }
    }

    pub fn active_config(&self) -> &ActiveConfig {
        &self.active_config
    }

    pub fn admission(&self) -> &AdmissionControl {
        &self.admission
    }

    pub fn admission_snapshot(&self) -> AdmissionSnapshot {
        self.admission.snapshot()
    }

    pub fn listeners(&self) -> &ListenerRegistry {
        &self.listeners
    }

    pub fn session_plan(&self) -> SessionPlan {
        self.session_plan
    }

    pub fn admin_bind(&self) -> Option<&str> {
        if self.active_config.admin().enabled {
            self.active_config.admin().bind.as_deref()
        } else {
            None
        }
    }

    pub fn admin_access_token(&self) -> Option<&str> {
        if self.active_config.admin().enabled {
            self.active_config.admin().access_token.as_deref()
        } else {
            None
        }
    }

    pub fn update_readiness(&self, status: ProbeStatus, detail: String) {
        let mut readiness =
            self.readiness.write().expect("runtime readiness lock should not poison");
        readiness.status = status;
        readiness.detail = detail;
    }

    pub fn readiness_report(&self) -> ProbeReport {
        let readiness = self.readiness.read().expect("runtime readiness lock should not poison");
        ProbeReport {
            kind: ProbeKind::Readiness,
            status: readiness.status,
            detail: readiness.detail.clone(),
        }
    }

    pub fn liveness_report(&self) -> ProbeReport {
        ProbeReport {
            kind: ProbeKind::Liveness,
            status: ProbeStatus::Ready,
            detail: ProbeSnapshot::planned(ProbeKind::Liveness).detail.to_string(),
        }
    }

    pub fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        RuntimeMetricsSnapshot {
            active_connections: self.admission.active_connections(),
            max_connections: self.admission.max_connections(),
            bound_listeners: self.listeners.listeners().len(),
            readiness: self.readiness_report().status,
        }
    }

    pub async fn spawn_accept_loops(&self) -> Result<Vec<ListenerTaskHandle>, Error> {
        spawn_registry_accept_loops(&self.listeners, &self.admission, self.session_plan).await
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeState;
    use crate::config::external::{
        AdminInput, ExternalConfig, LimitsInput, ListenerInput, ListenerProtocolInput, NodeInput,
        TargetRefInput,
    };
    use crate::config::internal::ActiveConfig;
    use crate::health::ProbeStatus;

    #[test]
    fn runtime_derives_listener_registry_and_shared_admission() {
        let active_config = ActiveConfig::from_external(ExternalConfig {
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
            limits: LimitsInput { max_connections: Some(128), relay_buffer_bytes: None },
            ..ExternalConfig::default()
        })
        .expect("runtime config should normalize");

        let runtime = RuntimeState::new(active_config);

        assert_eq!(runtime.listeners().listeners().len(), 1);
        assert_eq!(runtime.admission_snapshot().max_connections, 128);
        assert_eq!(runtime.listeners().listeners()[0].plan().admission.shared_limit, 128);
    }

    #[tokio::test]
    async fn runtime_spawns_accept_loops_for_configured_listeners() {
        let active_config = ActiveConfig::from_external(ExternalConfig {
            listeners: vec![
                ListenerInput {
                    name: "local-socks".to_string(),
                    bind: "127.0.0.1:0".to_string(),
                    protocol: ListenerProtocolInput::Socks5,
                    target: TargetRefInput::node("node-a"),
                },
                ListenerInput {
                    name: "local-connect".to_string(),
                    bind: "127.0.0.1:0".to_string(),
                    protocol: ListenerProtocolInput::HttpConnect,
                    target: TargetRefInput::node("node-a"),
                },
            ],
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
            limits: LimitsInput { max_connections: Some(64), relay_buffer_bytes: None },
            ..ExternalConfig::default()
        })
        .expect("runtime config should normalize");

        let runtime = RuntimeState::new(active_config);
        let handles = match runtime.spawn_accept_loops().await {
            Ok(handles) => handles,
            Err(crate::error::Error::Io(message))
                if message.contains("Operation not permitted") =>
            {
                return;
            }
            Err(error) => panic!("listeners should bind: {error}"),
        };

        assert_eq!(handles.len(), 2);
        assert!(handles.iter().all(|handle| handle.local_addr().port() != 0));

        for handle in handles {
            handle.abort();
            let join = handle.join().await.expect_err("task should be cancelled");
            assert!(join.is_cancelled());
        }
    }

    #[test]
    fn runtime_exposes_admin_and_readiness_snapshots() {
        let active_config = ActiveConfig::from_external(ExternalConfig {
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
            admin: AdminInput {
                enabled: true,
                bind: Some("127.0.0.1:0".to_string()),
                access_token: Some("secret".to_string()),
            },
            ..ExternalConfig::default()
        })
        .expect("runtime config should normalize");

        let runtime = RuntimeState::new(active_config);
        assert_eq!(runtime.admin_bind(), Some("127.0.0.1:0"));
        assert_eq!(runtime.admin_access_token(), Some("secret"));
        assert_eq!(runtime.readiness_report().status, ProbeStatus::Starting);

        runtime.update_readiness(ProbeStatus::Ready, "listeners bound".to_string());
        assert_eq!(runtime.readiness_report().status, ProbeStatus::Ready);
        assert_eq!(runtime.metrics_snapshot().bound_listeners, 1);
    }
}
