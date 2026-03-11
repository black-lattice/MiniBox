use std::fmt::{Display, Formatter};

use crate::protocol::http_connect::StatusCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpConnectError {
    UnexpectedEof,
    HeadersTooLarge { max_bytes: usize },
    InvalidEncoding,
    InvalidRequestLine,
    UnsupportedMethod(String),
    UnsupportedVersion(String),
    InvalidHeader,
    InvalidTarget,
}

impl HttpConnectError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::UnsupportedMethod(_) => StatusCode::MethodNotAllowed,
            Self::HeadersTooLarge { .. } => StatusCode::RequestHeaderFieldsTooLarge,
            _ => StatusCode::BadRequest,
        }
    }
}

impl Display for HttpConnectError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected EOF while reading HTTP CONNECT headers"),
            Self::HeadersTooLarge { max_bytes } => {
                write!(f, "HTTP CONNECT headers exceeded {max_bytes} bytes")
            }
            Self::InvalidEncoding => write!(f, "HTTP CONNECT request is not valid UTF-8"),
            Self::InvalidRequestLine => write!(f, "invalid HTTP CONNECT request line"),
            Self::UnsupportedMethod(method) => {
                write!(f, "unsupported HTTP method for tunnel request: {method}")
            }
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported HTTP version for CONNECT request: {version}")
            }
            Self::InvalidHeader => write!(f, "invalid HTTP header line in CONNECT request"),
            Self::InvalidTarget => write!(f, "invalid HTTP CONNECT target authority"),
        }
    }
}

impl std::error::Error for HttpConnectError {}

#[derive(Debug)]
pub enum HttpConnectHandshakeError {
    Io(std::io::Error),
    Protocol(HttpConnectError),
}

impl Display for HttpConnectHandshakeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "HTTP CONNECT handshake I/O error: {error}"),
            Self::Protocol(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for HttpConnectHandshakeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Protocol(error) => Some(error),
        }
    }
}

impl From<std::io::Error> for HttpConnectHandshakeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<HttpConnectError> for HttpConnectHandshakeError {
    fn from(error: HttpConnectError) -> Self {
        Self::Protocol(error)
    }
}
