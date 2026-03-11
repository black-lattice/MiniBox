use std::error::Error as StdError;
use std::fmt::{Display, Formatter};

use crate::protocol::http_connect::HttpConnectHandshakeError;
use crate::protocol::socks5::Socks5HandshakeError;
use crate::upstream::{DialError, ResolveError};

#[derive(Debug)]
pub enum SessionError {
    Io(std::io::Error),
    Socks5(Socks5HandshakeError),
    HttpConnect(HttpConnectHandshakeError),
    Resolve(ResolveError),
    Dial(DialError),
    Unimplemented(String),
}

impl SessionError {
    pub fn unimplemented(message: impl Into<String>) -> Self {
        Self::Unimplemented(message.into())
    }
}

impl Display for SessionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "session I/O error: {error}"),
            Self::Socks5(error) => write!(f, "SOCKS5 session error: {error}"),
            Self::HttpConnect(error) => write!(f, "HTTP CONNECT session error: {error}"),
            Self::Resolve(error) => write!(f, "upstream target resolution error: {error}"),
            Self::Dial(error) => write!(f, "upstream dial error: {error}"),
            Self::Unimplemented(message) => write!(f, "session path not implemented: {message}"),
        }
    }
}

impl StdError for SessionError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Socks5(error) => Some(error),
            Self::HttpConnect(error) => Some(error),
            Self::Resolve(error) => Some(error),
            Self::Dial(error) => Some(error),
            Self::Unimplemented(_) => None,
        }
    }
}

impl From<std::io::Error> for SessionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<Socks5HandshakeError> for SessionError {
    fn from(error: Socks5HandshakeError) -> Self {
        Self::Socks5(error)
    }
}

impl From<HttpConnectHandshakeError> for SessionError {
    fn from(error: HttpConnectHandshakeError) -> Self {
        Self::HttpConnect(error)
    }
}

impl From<ResolveError> for SessionError {
    fn from(error: ResolveError) -> Self {
        Self::Resolve(error)
    }
}

impl From<DialError> for SessionError {
    fn from(error: DialError) -> Self {
        Self::Dial(error)
    }
}
