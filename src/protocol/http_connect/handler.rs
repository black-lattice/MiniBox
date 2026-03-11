use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::protocol::http_connect::codec::encode_response;
use crate::protocol::http_connect::error::{HttpConnectError, HttpConnectHandshakeError};
use crate::protocol::http_connect::parser::{find_header_terminator, parse_request};
use crate::protocol::http_connect::{AcceptedRequest, StatusCode};

pub const DEFAULT_MAX_HEADER_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpConnectHandler {
    pub max_header_bytes: usize,
}

impl Default for HttpConnectHandler {
    fn default() -> Self {
        Self {
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
        }
    }
}

impl HttpConnectHandler {
    pub async fn accept_connect<S>(
        &self,
        stream: &mut S,
    ) -> Result<AcceptedRequest, HttpConnectHandshakeError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut buffer = Vec::with_capacity(1024);

        let header_len = loop {
            if let Some(header_len) = find_header_terminator(&buffer) {
                break header_len;
            }

            if buffer.len() >= self.max_header_bytes {
                let error = HttpConnectError::HeadersTooLarge {
                    max_bytes: self.max_header_bytes,
                };
                self.send_response(stream, error.status_code()).await?;
                return Err(error.into());
            }

            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(HttpConnectError::UnexpectedEof.into());
            }

            buffer.extend_from_slice(&chunk[..read]);
        };

        let request = match parse_request(&buffer[..header_len]) {
            Ok(request) => request,
            Err(error) => {
                self.send_response(stream, error.status_code()).await?;
                return Err(error.into());
            }
        };

        Ok(AcceptedRequest {
            destination: request.destination,
            buffered_bytes: buffer.split_off(header_len),
        })
    }

    pub async fn send_response<S>(&self, stream: &mut S, status: StatusCode) -> std::io::Result<()>
    where
        S: AsyncWrite + Unpin,
    {
        stream.write_all(&encode_response(status)).await?;
        stream.flush().await
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::HttpConnectHandler;
    use crate::protocol::http_connect::StatusCode;
    use crate::protocol::http_connect::error::{HttpConnectError, HttpConnectHandshakeError};
    use crate::session::{TargetAddr, TargetEndpoint};

    #[tokio::test]
    async fn accepts_connect_request_and_preserves_buffered_bytes() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let server_task = tokio::spawn(async move {
            HttpConnectHandler::default()
                .accept_connect(&mut server)
                .await
        });

        client
            .write_all(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\nping")
            .await
            .expect("write request");

        let request = server_task
            .await
            .expect("server task should join")
            .expect("request should parse");

        assert_eq!(
            request.destination,
            TargetEndpoint {
                address: TargetAddr::Domain("example.com".to_string()),
                port: 443,
            }
        );
        assert_eq!(request.buffered_bytes, b"ping");
    }

    #[tokio::test]
    async fn rejects_non_connect_method_with_http_error_response() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let server_task = tokio::spawn(async move {
            HttpConnectHandler::default()
                .accept_connect(&mut server)
                .await
        });

        client
            .write_all(b"GET example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
            .await
            .expect("write request");

        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("read response");
        assert_eq!(
            response,
            crate::protocol::http_connect::codec::encode_response(StatusCode::MethodNotAllowed)
        );

        let error = server_task
            .await
            .expect("server task should join")
            .expect_err("request should fail");

        assert!(matches!(
            error,
            HttpConnectHandshakeError::Protocol(HttpConnectError::UnsupportedMethod(method))
                if method == "GET"
        ));
    }

    #[tokio::test]
    async fn rejects_invalid_target_with_bad_request_response() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let server_task = tokio::spawn(async move {
            HttpConnectHandler::default()
                .accept_connect(&mut server)
                .await
        });

        client
            .write_all(b"CONNECT https://example.com HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
            .await
            .expect("write request");

        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("read response");
        assert_eq!(
            response,
            crate::protocol::http_connect::codec::encode_response(StatusCode::BadRequest)
        );

        let error = server_task
            .await
            .expect("server task should join")
            .expect_err("request should fail");

        assert!(matches!(
            error,
            HttpConnectHandshakeError::Protocol(HttpConnectError::InvalidTarget)
        ));
    }
}
