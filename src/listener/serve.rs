use std::net::SocketAddr;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::task::{JoinError, JoinHandle};

use crate::error::Error;
use crate::listener::{
    AdmissionControl, BoundListener, ListenerPlan, ListenerRegistry, PreparedListener,
    bind_prepared_listener,
};
use crate::session::{SessionContext, drive_placeholder_connection};

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
) -> Result<ListenerTaskHandle, Error> {
    let bound = bind_prepared_listener(listener).await?;
    let local_addr = bound.local_addr();
    let plan = bound.plan().clone();
    let task = tokio::spawn(run_accept_loop(bound, admission));

    Ok(ListenerTaskHandle {
        plan,
        local_addr,
        task,
    })
}

pub async fn spawn_registry_accept_loops(
    registry: &ListenerRegistry,
    admission: &AdmissionControl,
) -> Result<Vec<ListenerTaskHandle>, Error> {
    let mut handles = Vec::with_capacity(registry.listeners().len());

    for listener in registry.listeners() {
        handles.push(spawn_prepared_listener(listener.clone(), admission.clone()).await?);
    }

    Ok(handles)
}

pub async fn run_accept_loop(
    listener: BoundListener,
    admission: AdmissionControl,
) -> Result<(), Error> {
    loop {
        let (stream, peer_addr) = listener.accept().await?;
        spawn_session_task(
            listener.plan().clone(),
            listener.local_addr(),
            stream,
            peer_addr,
            admission.clone(),
        );
    }
}

fn spawn_session_task(
    plan: ListenerPlan,
    local_addr: SocketAddr,
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    admission: AdmissionControl,
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
        let _ = drive_placeholder_connection(&mut stream, context).await;
        let _ = stream.shutdown().await;
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    use super::spawn_prepared_listener;
    use crate::config::internal::{ProtocolKind, TargetRef};
    use crate::listener::{AdmissionControl, ListenerPlan, prepare_listener};

    #[tokio::test]
    async fn accept_loop_handles_socks5_session_and_returns_placeholder_failure() {
        let listener = prepare_listener(ListenerPlan {
            name: "local-socks".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::Socks5,
            target: TargetRef::Node("node-a".to_string()),
            handler: crate::listener::ListenerHandler::Socks5,
            admission: crate::listener::ListenerAdmissionPlan { shared_limit: 4 },
        });

        let handle = match spawn_prepared_listener(listener, AdmissionControl::new(4)).await {
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

        client
            .write_all(&[
                0x05, 0x01, 0x00, 0x03, 0x0b, b'e', b'x', b'a', b'm', b'p', b'l', b'e', b'.', b'c',
                b'o', b'm', 0x01, 0xbb,
            ])
            .await
            .expect("write connect request");

        let mut response = [0u8; 10];
        timeout(Duration::from_secs(1), client.read_exact(&mut response))
            .await
            .expect("failure response should arrive")
            .expect("failure response should read");
        assert_eq!(response, [0x05, 0x01, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);

        handle.abort();
        let join = handle
            .join()
            .await
            .expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }
}
