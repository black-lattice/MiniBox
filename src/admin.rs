use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinError, JoinHandle};

use crate::error::Error;
use crate::health::ProbeReport;
use crate::metrics::render_prometheus_text;
use crate::runtime::RuntimeState;

const MAX_ADMIN_REQUEST_BYTES: usize = 8 * 1024;

#[derive(Debug)]
pub struct AdminTaskHandle {
    local_addr: SocketAddr,
    task: JoinHandle<Result<(), Error>>,
}

impl AdminTaskHandle {
    fn new(local_addr: SocketAddr, task: JoinHandle<Result<(), Error>>) -> Self {
        Self { local_addr, task }
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn abort(&self) {
        self.task.abort();
    }

    pub async fn join(self) -> Result<Result<(), Error>, JoinError> {
        self.task.await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdminRequest {
    method: String,
    path: String,
    authorization: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdminResponse {
    status_code: u16,
    reason_phrase: &'static str,
    content_type: &'static str,
    body: String,
}

pub async fn spawn_admin_server(runtime: RuntimeState) -> Result<Option<AdminTaskHandle>, Error> {
    let Some(bind) = runtime.admin_bind() else {
        return Ok(None);
    };

    let listener = TcpListener::bind(bind).await.map_err(|error| {
        Error::io(format!("failed to bind admin endpoint on '{bind}': {error}"))
    })?;
    let local_addr = listener
        .local_addr()
        .map_err(|error| Error::io(format!("failed to inspect admin listener address: {error}")))?;
    let access_token = runtime.admin_access_token().map(ToOwned::to_owned);
    let task = tokio::spawn(run_admin_server(listener, runtime, access_token));

    Ok(Some(AdminTaskHandle::new(local_addr, task)))
}

async fn run_admin_server(
    listener: TcpListener,
    runtime: RuntimeState,
    access_token: Option<String>,
) -> Result<(), Error> {
    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|error| Error::io(format!("admin accept loop failed: {error}")))?;
        let runtime = runtime.clone();
        let access_token = access_token.clone();
        tokio::spawn(async move {
            let _ = handle_admin_connection(stream, runtime, access_token).await;
        });
    }
}

async fn handle_admin_connection(
    mut stream: TcpStream,
    runtime: RuntimeState,
    access_token: Option<String>,
) -> Result<(), Error> {
    let request = match read_admin_request(&mut stream).await {
        Ok(request) => request,
        Err(error) => {
            write_admin_response(
                &mut stream,
                "GET",
                AdminResponse {
                    status_code: 400,
                    reason_phrase: "Bad Request",
                    content_type: "text/plain; charset=utf-8",
                    body: format!("bad request: {error}\n"),
                },
            )
            .await?;
            return Ok(());
        }
    };

    let response = route_admin_request(&runtime, &request, access_token.as_deref());
    write_admin_response(&mut stream, request.method.as_str(), response).await?;
    Ok(())
}

fn route_admin_request(
    runtime: &RuntimeState,
    request: &AdminRequest,
    access_token: Option<&str>,
) -> AdminResponse {
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return text_response(405, "Method Not Allowed", "method not allowed\n");
    }

    if let Some(token) = access_token {
        let expected = format!("Bearer {token}");
        if request.authorization.as_deref() != Some(expected.as_str()) {
            return AdminResponse {
                status_code: 401,
                reason_phrase: "Unauthorized",
                content_type: "text/plain; charset=utf-8",
                body: "missing or invalid admin bearer token\n".to_string(),
            };
        }
    }

    match request.path.split('?').next().unwrap_or(request.path.as_str()) {
        "/healthz" => probe_response(runtime.liveness_report()),
        "/readyz" => probe_response(runtime.readiness_report()),
        "/metrics" => AdminResponse {
            status_code: 200,
            reason_phrase: "OK",
            content_type: "text/plain; version=0.0.4; charset=utf-8",
            body: render_prometheus_text(&runtime.metrics_snapshot()),
        },
        _ => text_response(404, "Not Found", "not found\n"),
    }
}

fn probe_response(report: ProbeReport) -> AdminResponse {
    AdminResponse {
        status_code: report.http_status_code(),
        reason_phrase: if report.http_status_code() == 200 { "OK" } else { "Service Unavailable" },
        content_type: "text/plain; charset=utf-8",
        body: report.render_text_body(),
    }
}

fn text_response(status_code: u16, reason_phrase: &'static str, body: &str) -> AdminResponse {
    AdminResponse {
        status_code,
        reason_phrase,
        content_type: "text/plain; charset=utf-8",
        body: body.to_string(),
    }
}

async fn read_admin_request(stream: &mut TcpStream) -> Result<AdminRequest, Error> {
    let mut buffer = vec![0u8; MAX_ADMIN_REQUEST_BYTES];
    let mut filled = 0usize;
    let header_end;

    loop {
        if filled == buffer.len() {
            return Err(Error::validation("admin request headers exceeded 8KiB"));
        }

        let read = stream
            .read(&mut buffer[filled..])
            .await
            .map_err(|error| Error::io(format!("failed to read admin request: {error}")))?;
        if read == 0 {
            return Err(Error::validation("admin client closed connection before sending headers"));
        }
        filled += read;

        if let Some(index) = buffer[..filled].windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = index + 4;
            break;
        }
    }

    parse_admin_request(&buffer[..header_end])
}

fn parse_admin_request(raw: &[u8]) -> Result<AdminRequest, Error> {
    let header = std::str::from_utf8(raw).map_err(|error| {
        Error::validation(format!("admin request was not valid UTF-8: {error}"))
    })?;
    let mut lines = header.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| Error::validation("admin request was missing a request line"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| Error::validation("admin request line was missing method"))?;
    let path = request_parts
        .next()
        .ok_or_else(|| Error::validation("admin request line was missing path"))?;
    if request_parts.next().is_none() {
        return Err(Error::validation("admin request line was missing HTTP version"));
    }

    let mut authorization = None;
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("authorization")
        {
            authorization = Some(value.trim().to_string());
        }
    }

    Ok(AdminRequest { method: method.to_string(), path: path.to_string(), authorization })
}

async fn write_admin_response(
    stream: &mut TcpStream,
    method: &str,
    response: AdminResponse,
) -> Result<(), Error> {
    let headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status_code,
        response.reason_phrase,
        response.content_type,
        response.body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .map_err(|error| Error::io(format!("failed to write admin response headers: {error}")))?;
    if method != "HEAD" {
        stream
            .write_all(response.body.as_bytes())
            .await
            .map_err(|error| Error::io(format!("failed to write admin response body: {error}")))?;
    }
    stream
        .shutdown()
        .await
        .map_err(|error| Error::io(format!("failed to shutdown admin response stream: {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    use super::spawn_admin_server;
    use crate::config::external::{
        AdminInput, ExternalConfig, ListenerInput, ListenerProtocolInput, NodeInput, TargetRefInput,
    };
    use crate::config::internal::ActiveConfig;
    use crate::error::Error;
    use crate::runtime::RuntimeState;

    #[tokio::test]
    async fn admin_server_exposes_health_ready_and_metrics_endpoints() {
        let runtime = test_runtime(AdminInput {
            enabled: true,
            bind: Some("127.0.0.1:0".to_string()),
            access_token: None,
        });
        let handle = match spawn_admin_server(runtime.clone()).await {
            Ok(Some(handle)) => handle,
            Ok(None) => panic!("admin should be enabled"),
            Err(Error::Io(message)) if message.contains("Operation not permitted") => return,
            Err(error) => panic!("admin server should bind: {error}"),
        };

        let health =
            request(handle.local_addr(), "GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
        assert!(health.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(health.contains("status=ready"));

        let readiness =
            request(handle.local_addr(), "GET /readyz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
        assert!(readiness.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
        assert!(readiness.contains("status=starting"));

        runtime.update_readiness(
            crate::health::ProbeStatus::Ready,
            "listeners bound and active config loaded".to_string(),
        );
        let ready =
            request(handle.local_addr(), "GET /readyz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
        assert!(ready.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(ready.contains("status=ready"));

        let metrics =
            request(handle.local_addr(), "GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
        assert!(metrics.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(metrics.contains("minibox_runtime_readiness"));
        assert!(metrics.contains("minibox_connections_active"));

        handle.abort();
        let join = handle.join().await.expect_err("admin task should cancel");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn admin_server_enforces_bearer_token_when_configured() {
        let runtime = test_runtime(AdminInput {
            enabled: true,
            bind: Some("127.0.0.1:0".to_string()),
            access_token: Some("secret-token".to_string()),
        });
        let handle = match spawn_admin_server(runtime).await {
            Ok(Some(handle)) => handle,
            Ok(None) => panic!("admin should be enabled"),
            Err(Error::Io(message)) if message.contains("Operation not permitted") => return,
            Err(error) => panic!("admin server should bind: {error}"),
        };

        let unauthorized =
            request(handle.local_addr(), "GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
        assert!(unauthorized.starts_with("HTTP/1.1 401 Unauthorized\r\n"));

        let authorized = request(
            handle.local_addr(),
            "GET /metrics HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret-token\r\n\r\n",
        )
        .await;
        assert!(authorized.starts_with("HTTP/1.1 200 OK\r\n"));

        handle.abort();
        let join = handle.join().await.expect_err("admin task should cancel");
        assert!(join.is_cancelled());
    }

    fn test_runtime(admin: AdminInput) -> RuntimeState {
        let active_config = ActiveConfig::from_external(ExternalConfig {
            listeners: vec![ListenerInput {
                name: "local-socks".to_string(),
                bind: "127.0.0.1:0".to_string(),
                protocol: ListenerProtocolInput::Socks5,
                target: TargetRefInput::node("node-a"),
            }],
            nodes: vec![NodeInput {
                name: "node-a".to_string(),
                kind: crate::config::external::NodeKindInput::DirectTcp,
                address: None,
                server: None,
                port: None,
                password: None,
                sni: None,
                skip_cert_verify: false,
                provider: None,
                subscription: None,
            }],
            admin,
            ..ExternalConfig::default()
        })
        .expect("runtime config should normalize");

        RuntimeState::new(active_config)
    }

    async fn request(addr: SocketAddr, raw_request: &str) -> String {
        let mut stream = TcpStream::connect(addr).await.expect("admin client should connect");
        stream.write_all(raw_request.as_bytes()).await.expect("admin request should write");
        stream.shutdown().await.expect("admin request should shutdown");

        let mut response = Vec::new();
        timeout(Duration::from_secs(1), stream.read_to_end(&mut response))
            .await
            .expect("admin response should arrive")
            .expect("admin response should read");
        String::from_utf8(response).expect("admin response should be utf8")
    }
}
