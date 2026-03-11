use std::fmt::{Display, Formatter};
use std::net::IpAddr;

use crate::session::{SessionRequest, TargetAddr};
use crate::upstream::{DialTarget, DialTargetHost};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    InvalidPort(u16),
}

impl Display for ResolveError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPort(port) => {
                write!(f, "cannot resolve upstream dial target with port {port}")
            }
        }
    }
}

impl std::error::Error for ResolveError {}

pub fn resolve_connect_target(request: &SessionRequest) -> Result<DialTarget, ResolveError> {
    if request.requested_target.port == 0 {
        return Err(ResolveError::InvalidPort(0));
    }

    // Stage 1 is direct-only: route selection from listener targets stays outside this resolver.
    let host = match &request.requested_target.address {
        TargetAddr::Ipv4(address) => DialTargetHost::Ip(IpAddr::V4(*address)),
        TargetAddr::Domain(domain) => DialTargetHost::Domain(domain.clone()),
        TargetAddr::Ipv6(address) => DialTargetHost::Ip(IpAddr::V6(*address)),
    };

    Ok(DialTarget {
        host,
        port: request.requested_target.port,
    })
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use super::{ResolveError, resolve_connect_target};
    use crate::config::internal::TargetRef;
    use crate::session::{
        SessionContext, SessionProtocol, SessionRequest, TargetAddr, TargetEndpoint,
    };
    use crate::upstream::{DialTarget, DialTargetHost};

    #[test]
    fn resolves_domain_connect_target_directly() {
        let request = SessionRequest {
            context: SessionContext {
                listener_name: "local-socks".to_string(),
                protocol: SessionProtocol::Socks5,
                listener_target: TargetRef::Node("node-a".to_string()),
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
            DialTarget {
                host: DialTargetHost::Domain("example.com".to_string()),
                port: 443,
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
}
