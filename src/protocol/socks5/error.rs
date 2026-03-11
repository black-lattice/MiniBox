use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Socks5Error {
    Truncated { expected: usize, actual: usize },
    InvalidVersion(u8),
    InvalidReservedByte(u8),
    UnsupportedCommand(u8),
    UnsupportedAddressType(u8),
    InvalidDomainLength,
    InvalidDomainName,
}

impl Display for Socks5Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated { expected, actual } => {
                write!(
                    f,
                    "truncated SOCKS5 frame: expected at least {expected} bytes, got {actual}"
                )
            }
            Self::InvalidVersion(version) => {
                write!(f, "invalid SOCKS5 version byte {version:#04x}")
            }
            Self::InvalidReservedByte(value) => {
                write!(f, "invalid SOCKS5 reserved byte {value:#04x}")
            }
            Self::UnsupportedCommand(command) => {
                write!(f, "unsupported SOCKS5 command {command:#04x}")
            }
            Self::UnsupportedAddressType(address_type) => {
                write!(f, "unsupported SOCKS5 address type {address_type:#04x}")
            }
            Self::InvalidDomainLength => write!(f, "invalid SOCKS5 domain length"),
            Self::InvalidDomainName => write!(f, "invalid SOCKS5 domain name"),
        }
    }
}

impl std::error::Error for Socks5Error {}
