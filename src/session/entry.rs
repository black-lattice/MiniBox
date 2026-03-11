use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::socks5::{ReplyCode, Socks5Handler};
use crate::session::http_connect;
use crate::session::{SessionContext, SessionError, SessionProtocol, SessionRequest};

pub async fn accept_downstream<S>(
    stream: &mut S,
    context: &SessionContext,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match context.protocol {
        SessionProtocol::Socks5 => accept_socks5(stream, context).await,
        SessionProtocol::HttpConnect => Err(http_connect::placeholder_error(context)),
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
        SessionProtocol::Socks5 => Socks5Handler
            .send_reply(stream, ReplyCode::GeneralFailure)
            .await
            .map_err(SessionError::from),
        SessionProtocol::HttpConnect => Err(http_connect::placeholder_error(&request.context)),
    }
}

pub async fn drive_placeholder_connection<S>(
    stream: &mut S,
    context: SessionContext,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = accept_downstream(stream, &context).await?;
    reject_deferred_connect(stream, &request).await?;
    Ok(request)
}

async fn accept_socks5<S>(
    stream: &mut S,
    context: &SessionContext,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = Socks5Handler.accept_connect(stream).await?;

    Ok(SessionRequest {
        context: context.clone(),
        requested_target: request.destination,
    })
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::drive_placeholder_connection;
    use crate::config::internal::TargetRef;
    use crate::protocol::socks5::TargetAddr;
    use crate::session::{SessionContext, SessionProtocol};

    #[tokio::test]
    async fn placeholder_drive_extracts_target_and_emits_general_failure() {
        let (mut client, mut server) = tokio::io::duplex(128);
        let context = SessionContext {
            listener_name: "local-socks".to_string(),
            protocol: SessionProtocol::Socks5,
            listener_target: TargetRef::Node("node-a".to_string()),
            downstream_peer: SocketAddr::from((Ipv4Addr::LOCALHOST, 40000)),
            downstream_local: SocketAddr::from((Ipv4Addr::LOCALHOST, 1080)),
        };

        let server_task =
            tokio::spawn(async move { drive_placeholder_connection(&mut server, context).await });

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

        let mut response = [0u8; 10];
        client
            .read_exact(&mut response)
            .await
            .expect("read deferred failure response");
        assert_eq!(response, [0x05, 0x01, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);

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
}
