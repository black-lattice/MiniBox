use std::fmt::{Display, Formatter};
use std::net::SocketAddr;
use std::time::Duration;

use native_tls::Error as NativeTlsError;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::protocol::socks5::ReplyCode;
use crate::upstream::{DialTarget, DialTargetHost};

use super::trojan::TrojanHandshakeStage;

pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectDialPlan {
    pub connect_timeout: Duration,
}

impl Default for DirectDialPlan {
    fn default() -> Self {
        Self { connect_timeout: Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS) }
    }
}

#[derive(Debug)]
pub enum DialError {
    Timeout { target: DialTarget, timeout: Duration },
    Io { target: DialTarget, source: std::io::Error },
    TrojanTlsConfig { server_name: String, source: NativeTlsError },
    TrojanTls { server: DialTarget, source: NativeTlsError },
    TrojanHandshakeTimeout { server: DialTarget, timeout: Duration, stage: TrojanHandshakeStage },
    TrojanHandshake { server: DialTarget, stage: TrojanHandshakeStage, source: std::io::Error },
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
            Self::TrojanTlsConfig { .. }
            | Self::TrojanTls { .. }
            | Self::TrojanHandshakeTimeout { .. }
            | Self::TrojanHandshake { .. } => ReplyCode::GeneralFailure,
        }
    }
}

impl Display for DialError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { target, timeout } => {
                write!(f, "timed out dialing upstream target {target} after {timeout:?}")
            }
            Self::Io { target, source } => {
                write!(f, "failed dialing upstream target {target}: {source}")
            }
            Self::TrojanTlsConfig { server_name, source } => {
                write!(
                    f,
                    "failed building Trojan TLS connector for server name '{server_name}': {source}"
                )
            }
            Self::TrojanTls { server, source } => {
                write!(f, "Trojan TLS handshake failed for upstream server {server}: {source}")
            }
            Self::TrojanHandshakeTimeout { server, timeout, stage } => {
                write!(
                    f,
                    "Trojan handshake timed out for upstream server {server} during {stage} after {timeout:?}"
                )
            }
            Self::TrojanHandshake { server, stage, source } => {
                write!(
                    f,
                    "Trojan handshake failed for upstream server {server} during {stage}: {source}"
                )
            }
        }
    }
}

impl std::error::Error for DialError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Timeout { .. } => None,
            Self::Io { source, .. } => Some(source),
            Self::TrojanTlsConfig { source, .. } => Some(source),
            Self::TrojanTls { source, .. } => Some(source),
            Self::TrojanHandshakeTimeout { .. } => None,
            Self::TrojanHandshake { source, .. } => Some(source),
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
            timeout(plan.connect_timeout, TcpStream::connect((domain.as_str(), target.port))).await
        }
    };

    let stream = stream
        .map_err(|_| DialError::Timeout { target: target.clone(), timeout: plan.connect_timeout })?
        .map_err(|source| DialError::Io { target: target.clone(), source })?;

    let bind_addr =
        stream.local_addr().map_err(|source| DialError::Io { target: target.clone(), source })?;

    Ok((stream, bind_addr))
}
