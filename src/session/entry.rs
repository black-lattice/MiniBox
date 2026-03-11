use tokio::io::{AsyncRead, AsyncWrite};

use crate::session::http_connect;
use crate::session::socks5;
use crate::session::{SessionContext, SessionError, SessionPlan, SessionProtocol, SessionRequest};

pub async fn accept_downstream<S>(
    stream: &mut S,
    context: &SessionContext,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match context.protocol {
        SessionProtocol::Socks5 => socks5::accept_downstream(stream, context).await,
        SessionProtocol::HttpConnect => http_connect::accept_downstream(stream, context).await,
    }
}

pub async fn reject_deferred_connect<S>(
    stream: &mut S,
    request: &SessionRequest,
) -> Result<(), SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match request.context.protocol {
        SessionProtocol::Socks5 => socks5::reject_deferred_connect(stream).await,
        SessionProtocol::HttpConnect => http_connect::reject_deferred_connect(stream).await,
    }
}

pub async fn drive_session<S>(
    stream: &mut S,
    context: SessionContext,
    plan: SessionPlan,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match context.protocol {
        SessionProtocol::Socks5 => socks5::drive_session(stream, context, plan).await,
        SessionProtocol::HttpConnect => http_connect::drive_session(stream, context, plan).await,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::accept_downstream;
    use crate::config::internal::TargetRef;
    use crate::session::{SessionContext, SessionProtocol, TargetAddr};

    #[tokio::test]
    async fn accept_downstream_extracts_socks5_target() {
        let (mut client, mut server) = tokio::io::duplex(128);
        let context = SessionContext {
            listener_name: "local-socks".to_string(),
            protocol: SessionProtocol::Socks5,
            listener_target: TargetRef::Node("node-a".to_string()),
            downstream_peer: SocketAddr::from((Ipv4Addr::LOCALHOST, 40000)),
            downstream_local: SocketAddr::from((Ipv4Addr::LOCALHOST, 1080)),
        };

        let server_task =
            tokio::spawn(async move { accept_downstream(&mut server, &context).await });

        client
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .expect("write greeting");

        let mut selection = [0u8; 2];
        client
            .read_exact(&mut selection)
            .await
            .expect("read no-auth selection");
        assert_eq!(selection, [0x05, 0x00]);

        client
            .write_all(&[
                0x05, 0x01, 0x00, 0x03, 0x0b, b'e', b'x', b'a', b'm', b'p', b'l', b'e', b'.', b'c',
                b'o', b'm', 0x01, 0xbb,
            ])
            .await
            .expect("write connect request");

        let request = server_task
            .await
            .expect("server task should join")
            .expect("session should parse downstream request");

        assert_eq!(request.context.listener_name, "local-socks");
        assert_eq!(
            request.context.listener_target,
            TargetRef::Node("node-a".to_string())
        );
        assert_eq!(
            request.requested_target.address,
            TargetAddr::Domain("example.com".to_string())
        );
        assert_eq!(request.requested_target.port, 443);
    }

    #[tokio::test]
    async fn accept_downstream_extracts_http_connect_target() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let context = SessionContext {
            listener_name: "local-connect".to_string(),
            protocol: SessionProtocol::HttpConnect,
            listener_target: TargetRef::Node("node-a".to_string()),
            downstream_peer: SocketAddr::from((Ipv4Addr::LOCALHOST, 40001)),
            downstream_local: SocketAddr::from((Ipv4Addr::LOCALHOST, 8080)),
        };

        let server_task =
            tokio::spawn(async move { accept_downstream(&mut server, &context).await });

        client
            .write_all(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
            .await
            .expect("write connect request");

        let request = server_task
            .await
            .expect("server task should join")
            .expect("session should parse downstream request");

        assert_eq!(request.context.listener_name, "local-connect");
        assert_eq!(
            request.requested_target.address,
            TargetAddr::Domain("example.com".to_string())
        );
        assert_eq!(request.requested_target.port, 443);
    }
}
