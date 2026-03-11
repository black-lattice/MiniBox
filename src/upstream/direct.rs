use std::fmt::{Display, Formatter};
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::protocol::socks5::ReplyCode;
use crate::upstream::{DialTarget, DialTargetHost};

pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectDialPlan {
    pub connect_timeout: Duration,
}

impl Default for DirectDialPlan {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
        }
    }
}

#[derive(Debug)]
pub enum DialError {
    Timeout {
        target: DialTarget,
        timeout: Duration,
    },
    Io {
        target: DialTarget,
        source: std::io::Error,
    },
}

impl DialError {
    pub fn reply_code(&self) -> ReplyCode {
        match self {
            Self::Timeout { .. } => ReplyCode::HostUnreachable,
            Self::Io { source, .. } => match source.kind() {
                std::io::ErrorKind::ConnectionRefused => ReplyCode::ConnectionRefused,
                std::io::ErrorKind::AddrNotAvailable
                | std::io::ErrorKind::NotFound
                | std::io::ErrorKind::HostUnreachable => ReplyCode::HostUnreachable,
                std::io::ErrorKind::NetworkUnreachable => ReplyCode::NetworkUnreachable,
                _ => ReplyCode::GeneralFailure,
            },
        }
    }
}

impl Display for DialError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { target, timeout } => {
                write!(
                    f,
                    "timed out dialing upstream target {target} after {timeout:?}"
                )
            }
            Self::Io { target, source } => {
                write!(f, "failed dialing upstream target {target}: {source}")
            }
        }
    }
}

impl std::error::Error for DialError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Timeout { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

pub async fn dial_tcp(
    target: &DialTarget,
    plan: DirectDialPlan,
) -> Result<(TcpStream, SocketAddr), DialError> {
    let stream = match &target.host {
        DialTargetHost::Ip(address) => {
            timeout(
                plan.connect_timeout,
                TcpStream::connect(SocketAddr::new(*address, target.port)),
            )
            .await
        }
        DialTargetHost::Domain(domain) => {
            timeout(
                plan.connect_timeout,
                TcpStream::connect((domain.as_str(), target.port)),
            )
            .await
        }
    };

    let stream = stream
        .map_err(|_| DialError::Timeout {
            target: target.clone(),
            timeout: plan.connect_timeout,
        })?
        .map_err(|source| DialError::Io {
            target: target.clone(),
            source,
        })?;

    let bind_addr = stream.local_addr().map_err(|source| DialError::Io {
        target: target.clone(),
        source,
    })?;

    Ok((stream, bind_addr))
}
