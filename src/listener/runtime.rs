use crate::config::internal::ActiveConfig;
use crate::listener::{AdmissionControl, ListenerPlan, derive_listener_plans};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerStage {
    Planned,
    Prepared,
}

pub trait ListenerLifecycle {
    fn stage(&self) -> ListenerStage;
    fn plan(&self) -> &ListenerPlan;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedListener {
    plan: ListenerPlan,
    stage: ListenerStage,
}

impl PreparedListener {
    pub fn plan(&self) -> &ListenerPlan {
        &self.plan
    }
}

impl ListenerLifecycle for PreparedListener {
    fn stage(&self) -> ListenerStage {
        self.stage
    }

    fn plan(&self) -> &ListenerPlan {
        &self.plan
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListenerRegistry {
    listeners: Vec<PreparedListener>,
}

impl ListenerRegistry {
    pub fn from_active_config(active_config: &ActiveConfig, admission: &AdmissionControl) -> Self {
        let snapshot = admission.snapshot();
        let listeners = derive_listener_plans(active_config, snapshot)
            .into_iter()
            .map(prepare_listener)
            .collect();

        Self { listeners }
    }

    pub fn listeners(&self) -> &[PreparedListener] {
        &self.listeners
    }
}

pub fn prepare_listener(plan: ListenerPlan) -> PreparedListener {
    PreparedListener { plan, stage: ListenerStage::Prepared }
}

#[cfg(test)]
mod tests {
    use super::{ListenerLifecycle, ListenerRegistry, ListenerStage};
    use crate::config::external::{
        ExternalConfig, ListenerInput, ListenerProtocolInput, NodeInput, TargetRefInput,
    };
    use crate::config::internal::ActiveConfig;
    use crate::listener::AdmissionControl;

    #[test]
    fn builds_prepared_listeners_from_configs() {
        let active_config = ActiveConfig::from_external(ExternalConfig {
            listeners: vec![
                ListenerInput {
                    name: "local-socks".to_string(),
                    bind: "127.0.0.1:1080".to_string(),
                    protocol: ListenerProtocolInput::Socks5,
                    target: TargetRefInput::node("node-a"),
                },
                ListenerInput {
                    name: "local-connect".to_string(),
                    bind: "127.0.0.1:8080".to_string(),
                    protocol: ListenerProtocolInput::HttpConnect,
                    target: TargetRefInput::group("group-a"),
                },
            ],
            nodes: vec![
                NodeInput {
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
                },
                NodeInput {
                    name: "node-b".to_string(),
                    kind: crate::config::external::NodeKindInput::DirectTcp,
                    address: None,
                    server: None,
                    port: None,
                    password: None,
                    sni: None,
                    skip_cert_verify: false,
                    provider: None,
                    subscription: None,
                },
            ],
            groups: vec![crate::config::external::GroupInput {
                name: "group-a".to_string(),
                strategy: crate::config::external::GroupStrategyInput::Fallback,
                members: vec![TargetRefInput::node("node-b")],
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        })
        .expect("listener test config should normalize");
        let admission = AdmissionControl::new(64);

        let registry = ListenerRegistry::from_active_config(&active_config, &admission);

        assert_eq!(registry.listeners().len(), 2);
        assert_eq!(registry.listeners()[0].stage(), ListenerStage::Prepared);
        assert_eq!(registry.listeners()[0].plan().admission.shared_limit, 64);
        assert_eq!(registry.listeners()[1].plan().bind, "127.0.0.1:8080");
        assert_eq!(registry.listeners()[1].plan().resolved_target.name, "node-b");
    }
}
