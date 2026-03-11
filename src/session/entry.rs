use std::net::SocketAddr;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::socks5::{ReplyCode, Socks5Handler};
use crate::relay::relay_bidirectional;
use crate::session::http_connect;
use crate::session::{SessionContext, SessionError, SessionPlan, SessionProtocol, SessionRequest};
use crate::upstream::resolve_connect_target;
use crate::upstream::{DialError, dial_tcp};

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

pub async fn drive_session<S>(
    stream: &mut S,
    context: SessionContext,
    plan: SessionPlan,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match context.protocol {
        SessionProtocol::Socks5 => drive_socks5_session(stream, context, plan).await,
        SessionProtocol::HttpConnect => Err(http_connect::placeholder_error(&context)),
    }
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

async fn drive_socks5_session<S>(
    stream: &mut S,
    context: SessionContext,
    plan: SessionPlan,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = accept_socks5(stream, &context).await?;
    let dial_target = match resolve_connect_target(&request) {
        Ok(target) => target,
        Err(error) => {
            Socks5Handler
                .send_reply(stream, ReplyCode::GeneralFailure)
                .await?;
            return Err(error.into());
        }
    };

    let (mut upstream, bind_addr) = match dial_tcp(&dial_target, plan.direct_dial).await {
        Ok(result) => result,
        Err(error) => {
            send_dial_failure_reply(stream, &error).await?;
            return Err(error.into());
        }
    };

    Socks5Handler
        .send_success_reply(stream, socket_addr_to_endpoint(bind_addr))
        .await?;

    relay_bidirectional(stream, &mut upstream, plan.relay).await?;

    Ok(request)
}

async fn send_dial_failure_reply<S>(stream: &mut S, error: &DialError) -> Result<(), SessionError>
where
    S: AsyncWrite + Unpin,
{
    Socks5Handler
        .send_reply(stream, error.reply_code())
        .await
        .map_err(SessionError::from)
}

fn socket_addr_to_endpoint(address: SocketAddr) -> crate::protocol::socks5::TargetEndpoint {
    use crate::protocol::socks5::{TargetAddr, TargetEndpoint};

    let target_addr = match address.ip() {
        std::net::IpAddr::V4(ip) => TargetAddr::Ipv4(ip),
        std::net::IpAddr::V6(ip) => TargetAddr::Ipv6(ip),
    };

    TargetEndpoint {
        address: target_addr,
        port: address.port(),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::accept_downstream;
    use crate::config::internal::TargetRef;
    use crate::protocol::socks5::TargetAddr;
    use crate::session::{SessionContext, SessionProtocol};

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
}
