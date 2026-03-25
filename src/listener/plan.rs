use crate::config::internal::{ActiveConfig, NodeConfig, ProtocolKind, TargetRef};
use crate::listener::AdmissionSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerHandler {
    Socks5,
    HttpConnect,
}

impl From<ProtocolKind> for ListenerHandler {
    fn from(protocol: ProtocolKind) -> Self {
        match protocol {
            ProtocolKind::Socks5 => Self::Socks5,
            ProtocolKind::HttpConnect => Self::HttpConnect,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerAdmissionPlan {
    pub shared_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerPlan {
    pub name: String,
    pub bind: String,
    pub protocol: ProtocolKind,
    pub target: TargetRef,
    pub resolved_target: NodeConfig,
    pub handler: ListenerHandler,
    pub admission: ListenerAdmissionPlan,
}

pub fn derive_listener_plans(
    active_config: &ActiveConfig,
    admission: AdmissionSnapshot,
) -> Vec<ListenerPlan> {
    active_config
        .listeners()
        .iter()
        .map(|listener| ListenerPlan {
            name: listener.name.clone(),
            bind: listener.bind.clone(),
            protocol: listener.protocol,
            target: listener.target.clone(),
            resolved_target: active_config
                .resolve_target_node(&listener.target)
                .expect("active config should resolve listener target during plan derivation")
                .clone(),
            handler: ListenerHandler::from(listener.protocol),
            admission: ListenerAdmissionPlan { shared_limit: admission.max_connections },
        })
        .collect()
}
