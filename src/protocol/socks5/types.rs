pub const VERSION: u8 = 0x05;

use crate::session::{TargetAddr, TargetEndpoint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    NoAuth,
    GssApi,
    UsernamePassword,
    NoAcceptableMethods,
    Private(u8),
    Other(u8),
}

impl AuthMethod {
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0x00 => Self::NoAuth,
            0x01 => Self::GssApi,
            0x02 => Self::UsernamePassword,
            0xff => Self::NoAcceptableMethods,
            0x80..=0xfe => Self::Private(byte),
            _ => Self::Other(byte),
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Self::NoAuth => 0x00,
            Self::GssApi => 0x01,
            Self::UsernamePassword => 0x02,
            Self::NoAcceptableMethods => 0xff,
            Self::Private(byte) | Self::Other(byte) => byte,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Greeting {
    pub methods: Vec<AuthMethod>,
}

impl Greeting {
    pub fn supports_no_auth(&self) -> bool {
        self.methods.contains(&AuthMethod::NoAuth)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodSelection {
    pub method: AuthMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Connect,
}

impl Command {
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Connect => 0x01,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressKind {
    Ipv4,
    Domain,
    Ipv6,
}

impl AddressKind {
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Ipv4 => 0x01,
            Self::Domain => 0x03,
            Self::Ipv6 => 0x04,
        }
    }
}

impl TargetAddr {
    pub fn kind(&self) -> AddressKind {
        match self {
            Self::Ipv4(_) => AddressKind::Ipv4,
            Self::Domain(_) => AddressKind::Domain,
            Self::Ipv6(_) => AddressKind::Ipv6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub command: Command,
    pub destination: TargetEndpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyCode {
    Succeeded,
    GeneralFailure,
    ConnectionNotAllowed,
    NetworkUnreachable,
    HostUnreachable,
    ConnectionRefused,
    TtlExpired,
    CommandNotSupported,
    AddressTypeNotSupported,
}

impl ReplyCode {
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Succeeded => 0x00,
            Self::GeneralFailure => 0x01,
            Self::ConnectionNotAllowed => 0x02,
            Self::NetworkUnreachable => 0x03,
            Self::HostUnreachable => 0x04,
            Self::ConnectionRefused => 0x05,
            Self::TtlExpired => 0x06,
            Self::CommandNotSupported => 0x07,
            Self::AddressTypeNotSupported => 0x08,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub reply: ReplyCode,
    pub bind: TargetEndpoint,
}
