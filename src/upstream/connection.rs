use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_native_tls::TlsStream;

use super::direct::{DialError, DirectDialPlan, dial_tcp};
use super::resolve::{ConnectRoute, ConnectRouteKind};
use super::trojan::{TrojanDialPlan, dial_trojan};

pub enum UpstreamStream {
    Tcp(TcpStream),
    Trojan(TlsStream<TcpStream>),
}

impl AsyncRead for UpstreamStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // Safety: the enum is never moved after pinning; we only project the active variant.
        unsafe {
            match self.get_unchecked_mut() {
                Self::Tcp(stream) => Pin::new_unchecked(stream).poll_read(cx, buf),
                Self::Trojan(stream) => Pin::new_unchecked(stream).poll_read(cx, buf),
            }
        }
    }
}

impl AsyncWrite for UpstreamStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        // Safety: the enum is never moved after pinning; we only project the active variant.
        unsafe {
            match self.get_unchecked_mut() {
                Self::Tcp(stream) => Pin::new_unchecked(stream).poll_write(cx, buf),
                Self::Trojan(stream) => Pin::new_unchecked(stream).poll_write(cx, buf),
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        // Safety: the enum is never moved after pinning; we only project the active variant.
        unsafe {
            match self.get_unchecked_mut() {
                Self::Tcp(stream) => Pin::new_unchecked(stream).poll_flush(cx),
                Self::Trojan(stream) => Pin::new_unchecked(stream).poll_flush(cx),
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        // Safety: the enum is never moved after pinning; we only project the active variant.
        unsafe {
            match self.get_unchecked_mut() {
                Self::Tcp(stream) => Pin::new_unchecked(stream).poll_shutdown(cx),
                Self::Trojan(stream) => Pin::new_unchecked(stream).poll_shutdown(cx),
            }
        }
    }
}

pub struct UpstreamConnection {
    pub stream: UpstreamStream,
    pub bind_addr: SocketAddr,
}

pub async fn connect_upstream(
    route: &ConnectRoute,
    direct_plan: DirectDialPlan,
    trojan_plan: TrojanDialPlan,
) -> Result<UpstreamConnection, DialError> {
    match route.kind {
        ConnectRouteKind::DirectTcp => {
            let (stream, bind_addr) = dial_tcp(&route.connect_target, direct_plan).await?;
            Ok(UpstreamConnection { stream: UpstreamStream::Tcp(stream), bind_addr })
        }
        ConnectRouteKind::Trojan => {
            let trojan = route.trojan.as_ref().expect("trojan route should carry trojan config");
            dial_trojan(
                &route.connect_target,
                &route.destination_target,
                trojan,
                direct_plan,
                trojan_plan,
            )
            .await
        }
    }
}
