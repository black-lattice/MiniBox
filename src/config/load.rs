use std::fs;
use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::config::external::{ExternalConfig, ExternalConfigSource, ExternalDocument};
use crate::config::internal::ActiveConfig;
use crate::error::Error;

const HTTP_USER_AGENT: &str = "MiniBox/0.1";
const MAX_SUBSCRIPTION_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

pub async fn read_source_document(
    source: &ExternalConfigSource,
) -> Result<ExternalDocument, Error> {
    match source {
        ExternalConfigSource::LocalFile { path } => read_local_file_document(path.as_str()),
        ExternalConfigSource::ClashSubscription { url } => {
            let raw = fetch_clash_subscription_text(url).await?;
            Ok(ExternalDocument::new(source.clone(), raw))
        }
    }
}

pub fn read_local_file_document(path: impl AsRef<Path>) -> Result<ExternalDocument, Error> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path)?;

    Ok(ExternalDocument::new(
        ExternalConfigSource::LocalFile {
            path: path.display().to_string(),
        },
        raw,
    ))
}

pub fn parse_local_document(document: &ExternalDocument) -> Result<ExternalConfig, Error> {
    let path = match &document.source {
        ExternalConfigSource::LocalFile { path } => path,
        ExternalConfigSource::ClashSubscription { url } => {
            return Err(Error::validation(format!(
                "Clash subscription document '{}' must be translated through the Clash adapter",
                url
            )));
        }
    };

    serde_yaml::from_str(document.raw.as_str()).map_err(|error| {
        Error::validation(format!("failed to parse local config '{}': {error}", path))
    })
}

pub fn load_local_document(document: &ExternalDocument) -> Result<ActiveConfig, Error> {
    ActiveConfig::from_external(parse_local_document(document)?)
}

async fn fetch_clash_subscription_text(url: &str) -> Result<String, Error> {
    let source = ParsedHttpSource::parse(url)?;
    let mut stream = TcpStream::connect((source.connect_host.as_str(), source.port))
        .await
        .map_err(|error| {
            Error::io(format!(
                "failed to connect to Clash subscription '{}': {error}",
                url
            ))
        })?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAccept: text/plain, application/yaml, */*\r\nConnection: close\r\n\r\n",
        source.request_target, source.host_header, HTTP_USER_AGENT
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| {
            Error::io(format!(
                "failed to send Clash subscription request '{}': {error}",
                url
            ))
        })?;

    let response =
        read_http_response_bytes(&mut stream, url, MAX_SUBSCRIPTION_RESPONSE_BYTES).await?;

    parse_http_response(url, &response)
}

async fn read_http_response_bytes<S>(
    stream: &mut S,
    url: &str,
    max_response_bytes: usize,
) -> Result<Vec<u8>, Error>
where
    S: tokio::io::AsyncRead + Unpin,
{
    let mut response = Vec::with_capacity(max_response_bytes.min(8 * 1024));
    let mut chunk = [0u8; 8 * 1024];

    loop {
        let read = stream.read(&mut chunk).await.map_err(|error| {
            Error::io(format!(
                "failed to read Clash subscription response '{}': {error}",
                url
            ))
        })?;
        if read == 0 {
            break;
        }

        if response.len().saturating_add(read) > max_response_bytes {
            return Err(Error::validation(format!(
                "Clash subscription response '{}' exceeded {} bytes",
                url, max_response_bytes
            )));
        }

        response.extend_from_slice(&chunk[..read]);
    }

    Ok(response)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedHttpSource {
    connect_host: String,
    host_header: String,
    port: u16,
    request_target: String,
}

impl ParsedHttpSource {
    fn parse(url: &str) -> Result<Self, Error> {
        let Some((scheme, remainder)) = url.split_once("://") else {
            return Err(Error::validation(format!(
                "Clash subscription source '{}' must include an explicit URL scheme",
                url
            )));
        };

        match scheme {
            "http" => {}
            "https" => {
                return Err(Error::unsupported(format!(
                    "https Clash subscription loading is not supported in this stage for '{}'; use http:// or preload the document",
                    url
                )));
            }
            other => {
                return Err(Error::unsupported(format!(
                    "Clash subscription source '{}' uses unsupported scheme '{}'; only http:// is supported in this stage",
                    url, other
                )));
            }
        }

        if remainder.is_empty() {
            return Err(Error::validation(format!(
                "Clash subscription source '{}' is missing a host",
                url
            )));
        }

        let fragment = remainder.find('#');
        if fragment.is_some() {
            return Err(Error::validation(format!(
                "Clash subscription source '{}' must not contain a fragment",
                url
            )));
        }

        let authority_end = remainder.find(['/', '?']).unwrap_or(remainder.len());
        let authority = &remainder[..authority_end];
        let path_and_query = &remainder[authority_end..];
        if authority.is_empty() {
            return Err(Error::validation(format!(
                "Clash subscription source '{}' is missing a host",
                url
            )));
        }
        if authority.contains('@') {
            return Err(Error::unsupported(format!(
                "Clash subscription source '{}' must not include credentials",
                url
            )));
        }

        let (connect_host, host_header, port) = parse_authority(url, authority)?;
        let request_target = if path_and_query.is_empty() {
            "/".to_string()
        } else if path_and_query.starts_with('?') {
            format!("/{path_and_query}")
        } else {
            path_and_query.to_string()
        };

        Ok(Self {
            connect_host,
            host_header,
            port,
            request_target,
        })
    }
}

fn parse_authority(url: &str, authority: &str) -> Result<(String, String, u16), Error> {
    if authority.starts_with('[') {
        let Some(end) = authority.find(']') else {
            return Err(Error::validation(format!(
                "Clash subscription source '{}' has an invalid IPv6 host",
                url
            )));
        };
        let host = authority[1..end].to_string();
        let suffix = &authority[end + 1..];
        let port = if suffix.is_empty() {
            80
        } else if let Some(raw_port) = suffix.strip_prefix(':') {
            parse_port(url, raw_port)?
        } else {
            return Err(Error::validation(format!(
                "Clash subscription source '{}' has an invalid authority",
                url
            )));
        };
        let host_header = if port == 80 {
            format!("[{host}]")
        } else {
            format!("[{host}]:{port}")
        };
        return Ok((host, host_header, port));
    }

    let mut host = authority;
    let mut port = 80;
    if let Some((raw_host, raw_port)) = authority.rsplit_once(':') {
        if !raw_host.contains(':') {
            host = raw_host;
            port = parse_port(url, raw_port)?;
        }
    }

    if host.trim().is_empty() {
        return Err(Error::validation(format!(
            "Clash subscription source '{}' is missing a host",
            url
        )));
    }

    let host = host.trim().to_string();
    let host_header = if port == 80 {
        host.clone()
    } else {
        format!("{host}:{port}")
    };
    Ok((host, host_header, port))
}

fn parse_port(url: &str, raw_port: &str) -> Result<u16, Error> {
    raw_port.parse::<u16>().map_err(|_| {
        Error::validation(format!(
            "Clash subscription source '{}' has invalid port '{}'",
            url, raw_port
        ))
    })
}

fn parse_http_response(url: &str, response: &[u8]) -> Result<String, Error> {
    let Some(header_end) = find_header_end(response) else {
        return Err(Error::validation(format!(
            "Clash subscription response '{}' is missing HTTP headers",
            url
        )));
    };
    let headers = std::str::from_utf8(&response[..header_end]).map_err(|error| {
        Error::validation(format!(
            "Clash subscription response '{}' contains non-UTF-8 headers: {error}",
            url
        ))
    })?;
    let mut lines = headers.split("\r\n");
    let Some(status_line) = lines.next() else {
        return Err(Error::validation(format!(
            "Clash subscription response '{}' is missing a status line",
            url
        )));
    };
    let mut status_parts = status_line.split_whitespace();
    let version = status_parts.next().unwrap_or_default();
    let code = status_parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(Error::validation(format!(
            "Clash subscription response '{}' returned unsupported protocol '{}'",
            url, version
        )));
    }
    let status_code = code.parse::<u16>().map_err(|_| {
        Error::validation(format!(
            "Clash subscription response '{}' returned invalid status '{}'",
            url, code
        ))
    })?;
    if !(200..=299).contains(&status_code) {
        return Err(Error::io(format!(
            "failed to fetch Clash subscription '{}': HTTP {}",
            url, status_code
        )));
    }

    let mut content_length = None;
    let mut transfer_chunked = false;
    let mut content_encoding_identity = true;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(Error::validation(format!(
                "Clash subscription response '{}' contains malformed header '{}'",
                url, line
            )));
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        match name.as_str() {
            "content-length" => {
                content_length = Some(value.parse::<usize>().map_err(|_| {
                    Error::validation(format!(
                        "Clash subscription response '{}' contains invalid Content-Length '{}'",
                        url, value
                    ))
                })?);
            }
            "transfer-encoding" => {
                transfer_chunked = value
                    .split(',')
                    .map(|part| part.trim().to_ascii_lowercase())
                    .any(|part| part == "chunked");
            }
            "content-encoding" => {
                content_encoding_identity =
                    value.eq_ignore_ascii_case("identity") || value.is_empty();
            }
            _ => {}
        }
    }

    if !content_encoding_identity {
        return Err(Error::unsupported(format!(
            "Clash subscription response '{}' uses unsupported content encoding",
            url
        )));
    }

    let body = &response[header_end + 4..];
    let decoded = if transfer_chunked {
        decode_chunked_body(url, body)?
    } else if let Some(length) = content_length {
        if body.len() < length {
            return Err(Error::validation(format!(
                "Clash subscription response '{}' ended before Content-Length bytes were received",
                url
            )));
        }
        body[..length].to_vec()
    } else {
        body.to_vec()
    };

    String::from_utf8(decoded).map_err(|error| {
        Error::validation(format!(
            "Clash subscription response '{}' contains non-UTF-8 text: {error}",
            url
        ))
    })
}

fn find_header_end(response: &[u8]) -> Option<usize> {
    response.windows(4).position(|window| window == b"\r\n\r\n")
}

fn decode_chunked_body(url: &str, body: &[u8]) -> Result<Vec<u8>, Error> {
    let mut cursor = 0usize;
    let mut decoded = Vec::new();

    loop {
        let Some(line_end) = find_crlf(body, cursor) else {
            return Err(Error::validation(format!(
                "Clash subscription response '{}' has an incomplete chunk header",
                url
            )));
        };
        let line = std::str::from_utf8(&body[cursor..line_end]).map_err(|error| {
            Error::validation(format!(
                "Clash subscription response '{}' contains invalid chunk metadata: {error}",
                url
            ))
        })?;
        let size_token = line.split(';').next().unwrap_or_default().trim();
        let chunk_size = usize::from_str_radix(size_token, 16).map_err(|_| {
            Error::validation(format!(
                "Clash subscription response '{}' contains invalid chunk size '{}'",
                url, size_token
            ))
        })?;
        cursor = line_end + 2;

        if chunk_size == 0 {
            let Some(trailer_end) = find_crlf(body, cursor) else {
                return Err(Error::validation(format!(
                    "Clash subscription response '{}' has an incomplete chunk trailer",
                    url
                )));
            };
            if trailer_end != cursor {
                return Err(Error::unsupported(format!(
                    "Clash subscription response '{}' uses unsupported chunk trailers",
                    url
                )));
            }
            return Ok(decoded);
        }

        let chunk_end = cursor.saturating_add(chunk_size);
        if chunk_end + 2 > body.len() {
            return Err(Error::validation(format!(
                "Clash subscription response '{}' ended mid-chunk",
                url
            )));
        }

        decoded.extend_from_slice(&body[cursor..chunk_end]);
        if &body[chunk_end..chunk_end + 2] != b"\r\n" {
            return Err(Error::validation(format!(
                "Clash subscription response '{}' is missing chunk terminators",
                url
            )));
        }
        cursor = chunk_end + 2;
    }
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start..]
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|offset| start + offset)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::{
        MAX_SUBSCRIPTION_RESPONSE_BYTES, load_local_document, parse_http_response,
        read_http_response_bytes, read_source_document,
    };
    use crate::config::external::{
        ExternalConfig, ExternalConfigSource, ListenerInput, ListenerProtocolInput, NodeInput,
        TargetRefInput,
    };
    use crate::error::Error;

    #[tokio::test]
    async fn local_file_source_reads_and_loads_into_active_config() {
        let path = temp_config_path("local-file");
        let config = ExternalConfig {
            listeners: vec![ListenerInput {
                name: "local-socks".to_string(),
                bind: "127.0.0.1:1080".to_string(),
                protocol: ListenerProtocolInput::Socks5,
                target: TargetRefInput::node("node-a"),
            }],
            nodes: vec![NodeInput {
                name: "node-a".to_string(),
                address: "1.1.1.1:443".to_string(),
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        };
        let encoded = serde_yaml::to_string(&config).expect("config should serialize");
        fs::write(&path, encoded).expect("config file should be written");

        let document = read_source_document(&ExternalConfigSource::LocalFile {
            path: path.display().to_string(),
        })
        .await
        .expect("local file source should load");
        let active = load_local_document(&document).expect("local document should normalize");

        assert_eq!(active.listeners()[0].name, "local-socks");
        assert_eq!(active.nodes()[0].address, "1.1.1.1:443");

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn clash_subscription_source_reads_remote_text_over_http() {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("test server should bind: {error}"),
        };
        let addr = listener
            .local_addr()
            .expect("test server should expose addr");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let mut request = vec![0u8; 1024];
            let read = stream
                .read(&mut request)
                .await
                .expect("server should read request");
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("GET /subscription HTTP/1.1\r\n"));
            assert!(request.contains("Host: 127.0.0.1:"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\n\r\n1A\r\nproxies:\n  - name: edge-a\n\r\n0\r\n\r\n",
                )
                .await
                .expect("server should write response");
        });

        let document = read_source_document(&ExternalConfigSource::ClashSubscription {
            url: format!("http://127.0.0.1:{}/subscription", addr.port()),
        })
        .await
        .expect("remote Clash source should load");

        assert_eq!(document.raw, "proxies:\n  - name: edge-a\n");
        server.await.expect("server task should join");
    }

    #[tokio::test]
    async fn clash_subscription_reader_rejects_oversized_http_responses() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let server_task = tokio::spawn(async move {
            read_http_response_bytes(
                &mut server,
                "http://example.com/subscription",
                MAX_SUBSCRIPTION_RESPONSE_BYTES,
            )
            .await
        });

        let oversized = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            MAX_SUBSCRIPTION_RESPONSE_BYTES + 1,
            "a".repeat(MAX_SUBSCRIPTION_RESPONSE_BYTES + 1)
        );
        client
            .write_all(oversized.as_bytes())
            .await
            .expect("oversized response should write");
        client.shutdown().await.expect("client should shut down");

        let error = server_task
            .await
            .expect("server task should join")
            .expect_err("oversized response should fail");

        assert_eq!(
            error,
            Error::validation(format!(
                "Clash subscription response 'http://example.com/subscription' exceeded {} bytes",
                MAX_SUBSCRIPTION_RESPONSE_BYTES
            ))
        );
    }

    #[tokio::test]
    async fn https_clash_subscription_loading_stays_explicitly_unsupported() {
        let error = read_source_document(&ExternalConfigSource::ClashSubscription {
            url: "https://example.com/subscription".to_string(),
        })
        .await
        .expect_err("https loading should stay outside this minimal stage");

        assert_eq!(
            error,
            Error::unsupported(
                "https Clash subscription loading is not supported in this stage for 'https://example.com/subscription'; use http:// or preload the document",
            )
        );
    }

    #[test]
    fn chunked_http_response_is_decoded_into_text_body() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nhello \r\n6\r\nworld!\r\n0\r\n\r\n";
        let body = parse_http_response("http://example.com/sub", response)
            .expect("chunked response should decode");

        assert_eq!(body, "hello world!");
    }

    fn temp_config_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be monotonic enough for tests")
            .as_nanos();
        std::env::temp_dir().join(format!("minibox-{label}-{nonce}.yaml"))
    }
}
