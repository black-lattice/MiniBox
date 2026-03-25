use std::net::SocketAddr;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::protocol::socks5::{ReplyCode, Socks5Handler};
use crate::relay::relay_bidirectional;
use crate::session::{SessionContext, SessionError, SessionPlan, SessionRequest};
use crate::upstream::{DialError, connect_upstream, resolve_connect_route};

pub async fn accept_downstream<S>(
    stream: &mut S,
    context: &SessionContext,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = Socks5Handler.accept_connect(stream).await?;

    Ok(SessionRequest { context: context.clone(), requested_target: request.destination })
}

pub async fn reject_deferred_connect<S>(stream: &mut S) -> Result<(), SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    Socks5Handler.send_reply(stream, ReplyCode::GeneralFailure).await.map_err(SessionError::from)
}

pub async fn drive_session<S>(
    stream: &mut S,
    context: SessionContext,
    plan: SessionPlan,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = accept_downstream(stream, &context).await?;
    let route = match resolve_connect_route(&request) {
        Ok(route) => route,
        Err(error) => {
            Socks5Handler.send_reply(stream, ReplyCode::GeneralFailure).await?;
            return Err(error.into());
        }
    };

    let mut upstream = match connect_upstream(&route, plan.direct_dial, plan.trojan_dial).await {
        Ok(result) => result,
        Err(error) => {
            send_dial_failure_reply(stream, &error).await?;
            return Err(error.into());
        }
    };

    Socks5Handler.send_success_reply(stream, socket_addr_to_endpoint(upstream.bind_addr)).await?;

    relay_bidirectional(stream, &mut upstream.stream, plan.relay).await?;

    Ok(request)
}

async fn send_dial_failure_reply<S>(stream: &mut S, error: &DialError) -> Result<(), SessionError>
where
    S: AsyncWrite + Unpin,
{
    Socks5Handler.send_reply(stream, error.reply_code()).await.map_err(SessionError::from)
}

fn socket_addr_to_endpoint(address: SocketAddr) -> crate::session::TargetEndpoint {
    use crate::session::{TargetAddr, TargetEndpoint};

    let target_addr = match address.ip() {
        std::net::IpAddr::V4(ip) => TargetAddr::Ipv4(ip),
        std::net::IpAddr::V6(ip) => TargetAddr::Ipv6(ip),
    };

    TargetEndpoint { address: target_addr, port: address.port() }
}
