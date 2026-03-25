use std::fmt::{Display, Formatter};
use std::net::IpAddr;

use crate::config::internal::{NodeKind, TrojanNodeConfig};
use crate::session::{SessionRequest, TargetAddr};
use crate::upstream::{DialTarget, DialTargetHost};

use super::trojan::TrojanRouteConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    InvalidPort(u16),
    UnsupportedNodeKind { node: String, kind: NodeKind },
}

impl Display for ResolveError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPort(port) => {
                write!(f, "cannot resolve upstream dial target with port {port}")
            }
            Self::UnsupportedNodeKind { node, kind } => {
                write!(
                    f,
                    "listener target node '{}' uses unsupported outbound kind '{:?}' in the current runtime",
                    node, kind
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectRouteKind {
    DirectTcp,
    Trojan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectRoute {
    pub kind: ConnectRouteKind,
    pub selected_node_name: String,
    pub connect_target: DialTarget,
    pub destination_target: DialTarget,
    pub trojan: Option<TrojanRouteConfig>,
}

pub fn resolve_connect_target(request: &SessionRequest) -> Result<DialTarget, ResolveError> {
    Ok(resolve_connect_route(request)?.connect_target)
}

pub fn resolve_connect_route(request: &SessionRequest) -> Result<ConnectRoute, ResolveError> {
    let destination_target = resolve_destination_target(request)?;

    match request.context.listener_target_node.kind {
        NodeKind::DirectTcp => Ok(ConnectRoute {
            kind: ConnectRouteKind::DirectTcp,
            selected_node_name: request.context.listener_target_node.name.clone(),
            connect_target: destination_target.clone(),
            destination_target,
            trojan: None,
        }),
        NodeKind::Trojan => {
            let trojan = request.context.listener_target_node.trojan.as_ref().ok_or_else(|| {
                ResolveError::UnsupportedNodeKind {
                    node: request.context.listener_target_node.name.clone(),
                    kind: NodeKind::Trojan,
                }
            })?;

            Ok(ConnectRoute {
                kind: ConnectRouteKind::Trojan,
                selected_node_name: request.context.listener_target_node.name.clone(),
                connect_target: DialTarget {
                    host: DialTargetHost::Domain(trojan.server.clone()),
                    port: trojan.port,
                },
                destination_target,
                trojan: Some(resolve_trojan_route_config(trojan)),
            })
        }
    }
}

fn resolve_destination_target(request: &SessionRequest) -> Result<DialTarget, ResolveError> {
    if request.requested_target.port == 0 {
        return Err(ResolveError::InvalidPort(0));
    }

    let host = match &request.requested_target.address {
        TargetAddr::Ipv4(address) => DialTargetHost::Ip(IpAddr::V4(*address)),
        TargetAddr::Domain(domain) => DialTargetHost::Domain(domain.clone()),
        TargetAddr::Ipv6(address) => DialTargetHost::Ip(IpAddr::V6(*address)),
    };

    Ok(DialTarget { host, port: request.requested_target.port })
}

fn resolve_trojan_route_config(trojan: &TrojanNodeConfig) -> TrojanRouteConfig {
    TrojanRouteConfig {
        password: trojan.password.clone(),
        tls_server_name: trojan.sni.clone().unwrap_or_else(|| trojan.server.clone()),
        skip_cert_verify: trojan.skip_cert_verify,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use super::{
        ConnectRoute, ConnectRouteKind, ResolveError, resolve_connect_route, resolve_connect_target,
    };
    use crate::config::internal::{
        ConfigOrigin, NodeConfig, NodeKind, TargetRef, TrojanNodeConfig,
    };
    use crate::session::{
        SessionContext, SessionProtocol, SessionRequest, TargetAddr, TargetEndpoint,
    };
    use crate::upstream::{DialTarget, DialTargetHost, TrojanRouteConfig};

    #[test]
    fn resolves_domain_connect_target_directly() {
        let request = SessionRequest {
            context: SessionContext {
                listener_name: "local-socks".to_string(),
                protocol: SessionProtocol::Socks5,
                listener_target: TargetRef::Node("node-a".to_string()),
                listener_target_node: test_listener_node(),
                downstream_peer: SocketAddr::from((Ipv4Addr::LOCALHOST, 30000)),
                downstream_local: SocketAddr::from((Ipv4Addr::LOCALHOST, 1080)),
            },
            requested_target: TargetEndpoint {
                address: TargetAddr::Domain("example.com".to_string()),
                port: 443,
            },
        };

        assert_eq!(
            resolve_connect_target(&request).expect("target should resolve"),
            DialTarget { host: DialTargetHost::Domain("example.com".to_string()), port: 443 }
        );
    }

    #[test]
    fn resolves_connect_route_from_listener_target_node_context() {
        let request = SessionRequest {
            context: SessionContext {
                listener_name: "local-socks".to_string(),
                protocol: SessionProtocol::Socks5,
                listener_target: TargetRef::Group("entry".to_string()),
                listener_target_node: test_listener_node(),
                downstream_peer: SocketAddr::from((Ipv4Addr::LOCALHOST, 30000)),
                downstream_local: SocketAddr::from((Ipv4Addr::LOCALHOST, 1080)),
            },
            requested_target: TargetEndpoint {
                address: TargetAddr::Domain("example.com".to_string()),
                port: 443,
            },
        };

        assert_eq!(
            resolve_connect_route(&request).expect("route should resolve"),
            ConnectRoute {
                kind: ConnectRouteKind::DirectTcp,
                selected_node_name: "node-a".to_string(),
                connect_target: DialTarget {
                    host: DialTargetHost::Domain("example.com".to_string()),
                    port: 443,
                },
                destination_target: DialTarget {
                    host: DialTargetHost::Domain("example.com".to_string()),
                    port: 443,
                },
                trojan: None,
            }
        );
    }

    #[test]
    fn resolves_trojan_route_from_listener_target_node_context() {
        let request = SessionRequest {
            context: SessionContext {
                listener_name: "local-socks".to_string(),
                protocol: SessionProtocol::Socks5,
                listener_target: TargetRef::Group("entry".to_string()),
                listener_target_node: NodeConfig {
                    name: "node-trojan".to_string(),
                    kind: NodeKind::Trojan,
                    trojan: Some(TrojanNodeConfig {
                        server: "trojan.example.com".to_string(),
                        port: 443,
                        password: "secret".to_string(),
                        sni: Some("cdn.example.com".to_string()),
                        skip_cert_verify: true,
                    }),
                    origin: ConfigOrigin::Inline,
                },
                downstream_peer: SocketAddr::from((Ipv4Addr::LOCALHOST, 30000)),
                downstream_local: SocketAddr::from((Ipv4Addr::LOCALHOST, 1080)),
            },
            requested_target: TargetEndpoint {
                address: TargetAddr::Domain("example.com".to_string()),
                port: 443,
            },
        };

        let route = resolve_connect_route(&request).expect("route should resolve");
        assert_eq!(route.kind, ConnectRouteKind::Trojan);
        assert_eq!(route.selected_node_name, "node-trojan");
        assert_eq!(
            route.connect_target,
            DialTarget {
                host: DialTargetHost::Domain("trojan.example.com".to_string()),
                port: 443,
            }
        );
        assert_eq!(
            route.destination_target,
            DialTarget { host: DialTargetHost::Domain("example.com".to_string()), port: 443 }
        );
        assert_eq!(
            route.trojan.expect("trojan route config"),
            TrojanRouteConfig {
                password: "secret".to_string(),
                tls_server_name: "cdn.example.com".to_string(),
                skip_cert_verify: true,
            }
        );
    }

    #[test]
    fn rejects_zero_port_targets() {
        let request = SessionRequest {
            context: SessionContext {
                listener_name: "local-socks".to_string(),
                protocol: SessionProtocol::Socks5,
                listener_target: TargetRef::Node("node-a".to_string()),
                listener_target_node: test_listener_node(),
                downstream_peer: SocketAddr::from((Ipv4Addr::LOCALHOST, 30000)),
                downstream_local: SocketAddr::from((Ipv4Addr::LOCALHOST, 1080)),
            },
            requested_target: TargetEndpoint {
                address: TargetAddr::Ipv4(Ipv4Addr::LOCALHOST),
                port: 0,
            },
        };

        assert_eq!(
            resolve_connect_target(&request).expect_err("port 0 should fail"),
            ResolveError::InvalidPort(0)
        );
    }

    fn test_listener_node() -> NodeConfig {
        NodeConfig {
            name: "node-a".to_string(),
            kind: NodeKind::DirectTcp,
            trojan: None,
            origin: ConfigOrigin::Inline,
        }
    }
}
