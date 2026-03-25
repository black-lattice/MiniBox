use std::net::{Ipv4Addr, Ipv6Addr};

use crate::protocol::socks5::error::Socks5Error;
use crate::protocol::socks5::types::{AuthMethod, Command, Greeting, Request, VERSION};
use crate::session::{TargetAddr, TargetEndpoint};

pub fn parse_greeting(input: &[u8]) -> Result<(Greeting, usize), Socks5Error> {
    let header =
        input.get(..2).ok_or(Socks5Error::Truncated { expected: 2, actual: input.len() })?;

    ensure_version(header[0])?;

    let methods_len = header[1] as usize;
    let frame_len = 2 + methods_len;
    let methods = input
        .get(2..frame_len)
        .ok_or(Socks5Error::Truncated { expected: frame_len, actual: input.len() })?;

    Ok((
        Greeting { methods: methods.iter().copied().map(AuthMethod::from_byte).collect() },
        frame_len,
    ))
}

pub fn parse_request(input: &[u8]) -> Result<(Request, usize), Socks5Error> {
    let header =
        input.get(..4).ok_or(Socks5Error::Truncated { expected: 4, actual: input.len() })?;

    ensure_version(header[0])?;

    if header[2] != 0x00 {
        return Err(Socks5Error::InvalidReservedByte(header[2]));
    }

    let command = match header[1] {
        0x01 => Command::Connect,
        other => return Err(Socks5Error::UnsupportedCommand(other)),
    };

    let (destination, consumed) = parse_target_endpoint(&input[3..])?;

    Ok((Request { command, destination }, 3 + consumed))
}

pub fn parse_target_endpoint(input: &[u8]) -> Result<(TargetEndpoint, usize), Socks5Error> {
    let address_type = input
        .first()
        .copied()
        .ok_or(Socks5Error::Truncated { expected: 1, actual: input.len() })?;

    match address_type {
        0x01 => {
            let payload = input
                .get(1..7)
                .ok_or(Socks5Error::Truncated { expected: 7, actual: input.len() })?;
            let address = Ipv4Addr::new(payload[0], payload[1], payload[2], payload[3]);
            let port = u16::from_be_bytes([payload[4], payload[5]]);

            Ok((TargetEndpoint { address: TargetAddr::Ipv4(address), port }, 7))
        }
        0x03 => {
            let domain_len =
                *input.get(1).ok_or(Socks5Error::Truncated { expected: 2, actual: input.len() })?
                    as usize;

            if domain_len == 0 {
                return Err(Socks5Error::InvalidDomainLength);
            }

            let frame_len = 1 + 1 + domain_len + 2;
            let domain = input
                .get(2..2 + domain_len)
                .ok_or(Socks5Error::Truncated { expected: frame_len, actual: input.len() })?;
            let port_bytes = input
                .get(2 + domain_len..frame_len)
                .ok_or(Socks5Error::Truncated { expected: frame_len, actual: input.len() })?;
            let domain = std::str::from_utf8(domain).map_err(|_| Socks5Error::InvalidDomainName)?;
            if domain.is_empty() {
                return Err(Socks5Error::InvalidDomainName);
            }

            Ok((
                TargetEndpoint {
                    address: TargetAddr::Domain(domain.to_string()),
                    port: u16::from_be_bytes([port_bytes[0], port_bytes[1]]),
                },
                frame_len,
            ))
        }
        0x04 => {
            let payload = input
                .get(1..19)
                .ok_or(Socks5Error::Truncated { expected: 19, actual: input.len() })?;
            let address = Ipv6Addr::from([
                payload[0],
                payload[1],
                payload[2],
                payload[3],
                payload[4],
                payload[5],
                payload[6],
                payload[7],
                payload[8],
                payload[9],
                payload[10],
                payload[11],
                payload[12],
                payload[13],
                payload[14],
                payload[15],
            ]);
            let port = u16::from_be_bytes([payload[16], payload[17]]);

            Ok((TargetEndpoint { address: TargetAddr::Ipv6(address), port }, 19))
        }
        other => Err(Socks5Error::UnsupportedAddressType(other)),
    }
}

fn ensure_version(version: u8) -> Result<(), Socks5Error> {
    if version == VERSION { Ok(()) } else { Err(Socks5Error::InvalidVersion(version)) }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::{parse_greeting, parse_request, parse_target_endpoint};
    use crate::protocol::socks5::{
        AuthMethod, Command, Greeting, Request, Socks5Error, TargetAddr, TargetEndpoint,
    };

    #[test]
    fn parses_no_auth_greeting() {
        let input = [0x05, 0x02, 0x00, 0x02, 0xff];

        let (greeting, consumed) = parse_greeting(&input).expect("greeting should parse");

        assert_eq!(
            greeting,
            Greeting { methods: vec![AuthMethod::NoAuth, AuthMethod::UsernamePassword] }
        );
        assert!(greeting.supports_no_auth());
        assert_eq!(consumed, 4);
    }

    #[test]
    fn parses_connect_request_with_ipv4_target() {
        let input = [0x05, 0x01, 0x00, 0x01, 10, 0, 0, 7, 0x01, 0xbb];

        let (request, consumed) = parse_request(&input).expect("request should parse");

        assert_eq!(
            request,
            Request {
                command: Command::Connect,
                destination: TargetEndpoint {
                    address: TargetAddr::Ipv4(Ipv4Addr::new(10, 0, 0, 7)),
                    port: 443,
                }
            }
        );
        assert_eq!(consumed, input.len());
    }

    #[test]
    fn parses_domain_target() {
        let input = [
            0x03, 0x0b, b'e', b'x', b'a', b'm', b'p', b'l', b'e', b'.', b'c', b'o', b'm', 0x00,
            0x50,
        ];

        let (endpoint, consumed) = parse_target_endpoint(&input).expect("domain should parse");

        assert_eq!(
            endpoint,
            TargetEndpoint { address: TargetAddr::Domain("example.com".to_string()), port: 80 }
        );
        assert_eq!(consumed, input.len());
    }

    #[test]
    fn parses_ipv6_target() {
        let input = [0x04, 0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0x01, 0xbb];

        let (endpoint, consumed) = parse_target_endpoint(&input).expect("ipv6 should parse");

        assert_eq!(
            endpoint,
            TargetEndpoint {
                address: TargetAddr::Ipv6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1)),
                port: 443,
            }
        );
        assert_eq!(consumed, input.len());
    }

    #[test]
    fn rejects_unsupported_command() {
        let input = [0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x35];

        let error = parse_request(&input).expect_err("bind is intentionally unsupported");

        assert_eq!(error, Socks5Error::UnsupportedCommand(0x02));
    }

    #[test]
    fn rejects_truncated_target() {
        let error = parse_target_endpoint(&[0x01, 127, 0]).expect_err("target is truncated");

        assert_eq!(error, Socks5Error::Truncated { expected: 7, actual: 3 });
    }
}
