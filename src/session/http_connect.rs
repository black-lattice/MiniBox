use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::protocol::http_connect::{HttpConnectHandler, StatusCode};
use crate::relay::relay_bidirectional;
use crate::session::{SessionContext, SessionError, SessionPlan, SessionRequest};
use crate::upstream::{connect_upstream, resolve_connect_route};

pub async fn accept_downstream<S>(
    stream: &mut S,
    context: &SessionContext,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let handler = HttpConnectHandler::default();
    let request = handler.accept_connect(stream).await?;

    Ok(SessionRequest { context: context.clone(), requested_target: request.destination })
}

pub async fn reject_deferred_connect<S>(stream: &mut S) -> Result<(), SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    HttpConnectHandler::default()
        .send_response(stream, StatusCode::BadGateway)
        .await
        .map_err(SessionError::from)
}

pub async fn drive_session<S>(
    stream: &mut S,
    context: SessionContext,
    plan: SessionPlan,
) -> Result<SessionRequest, SessionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let handler = HttpConnectHandler::default();
    let request = handler.accept_connect(stream).await?;
    let session_request = SessionRequest { context, requested_target: request.destination };
    let route = match resolve_connect_route(&session_request) {
        Ok(route) => route,
        Err(error) => {
            handler.send_response(stream, StatusCode::BadRequest).await?;
            return Err(error.into());
        }
    };

    let mut upstream = match connect_upstream(&route, plan.direct_dial, plan.trojan_dial).await {
        Ok(result) => result,
        Err(error) => {
            handler.send_response(stream, StatusCode::BadGateway).await?;
            return Err(error.into());
        }
    };

    handler.send_response(stream, StatusCode::ConnectionEstablished).await?;

    if !request.buffered_bytes.is_empty() {
        upstream.stream.write_all(&request.buffered_bytes).await?;
    }

    relay_bidirectional(stream, &mut upstream.stream, plan.relay).await?;

    Ok(session_request)
}
