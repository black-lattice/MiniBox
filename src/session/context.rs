use std::net::SocketAddr;

use crate::config::internal::TargetRef;
use crate::listener::{ListenerHandler, ListenerPlan};
use crate::session::TargetEndpoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionProtocol {
    Socks5,
    HttpConnect,
}

impl From<ListenerHandler> for SessionProtocol {
    fn from(handler: ListenerHandler) -> Self {
        match handler {
            ListenerHandler::Socks5 => Self::Socks5,
            ListenerHandler::HttpConnect => Self::HttpConnect,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    pub listener_name: String,
    pub protocol: SessionProtocol,
    pub listener_target: TargetRef,
    pub downstream_peer: SocketAddr,
    pub downstream_local: SocketAddr,
}

impl SessionContext {
    pub fn from_listener_plan(
        plan: &ListenerPlan,
        downstream_peer: SocketAddr,
        downstream_local: SocketAddr,
    ) -> Self {
        Self {
            listener_name: plan.name.clone(),
            protocol: SessionProtocol::from(plan.handler),
            listener_target: plan.target.clone(),
            downstream_peer,
            downstream_local,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRequest {
    pub context: SessionContext,
    pub requested_target: TargetEndpoint,
}
