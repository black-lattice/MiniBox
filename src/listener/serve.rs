use std::net::SocketAddr;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::task::{JoinError, JoinHandle};

use crate::error::Error;
use crate::listener::{
    AdmissionControl, BoundListener, ListenerPlan, ListenerRegistry, PreparedListener,
    bind_prepared_listener,
};
use crate::session::{SessionContext, SessionPlan, drive_session};

#[derive(Debug)]
pub struct ListenerTaskHandle {
    plan: ListenerPlan,
    local_addr: SocketAddr,
    task: JoinHandle<Result<(), Error>>,
}

impl ListenerTaskHandle {
    pub fn plan(&self) -> &ListenerPlan {
        &self.plan
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn abort(&self) {
        self.task.abort();
    }

    pub async fn join(self) -> Result<Result<(), Error>, JoinError> {
        self.task.await
    }
}

pub async fn spawn_prepared_listener(
    listener: PreparedListener,
    admission: AdmissionControl,
    session_plan: SessionPlan,
) -> Result<ListenerTaskHandle, Error> {
    let bound = bind_prepared_listener(listener).await?;
    let local_addr = bound.local_addr();
    let plan = bound.plan().clone();
    let task = tokio::spawn(run_accept_loop(bound, admission, session_plan));

    Ok(ListenerTaskHandle {
        plan,
        local_addr,
        task,
    })
}

pub async fn spawn_registry_accept_loops(
    registry: &ListenerRegistry,
    admission: &AdmissionControl,
    session_plan: SessionPlan,
) -> Result<Vec<ListenerTaskHandle>, Error> {
    let mut handles = Vec::with_capacity(registry.listeners().len());

    for listener in registry.listeners() {
        handles.push(
            spawn_prepared_listener(listener.clone(), admission.clone(), session_plan).await?,
        );
    }

    Ok(handles)
}

pub async fn run_accept_loop(
    listener: BoundListener,
    admission: AdmissionControl,
    session_plan: SessionPlan,
) -> Result<(), Error> {
    loop {
        let (stream, peer_addr) = listener.accept().await?;
        spawn_session_task(
            listener.plan().clone(),
            listener.local_addr(),
            stream,
            peer_addr,
            admission.clone(),
            session_plan,
        );
    }
}

fn spawn_session_task(
    plan: ListenerPlan,
    local_addr: SocketAddr,
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    admission: AdmissionControl,
    session_plan: SessionPlan,
) {
    tokio::spawn(async move {
        let guard = match admission.try_acquire() {
            Ok(guard) => guard,
            Err(_) => {
                let _ = stream.shutdown().await;
                return;
            }
        };

        let _guard = guard;
        let context = SessionContext::from_listener_plan(&plan, peer_addr, local_addr);
        let _ = drive_session(&mut stream, context, session_plan).await;
        let _ = stream.shutdown().await;
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::timeout;

    use super::spawn_prepared_listener;
    use crate::config::internal::{Limits, ProtocolKind, TargetRef};
    use crate::listener::{AdmissionControl, ListenerPlan, prepare_listener};
    use crate::session::SessionPlan;

    #[tokio::test]
    async fn accept_loop_relays_successful_socks5_connect_session() {
        let (target_addr, target_task) = match spawn_echo_server().await {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("echo listener should bind: {error}"),
        };
        let listener = prepare_listener(ListenerPlan {
            name: "local-socks".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::Socks5,
            target: TargetRef::Node("node-a".to_string()),
            handler: crate::listener::ListenerHandler::Socks5,
            admission: crate::listener::ListenerAdmissionPlan { shared_limit: 4 },
        });

        let handle = match spawn_prepared_listener(
            listener,
            AdmissionControl::new(4),
            SessionPlan::from_limits(&Limits::default()),
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::error::Error::Io(message))
                if message.contains("Operation not permitted") =>
            {
                return;
            }
            Err(error) => panic!("listener should bind: {error}"),
        };

        let mut client = TcpStream::connect(handle.local_addr())
            .await
            .expect("client should connect");

        client
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .expect("write greeting");

        let mut selection = [0u8; 2];
        timeout(Duration::from_secs(1), client.read_exact(&mut selection))
            .await
            .expect("selection should arrive")
            .expect("selection should read");
        assert_eq!(selection, [0x05, 0x00]);

        let target_ip = match target_addr.ip() {
            std::net::IpAddr::V4(ip) => ip.octets(),
            std::net::IpAddr::V6(_) => panic!("test listener should bind IPv4"),
        };

        client
            .write_all(&[
                0x05,
                0x01,
                0x00,
                0x01,
                target_ip[0],
                target_ip[1],
                target_ip[2],
                target_ip[3],
                (target_addr.port() >> 8) as u8,
                target_addr.port() as u8,
            ])
            .await
            .expect("write connect request");

        let mut response = [0u8; 10];
        timeout(Duration::from_secs(1), client.read_exact(&mut response))
            .await
            .expect("success response should arrive")
            .expect("success response should read");
        assert_eq!(response[0], 0x05);
        assert_eq!(response[1], 0x00);
        assert_eq!(response[2], 0x00);
        assert_eq!(response[3], 0x01);

        client
            .write_all(b"ping through proxy")
            .await
            .expect("write proxied bytes");

        let mut echoed = vec![0u8; 18];
        timeout(Duration::from_secs(1), client.read_exact(&mut echoed))
            .await
            .expect("echoed bytes should arrive")
            .expect("echoed bytes should read");
        assert_eq!(&echoed, b"ping through proxy");

        client.shutdown().await.expect("client should shut down");
        timeout(Duration::from_secs(1), target_task)
            .await
            .expect("echo task should finish")
            .expect("echo task should join")
            .expect("echo task should succeed");

        handle.abort();
        let join = handle
            .join()
            .await
            .expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn accept_loop_returns_connection_refused_for_failed_upstream_dial() {
        let listener = prepare_listener(ListenerPlan {
            name: "local-socks".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::Socks5,
            target: TargetRef::Node("node-a".to_string()),
            handler: crate::listener::ListenerHandler::Socks5,
            admission: crate::listener::ListenerAdmissionPlan { shared_limit: 4 },
        });

        let handle = match spawn_prepared_listener(
            listener,
            AdmissionControl::new(4),
            SessionPlan::from_limits(&Limits::default()),
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::error::Error::Io(message))
                if message.contains("Operation not permitted") =>
            {
                return;
            }
            Err(error) => panic!("listener should bind: {error}"),
        };

        let mut client = TcpStream::connect(handle.local_addr())
            .await
            .expect("client should connect");

        client
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .expect("write greeting");

        let mut selection = [0u8; 2];
        timeout(Duration::from_secs(1), client.read_exact(&mut selection))
            .await
            .expect("selection should arrive")
            .expect("selection should read");
        assert_eq!(selection, [0x05, 0x00]);

        let refused_port = closed_local_port().await;

        client
            .write_all(&[
                0x05,
                0x01,
                0x00,
                0x01,
                127,
                0,
                0,
                1,
                (refused_port >> 8) as u8,
                refused_port as u8,
            ])
            .await
            .expect("write connect request");

        let mut response = [0u8; 10];
        timeout(Duration::from_secs(1), client.read_exact(&mut response))
            .await
            .expect("failure response should arrive")
            .expect("failure response should read");
        assert_eq!(response, [0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);

        handle.abort();
        let join = handle
            .join()
            .await
            .expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    async fn spawn_echo_server() -> std::io::Result<(
        std::net::SocketAddr,
        tokio::task::JoinHandle<std::io::Result<()>>,
    )> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut buffer = [0u8; 256];

            loop {
                let read = stream.read(&mut buffer).await?;
                if read == 0 {
                    break;
                }

                stream.write_all(&buffer[..read]).await?;
            }

            Ok(())
        });

        Ok((addr, task))
    }

    async fn closed_local_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("temporary listener should bind");
        let port = listener
            .local_addr()
            .expect("temporary listener should have addr")
            .port();
        drop(listener);
        port
    }
}
