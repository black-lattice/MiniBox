use crate::config::internal::ActiveConfig;
use crate::error::Error;
use crate::listener::{
    AdmissionControl, AdmissionSnapshot, ListenerRegistry, ListenerTaskHandle,
    spawn_registry_accept_loops,
};
use crate::session::SessionPlan;

#[derive(Debug, Clone)]
pub struct RuntimeState {
    active_config: ActiveConfig,
    admission: AdmissionControl,
    listeners: ListenerRegistry,
    session_plan: SessionPlan,
}

impl RuntimeState {
    pub fn new(active_config: ActiveConfig) -> Self {
        let admission = AdmissionControl::new(active_config.limits().max_connections);
        let listeners = ListenerRegistry::from_configs(active_config.listeners(), &admission);
        let session_plan = SessionPlan::from_limits(active_config.limits());

        Self {
            active_config,
            admission,
            listeners,
            session_plan,
        }
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

    pub async fn spawn_accept_loops(&self) -> Result<Vec<ListenerTaskHandle>, Error> {
        spawn_registry_accept_loops(&self.listeners, &self.admission, self.session_plan).await
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeState;
    use crate::config::external::{
        ExternalConfig, LimitsInput, ListenerInput, ListenerProtocolInput, NodeInput,
        TargetRefInput,
    };
    use crate::config::internal::ActiveConfig;

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
                address: "1.1.1.1:443".to_string(),
                provider: None,
                subscription: None,
            }],
            limits: LimitsInput {
                max_connections: Some(128),
                relay_buffer_bytes: None,
            },
            ..ExternalConfig::default()
        })
        .expect("runtime config should normalize");

        let runtime = RuntimeState::new(active_config);

        assert_eq!(runtime.listeners().listeners().len(), 1);
        assert_eq!(runtime.admission_snapshot().max_connections, 128);
        assert_eq!(
            runtime.listeners().listeners()[0]
                .plan()
                .admission
                .shared_limit,
            128
        );
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
                address: "1.1.1.1:443".to_string(),
                provider: None,
                subscription: None,
            }],
            limits: LimitsInput {
                max_connections: Some(64),
                relay_buffer_bytes: None,
            },
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
}
