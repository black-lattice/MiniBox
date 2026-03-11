use std::net::Ipv4Addr;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::protocol::socks5::codec::{encode_method_selection, encode_response};
use crate::protocol::socks5::error::{Socks5Error, Socks5HandshakeError};
use crate::protocol::socks5::parser::{parse_greeting, parse_request};
use crate::protocol::socks5::types::{
    AuthMethod, MethodSelection, ReplyCode, Request, Response, TargetAddr, TargetEndpoint,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct Socks5Handler;

impl Socks5Handler {
    pub async fn accept_connect<S>(&self, stream: &mut S) -> Result<Request, Socks5HandshakeError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let greeting = self.read_greeting(stream).await?;

        if !greeting.supports_no_auth() {
            self.write_method_selection(stream, AuthMethod::NoAcceptableMethods)
                .await?;
            return Err(Socks5HandshakeError::NoAcceptableAuthMethod);
        }

        self.write_method_selection(stream, AuthMethod::NoAuth)
            .await?;
        self.read_request(stream).await
    }

    pub async fn send_reply<S>(&self, stream: &mut S, reply: ReplyCode) -> std::io::Result<()>
    where
        S: AsyncWrite + Unpin,
    {
        self.send_response(
            stream,
            Response {
                reply,
                bind: failure_bind_addr(),
            },
        )
        .await
    }

    pub async fn send_success_reply<S>(
        &self,
        stream: &mut S,
        bind: TargetEndpoint,
    ) -> std::io::Result<()>
    where
        S: AsyncWrite + Unpin,
    {
        self.send_response(
            stream,
            Response {
                reply: ReplyCode::Succeeded,
                bind,
            },
        )
        .await
    }

    async fn send_response<S>(&self, stream: &mut S, response: Response) -> std::io::Result<()>
    where
        S: AsyncWrite + Unpin,
    {
        stream.write_all(&encode_response(&response)).await?;
        stream.flush().await
    }

    async fn read_greeting<S>(
        &self,
        stream: &mut S,
    ) -> Result<crate::protocol::socks5::Greeting, Socks5HandshakeError>
    where
        S: AsyncRead + Unpin,
    {
        let mut header = [0u8; 2];
        stream.read_exact(&mut header).await?;

        let methods_len = header[1] as usize;
        let mut frame = vec![0u8; 2 + methods_len];
        frame[..2].copy_from_slice(&header);
        stream.read_exact(&mut frame[2..]).await?;

        let (greeting, _) = parse_greeting(&frame)?;
        Ok(greeting)
    }

    async fn read_request<S>(&self, stream: &mut S) -> Result<Request, Socks5HandshakeError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await?;

        let mut frame = header.to_vec();

        match header[3] {
            0x01 => {
                let mut remainder = [0u8; 6];
                stream.read_exact(&mut remainder).await?;
                frame.extend_from_slice(&remainder);
            }
            0x03 => {
                let mut domain_len = [0u8; 1];
                stream.read_exact(&mut domain_len).await?;
                frame.extend_from_slice(&domain_len);

                let mut remainder = vec![0u8; domain_len[0] as usize + 2];
                stream.read_exact(&mut remainder).await?;
                frame.extend_from_slice(&remainder);
            }
            0x04 => {
                let mut remainder = [0u8; 18];
                stream.read_exact(&mut remainder).await?;
                frame.extend_from_slice(&remainder);
            }
            _ => {}
        }

        match parse_request(&frame) {
            Ok((request, _)) => Ok(request),
            Err(error) => {
                if let Some(reply) = reply_code_for_error(&error) {
                    self.send_reply(stream, reply).await?;
                }

                Err(error.into())
            }
        }
    }

    async fn write_method_selection<S>(
        &self,
        stream: &mut S,
        method: AuthMethod,
    ) -> std::io::Result<()>
    where
        S: AsyncWrite + Unpin,
    {
        let frame = encode_method_selection(MethodSelection { method });
        stream.write_all(&frame).await?;
        stream.flush().await
    }
}

fn failure_bind_addr() -> TargetEndpoint {
    TargetEndpoint {
        address: TargetAddr::Ipv4(Ipv4Addr::UNSPECIFIED),
        port: 0,
    }
}

fn reply_code_for_error(error: &Socks5Error) -> Option<ReplyCode> {
    match error {
        Socks5Error::UnsupportedCommand(_) => Some(ReplyCode::CommandNotSupported),
        Socks5Error::UnsupportedAddressType(_) => Some(ReplyCode::AddressTypeNotSupported),
        Socks5Error::InvalidReservedByte(_)
        | Socks5Error::InvalidDomainLength
        | Socks5Error::InvalidDomainName
        | Socks5Error::Truncated { .. } => Some(ReplyCode::GeneralFailure),
        Socks5Error::InvalidVersion(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::Socks5Handler;
    use crate::protocol::socks5::error::{Socks5Error, Socks5HandshakeError};
    use crate::protocol::socks5::{Command, Request, TargetAddr, TargetEndpoint};

    #[tokio::test]
    async fn accepts_no_auth_connect_request_and_extracts_target() {
        let (mut client, mut server) = tokio::io::duplex(64);

        let server_task =
            tokio::spawn(async move { Socks5Handler.accept_connect(&mut server).await });

        client
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .expect("write greeting");

        let mut selection = [0u8; 2];
        client
            .read_exact(&mut selection)
            .await
            .expect("read no-auth selection");
        assert_eq!(selection, [0x05, 0x00]);

        client
            .write_all(&[0x05, 0x01, 0x00, 0x01, 10, 0, 0, 7, 0x01, 0xbb])
            .await
            .expect("write connect request");

        let request = server_task
            .await
            .expect("server task should join")
            .expect("request should parse");

        assert_eq!(
            request,
            Request {
                command: Command::Connect,
                destination: TargetEndpoint {
                    address: TargetAddr::Ipv4(Ipv4Addr::new(10, 0, 0, 7)),
                    port: 443,
                },
            }
        );
    }

    #[tokio::test]
    async fn rejects_missing_no_auth_method_with_selection_failure() {
        let (mut client, mut server) = tokio::io::duplex(64);

        let server_task =
            tokio::spawn(async move { Socks5Handler.accept_connect(&mut server).await });

        client
            .write_all(&[0x05, 0x01, 0x02])
            .await
            .expect("write unsupported auth methods");

        let mut selection = [0u8; 2];
        client
            .read_exact(&mut selection)
            .await
            .expect("read failure selection");
        assert_eq!(selection, [0x05, 0xff]);

        let error = server_task
            .await
            .expect("server task should join")
            .expect_err("handshake should reject unsupported auth methods");

        assert!(matches!(
            error,
            Socks5HandshakeError::NoAcceptableAuthMethod
        ));
    }

    #[tokio::test]
    async fn rejects_unsupported_command_with_protocol_reply() {
        let (mut client, mut server) = tokio::io::duplex(64);

        let server_task =
            tokio::spawn(async move { Socks5Handler.accept_connect(&mut server).await });

        client
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .expect("write greeting");

        let mut selection = [0u8; 2];
        client
            .read_exact(&mut selection)
            .await
            .expect("read no-auth selection");
        assert_eq!(selection, [0x05, 0x00]);

        client
            .write_all(&[0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x35])
            .await
            .expect("write unsupported command request");

        let mut response = [0u8; 10];
        client
            .read_exact(&mut response)
            .await
            .expect("read failure response");
        assert_eq!(response, [0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);

        let error = server_task
            .await
            .expect("server task should join")
            .expect_err("unsupported command should fail");

        assert!(matches!(
            error,
            Socks5HandshakeError::Protocol(Socks5Error::UnsupportedCommand(0x02))
        ));
    }
}
