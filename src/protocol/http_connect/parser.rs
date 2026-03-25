use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use crate::protocol::http_connect::HttpConnectError;
use crate::protocol::http_connect::{Request, Version};
use crate::session::{TargetAddr, TargetEndpoint};

pub fn parse_request(input: &[u8]) -> Result<Request, HttpConnectError> {
    let request = std::str::from_utf8(input).map_err(|_| HttpConnectError::InvalidEncoding)?;
    let request = request.strip_suffix("\r\n\r\n").ok_or(HttpConnectError::InvalidRequestLine)?;
    let mut lines = request.split("\r\n");
    let request_line = lines.next().ok_or(HttpConnectError::InvalidRequestLine)?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts.next().ok_or(HttpConnectError::InvalidRequestLine)?;
    let target = parts.next().ok_or(HttpConnectError::InvalidRequestLine)?;
    let version = parts.next().ok_or(HttpConnectError::InvalidRequestLine)?;

    if parts.next().is_some() {
        return Err(HttpConnectError::InvalidRequestLine);
    }

    if method != "CONNECT" {
        return Err(HttpConnectError::UnsupportedMethod(method.to_string()));
    }

    let version = match version {
        "HTTP/1.0" => Version::Http10,
        "HTTP/1.1" => Version::Http11,
        other => return Err(HttpConnectError::UnsupportedVersion(other.to_string())),
    };

    for line in lines {
        if line.is_empty() {
            continue;
        }

        let Some((name, _value)) = line.split_once(':') else {
            return Err(HttpConnectError::InvalidHeader);
        };
        if name.is_empty() || name.chars().any(|ch| ch.is_ascii_whitespace() || ch.is_control()) {
            return Err(HttpConnectError::InvalidHeader);
        }
    }

    Ok(Request { version, destination: parse_target(target)? })
}

pub fn find_header_terminator(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n").map(|index| index + 4)
}

fn parse_target(target: &str) -> Result<TargetEndpoint, HttpConnectError> {
    if let Some(remainder) = target.strip_prefix('[') {
        let Some((host, port)) = remainder.split_once("]:") else {
            return Err(HttpConnectError::InvalidTarget);
        };
        let host = Ipv6Addr::from_str(host).map_err(|_| HttpConnectError::InvalidTarget)?;
        let port = parse_port(port)?;

        return Ok(TargetEndpoint { address: TargetAddr::Ipv6(host), port });
    }

    let Some((host, port)) = target.rsplit_once(':') else {
        return Err(HttpConnectError::InvalidTarget);
    };
    if host.is_empty() || host.contains(':') || !is_valid_host(host) {
        return Err(HttpConnectError::InvalidTarget);
    }

    let port = parse_port(port)?;
    let address = match Ipv4Addr::from_str(host) {
        Ok(ip) => TargetAddr::Ipv4(ip),
        Err(_) => TargetAddr::Domain(host.to_string()),
    };

    Ok(TargetEndpoint { address, port })
}

fn parse_port(port: &str) -> Result<u16, HttpConnectError> {
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(HttpConnectError::InvalidTarget);
    }

    let port = port.parse::<u16>().map_err(|_| HttpConnectError::InvalidTarget)?;
    if port == 0 {
        return Err(HttpConnectError::InvalidTarget);
    }

    Ok(port)
}

fn is_valid_host(host: &str) -> bool {
    host.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::{find_header_terminator, parse_request};
    use crate::protocol::http_connect::{HttpConnectError, Request, Version};
    use crate::session::{TargetAddr, TargetEndpoint};

    #[test]
    fn parses_connect_request_with_domain_target() {
        let request =
            parse_request(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
                .expect("request should parse");

        assert_eq!(
            request,
            Request {
                version: Version::Http11,
                destination: TargetEndpoint {
                    address: TargetAddr::Domain("example.com".to_string()),
                    port: 443,
                },
            }
        );
    }

    #[test]
    fn parses_connect_request_with_ipv4_target() {
        let request =
            parse_request(b"CONNECT 127.0.0.1:8080 HTTP/1.0\r\nHost: 127.0.0.1:8080\r\n\r\n")
                .expect("request should parse");

        assert_eq!(
            request,
            Request {
                version: Version::Http10,
                destination: TargetEndpoint {
                    address: TargetAddr::Ipv4(Ipv4Addr::LOCALHOST),
                    port: 8080,
                },
            }
        );
    }

    #[test]
    fn parses_connect_request_with_ipv6_target() {
        let request =
            parse_request(b"CONNECT [2001:db8::1]:443 HTTP/1.1\r\nHost: [2001:db8::1]:443\r\n\r\n")
                .expect("request should parse");

        assert_eq!(
            request,
            Request {
                version: Version::Http11,
                destination: TargetEndpoint {
                    address: TargetAddr::Ipv6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1)),
                    port: 443,
                },
            }
        );
    }

    #[test]
    fn rejects_non_connect_method() {
        let error = parse_request(b"GET example.com:443 HTTP/1.1\r\nHost: example.com\r\n\r\n")
            .expect_err("method should be rejected");

        assert_eq!(error, HttpConnectError::UnsupportedMethod("GET".to_string()));
    }

    #[test]
    fn rejects_invalid_target_authority() {
        let error =
            parse_request(b"CONNECT https://example.com HTTP/1.1\r\nHost: example.com\r\n\r\n")
                .expect_err("absolute-form target should be rejected");

        assert_eq!(error, HttpConnectError::InvalidTarget);
    }

    #[test]
    fn finds_header_terminator_with_buffered_tunnel_bytes() {
        let input = b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\nping";

        assert_eq!(find_header_terminator(input), Some(input.len() - b"ping".len()));
    }
}
