use std::error::Error as StdError;
use std::fmt::{Display, Formatter};

use crate::protocol::socks5::Socks5HandshakeError;

#[derive(Debug)]
pub enum SessionError {
    Io(std::io::Error),
    Socks5(Socks5HandshakeError),
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
            Self::Unimplemented(message) => write!(f, "session path not implemented: {message}"),
        }
    }
}

impl StdError for SessionError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Socks5(error) => Some(error),
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
