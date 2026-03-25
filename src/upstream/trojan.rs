use std::fmt::{Display, Formatter};
use std::time::Duration;

use hex::encode as hex_encode;
use native_tls::TlsConnector as NativeTlsConnector;
use sha2::{Digest, Sha224};
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;
use tokio_native_tls::TlsConnector;

use super::connection::{UpstreamConnection, UpstreamStream};
use super::direct::{DialError, DirectDialPlan, dial_tcp};
use super::target::{DialTarget, DialTargetHost};

const DEFAULT_TLS_HANDSHAKE_TIMEOUT_SECS: u64 = 10;
const TROJAN_CONNECT_COMMAND: u8 = 0x01;
const TROJAN_ATYP_IPV4: u8 = 0x01;
const TROJAN_ATYP_DOMAIN: u8 = 0x03;
const TROJAN_ATYP_IPV6: u8 = 0x04;
const CRLF: &[u8; 2] = b"\r\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrojanDialPlan {
    pub tls_handshake_timeout: Duration,
}

impl Default for TrojanDialPlan {
    fn default() -> Self {
        Self { tls_handshake_timeout: Duration::from_secs(DEFAULT_TLS_HANDSHAKE_TIMEOUT_SECS) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrojanRouteConfig {
    pub password: String,
    pub tls_server_name: String,
    pub skip_cert_verify: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrojanHandshakeStage {
    TlsHandshake,
    WritePassword,
    WriteConnectRequest,
    Flush,
}

impl Display for TrojanHandshakeStage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TlsHandshake => write!(f, "tls_handshake"),
            Self::WritePassword => write!(f, "write_password"),
            Self::WriteConnectRequest => write!(f, "write_connect_request"),
            Self::Flush => write!(f, "flush"),
        }
    }
}

pub async fn dial_trojan(
    server_target: &DialTarget,
    destination_target: &DialTarget,
    route: &TrojanRouteConfig,
    direct_plan: DirectDialPlan,
    trojan_plan: TrojanDialPlan,
) -> Result<UpstreamConnection, DialError> {
    let (stream, bind_addr) = dial_tcp(server_target, direct_plan).await?;
    let tls_connector = build_tls_connector(route)?;
    let tls_connector = TlsConnector::from(tls_connector);

    let mut tls_stream = timeout(
        trojan_plan.tls_handshake_timeout,
        tls_connector.connect(route.tls_server_name.as_str(), stream),
    )
    .await
    .map_err(|_| DialError::TrojanHandshakeTimeout {
        server: server_target.clone(),
        timeout: trojan_plan.tls_handshake_timeout,
        stage: TrojanHandshakeStage::TlsHandshake,
    })?
    .map_err(|source| DialError::TrojanTls { server: server_target.clone(), source })?;

    let password_line = trojan_password_line(route.password.as_str());
    tls_stream.write_all(&password_line).await.map_err(|source| DialError::TrojanHandshake {
        server: server_target.clone(),
        stage: TrojanHandshakeStage::WritePassword,
        source,
    })?;
    tls_stream.write_all(&encode_trojan_connect_request(destination_target)).await.map_err(
        |source| DialError::TrojanHandshake {
            server: server_target.clone(),
            stage: TrojanHandshakeStage::WriteConnectRequest,
            source,
        },
    )?;
    tls_stream.flush().await.map_err(|source| DialError::TrojanHandshake {
        server: server_target.clone(),
        stage: TrojanHandshakeStage::Flush,
        source,
    })?;

    Ok(UpstreamConnection { stream: UpstreamStream::Trojan(tls_stream), bind_addr })
}

fn build_tls_connector(route: &TrojanRouteConfig) -> Result<NativeTlsConnector, DialError> {
    let mut builder = NativeTlsConnector::builder();
    if route.skip_cert_verify {
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
    }

    builder.build().map_err(|source| DialError::TrojanTlsConfig {
        server_name: route.tls_server_name.clone(),
        source,
    })
}

fn trojan_password_line(password: &str) -> Vec<u8> {
    let digest = Sha224::digest(password.as_bytes());
    let mut line = hex_encode(digest).into_bytes();
    line.extend_from_slice(CRLF);
    line
}

fn encode_trojan_connect_request(destination_target: &DialTarget) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + 1 + 256 + 2 + CRLF.len());
    payload.push(TROJAN_CONNECT_COMMAND);

    match &destination_target.host {
        DialTargetHost::Ip(address) => match address {
            std::net::IpAddr::V4(address) => {
                payload.push(TROJAN_ATYP_IPV4);
                payload.extend_from_slice(&address.octets());
            }
            std::net::IpAddr::V6(address) => {
                payload.push(TROJAN_ATYP_IPV6);
                payload.extend_from_slice(&address.octets());
            }
        },
        DialTargetHost::Domain(domain) => {
            payload.push(TROJAN_ATYP_DOMAIN);
            payload.push(
                u8::try_from(domain.len())
                    .expect("trojan destination domains should fit within one octet"),
            );
            payload.extend_from_slice(domain.as_bytes());
        }
    }

    payload.extend_from_slice(&destination_target.port.to_be_bytes());
    payload.extend_from_slice(CRLF);
    payload
}

#[cfg(test)]
mod tests {
    use super::{encode_trojan_connect_request, trojan_password_line};
    use crate::upstream::{DialTarget, DialTargetHost};

    #[test]
    fn hashes_password_into_trojan_auth_line() {
        assert_eq!(
            String::from_utf8(trojan_password_line("password")).expect("utf8"),
            "d63dc919e201d7bc4c825630d2cf25fdc93d4b2f0d46706d29038d01\r\n"
        );
    }

    #[test]
    fn encodes_domain_connect_request() {
        let request = encode_trojan_connect_request(&DialTarget {
            host: DialTargetHost::Domain("example.com".to_string()),
            port: 443,
        });

        assert_eq!(
            request,
            vec![
                0x01, 0x03, 0x0b, b'e', b'x', b'a', b'm', b'p', b'l', b'e', b'.', b'c', b'o', b'm',
                0x01, 0xbb, b'\r', b'\n',
            ]
        );
    }

    #[test]
    fn encodes_ipv4_connect_request() {
        let request = encode_trojan_connect_request(&DialTarget {
            host: DialTargetHost::Ip(std::net::IpAddr::V4(std::net::Ipv4Addr::new(1, 2, 3, 4))),
            port: 80,
        });

        assert_eq!(request, vec![0x01, 0x01, 1, 2, 3, 4, 0x00, 0x50, b'\r', b'\n']);
    }
}
