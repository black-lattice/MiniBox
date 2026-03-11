use crate::config::internal::ListenerConfig;
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
    pub fn from_configs(listeners: &[ListenerConfig], admission: &AdmissionControl) -> Self {
        let snapshot = admission.snapshot();
        let listeners = derive_listener_plans(listeners, snapshot)
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
    PreparedListener {
        plan,
        stage: ListenerStage::Prepared,
    }
}

#[cfg(test)]
mod tests {
    use super::{ListenerLifecycle, ListenerRegistry, ListenerStage};
    use crate::config::internal::{ListenerConfig, ProtocolKind, TargetRef};
    use crate::listener::AdmissionControl;

    #[test]
    fn builds_prepared_listeners_from_configs() {
        let listeners = vec![
            ListenerConfig {
                name: "local-socks".to_string(),
                bind: "127.0.0.1:1080".to_string(),
                protocol: ProtocolKind::Socks5,
                target: TargetRef::Node("node-a".to_string()),
            },
            ListenerConfig {
                name: "local-connect".to_string(),
                bind: "127.0.0.1:8080".to_string(),
                protocol: ProtocolKind::HttpConnect,
                target: TargetRef::Group("group-a".to_string()),
            },
        ];
        let admission = AdmissionControl::new(64);

        let registry = ListenerRegistry::from_configs(&listeners, &admission);

        assert_eq!(registry.listeners().len(), 2);
        assert_eq!(registry.listeners()[0].stage(), ListenerStage::Prepared);
        assert_eq!(registry.listeners()[0].plan().admission.shared_limit, 64);
        assert_eq!(registry.listeners()[1].plan().bind, "127.0.0.1:8080");
    }
}
