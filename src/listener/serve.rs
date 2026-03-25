use std::net::SocketAddr;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::task::{JoinError, JoinHandle};

use crate::error::Error;
use crate::listener::{
    AdmissionControl, BoundListener, ListenerPlan, ListenerRegistry, PreparedListener,
    bind_prepared_listener, bind_registry,
};
use crate::logging::{default_log_event, emit_log_event};
use crate::session::{SessionContext, SessionPlan, drive_session};

#[derive(Debug)]
pub struct ListenerTaskHandle {
    plan: ListenerPlan,
    local_addr: SocketAddr,
    task: JoinHandle<Result<(), Error>>,
}

impl ListenerTaskHandle {
    pub(crate) fn new(
        plan: ListenerPlan,
        local_addr: SocketAddr,
        task: JoinHandle<Result<(), Error>>,
    ) -> Self {
        Self { plan, local_addr, task }
    }

    pub fn plan(&self) -> &ListenerPlan {
        &self.plan
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn abort(&self) {
        self.task.abort();
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
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

    Ok(ListenerTaskHandle::new(plan, local_addr, task))
}

pub async fn spawn_registry_accept_loops(
    registry: &ListenerRegistry,
    admission: &AdmissionControl,
    session_plan: SessionPlan,
) -> Result<Vec<ListenerTaskHandle>, Error> {
    let bound_listeners = bind_registry(registry).await?;
    let mut handles = Vec::with_capacity(bound_listeners.len());

    for listener in bound_listeners {
        let local_addr = listener.local_addr();
        let plan = listener.plan().clone();
        let task = tokio::spawn(run_accept_loop(listener, admission.clone(), session_plan));

        handles.push(ListenerTaskHandle::new(plan, local_addr, task));
    }

    Ok(handles)
}

pub async fn run_accept_loop(
    listener: BoundListener,
    admission: AdmissionControl,
    session_plan: SessionPlan,
) -> Result<(), Error> {
    loop {
        let (stream, peer_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) if is_transient_accept_error(&error) => continue,
            Err(error) => {
                return Err(Error::io(format!(
                    "listener '{}' ({:?}) on '{}' failed while accepting downstream connections: {error}",
                    listener.plan().name,
                    listener.plan().protocol,
                    listener.local_addr()
                )));
            }
        };
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
                emit_session_closed(&plan, "rejected_capacity");
                let _ = stream.shutdown().await;
                return;
            }
        };

        let _guard = guard;
        let context = SessionContext::from_listener_plan(&plan, peer_addr, local_addr);
        let result = drive_session(&mut stream, context, session_plan).await;
        let result_label = match &result {
            Ok(_) => "ok",
            Err(error) => error.result_label(),
        };
        emit_session_closed(&plan, result_label);
        let _ = stream.shutdown().await;
    });
}

fn emit_session_closed(plan: &ListenerPlan, result: &str) {
    if let Some(event) = default_log_event("session.closed") {
        emit_log_event(
            event,
            &[
                ("listener", plan.name.clone()),
                ("protocol", format!("{:?}", plan.protocol)),
                ("result", result.to_string()),
            ],
        );
    }
}

fn is_transient_accept_error(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::ConnectionAborted)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use hex::decode as hex_decode;
    use native_tls::Identity;
    use sha2::{Digest, Sha224};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::timeout;
    use tokio_native_tls::TlsAcceptor;

    use super::{spawn_prepared_listener, spawn_registry_accept_loops};
    use crate::config::external::{
        ExternalConfig, GroupInput, GroupStrategyInput, ListenerInput, ListenerProtocolInput,
        NodeInput, NodeKindInput, TargetRefInput,
    };
    use crate::config::internal::{
        ConfigOrigin, Limits, NodeConfig, NodeKind, ProtocolKind, TargetRef, TrojanNodeConfig,
    };
    use crate::error::Error;
    use crate::listener::{AdmissionControl, ListenerPlan, ListenerRegistry, prepare_listener};
    use crate::session::SessionPlan;

    const TROJAN_TEST_PASSWORD: &str = "secret";
    const TROJAN_TEST_IDENTITY_P12_HEX: &str = "308209f7020103308209a506092a864886f70d010701a0820996048209923082098e308203fa06092a864886f70d010706a08203eb308203e7020100308203e006092a864886f70d010701305f06092a864886f70d01050d3052303106092a864886f70d01050c302404106c5a1f0a5626a025d5dd0c365511b9ce02020800300c06082a864886f70d02090500301d060960864801650304012a0410be278b28d00ea78e67f7441d55c251af808203709d24c961a4b21ca3578700d58e43cf85bc613d0a0bd3fabfff0305423a4c213633b40e073d653738e268f23b527d70ef5b8e2a89dc693a761c5dc7bfd1ee1329bcc50156aff6d37b105c312e2e09b5541cf89b94b92fc641a401f13200cfd866e5660bb00892835c2906d4bd8f189b20b6cfd1967d178fd39b7029e5f2a855b93afd3ea8eb02892560996660b14ac7c61bafc67b9c35390fc6cd5ff9f1a9d855c86669004d606860bbf32ee3f7bd384225064ad178ec8469d172bde3be966f51f66a016e35fcf9ca8e17c18c40cab2ec2a4289d8ca0f1c9705743b08bad12e2eb12418f501ec288a6e480687c6d89f005feb19f6c75108b7dfb4d5d4a5381c272f0686f886726d7c054087299e037313232c5876751f99f0f55a7becf109caaf5c1eb4b63199951d315386322699c3f88a405a02ab7ca5c34f6adc1de8b4a0faa4c0b4a8195b932b1c6bb3fd03e465a2ba8e8eb38d39300f48b446f301469a73d96b81161b9444c2a65a507515f03363e4343f3e72383a3c94b155456f2b4bd86e4187c913cf56e1d11d408aacc367052c4ce9c1be47d3e2a23310ca64ca6a35abcd17568b97dc70ab4542c85ecde80f31bbb0f8eccea6081f78acebb748bcc2b2295356a2e247e617336be6fbdda9a25bac030ec0e9acf87866b16c4c75978d847541ac84560b785fb76087be9f80a08d748a22262d6818666d09d289a22dc16f3cccc25a1a60acb8c185fe80b4d088ce454b1c93cf4e2d3b65cd9f97b9a59005f25702b5820ba43233f62e750fde06d8bd2935e409a98f2a099a80a9406b05443d02c9272e965ff6c6e447c98d92ca7e59e6dfdec4f820fff5bbbaa3de4da8ca0f3f487a74c0f18007eb4d1bc83ee2d4cd70fbf07409f6cf6d446c38557b1796a426328eedcd9039d9d9bdbdb5fe3b3919408a5468cd597108319b19765e1ccd66f33d2eee0a6dec9b19bb52c0371e581af8b3a89e88da8f402129804f2de71c50884ba31d107151d44882bcf5a0ad3cc03badaf850fdb04f328ff885f4ffc8030b919e764fbce747ace199f3bc6bb236adcad3f25228188db75ddd1ea16ddfec9751f72004930b89d216aae911f88a17f4d83d1a3448e99cfa5b5af6db4f25b73d2bc56c7701d595dbdf9be5b38d0a94937b3748744ec156395ddd5dcb67d5168c541bf34696538d47580243f00067862bdd7f079293f39079462a5bd37031d84d3a9b5a840ad42cd71bcf665c08d3082058c06092a864886f70d010701a082057d048205793082057530820571060b2a864886f70d010c0a0102a082053930820535305f06092a864886f70d01050d3052303106092a864886f70d01050c302404103329cf0e9ce96d4b577c985c3229030402020800300c06082a864886f70d02090500301d060960864801650304012a0410ee7b06fa25e886426faa4f08dd4496f1048204d05eac14b77a7a874038fe039a456eb3cc2a6ceea0b1eaa33e4dd38564a09a5be8f54c7d2ccb131ed1d6a7ef548019d9f9108b4da116ce44cea513e12c9905747b213ef13aee5b0ebdf1b31f2af8e8b97dbdf584aa37ea532ce8a65f9c27628c11fd6bbc5f81c549564eea076cd435ec0330779505501d5e6e385e087a17725e68d7ef8b31f3eacf8ad86dd05b7b104689984c01152109826ab34bc572e390fe1fcebf473bd14344efcf92eb2f4295117513bbf16f3c7db5d877356460e003041c396dbb62f9ac3333c90b3a958eb2719f795226afdef2178eb212e13d56832369b536e47ebe14dcf27b3292333f318d68aeae544ccc384de6d22a38ca6189de7dc4ffaee20287cd74c9184a39dcabb7faa5c487df08bc650fb01dde630d85ec1203c48304d550a14da5dbae98b2631327cb433abde1368e976d567cf5454ad2fb3dd61911fadc760a8744a02a18d5891be3d0f210975d243aae3d1d98b564d21d614224cc0381e2330a43eb994cb254400fa164ec38737c7b702560dfe1aca80915f8486aa5ee7b6b292676f76cd418fd217251f9bed739e4ec0c00d660271403ceafb5800201f0196ff1f20680ea22e61d29c775a7ea146b2c902dce2d89315b94e17b952d3798a7b2361dd73927acae2e40c7350d6b0767b95e70aea2883a31c65c7f6bb74dc004c70d9af35cb0b61963ab44ef93dc40b6290fa477e254dc4f08a72866974056f7ae4c562c00d9d158f3fe1b677e15cac5a840958db95dab6a2308fb2614bcc066cbec8c28425760373613af9526844fb46fab1cd55d373418848f1d7df74a60717ad819951a9a535c098c380f14ca5bec6533010e0f44003ebef75252ed8b726cd9701216eb0fa29037954593bd8d69d1f59355f050703f8137d5373eaf2e2ab95e49fe6930e5f1fa6f8f4b59d3d45a5a428f7704b2664e8334ddacd972ae3b04944196f3f14a103ddc990465abf14ffe06ffda998bc8ffc6f5155d7a3b7a33e27b3095f0d13dc04266ad6abfd43ee90c81bf0233c618120d876304ca5bb4206a148bcfe64228a3bc4938bec30d3fd62f0973d2095c02d1c0f06d71a9291cf042ce83643f36b5b51e32319eb364c9dc49e68afe97ab72d1d0ba67bbd7d7e6c617f53e520333558ce3280648290ef0aa07e4ca6d40aa8eb98861bd40a0cb07d23507ada5919c4fdc7f22de577e940541588f647af8d9cc28b10f1b4338caf9d2cb39b5a0739f039e9600cd7688ea7afd356882cc88dc345fa1149dbd37a5fa747a6d47b4d276f56f583b634169c2badfc81999b2e1b6ae38704a645b18bf6e7098547b8d0915f73d6b7931d2ef0b530f2238311a39fbe1cba3239bbe10ecb5803c78ab2c2f86630165f37f87dfa3c1d3a4d4f7a6988da398e4c59358fc5deb2e4019c10c3082073302005bda6fa9ca3a5176f40aa4f20eae5770569302eb8e8e56d541ca9f6e6914473d9a97982389bc61811c6233106e97f4d8f62d86778a481f48e06e2fe73ba076d0ea34191c69947452785a1e02b1302a189bd14454b011698418af1865127402b09b8126f6cdc1e2ac463503857ef3b47df5172acddec44b6b3ed0862b9d2e237ae2462cd1f55a93199fba6219b7c9531cbbf5c7f36171a6947ac0a7c78d4f7ad4b9d6ca2d7d81d566c98095254f43c7b0adbecf6886103364f3fafcb5cec409784d4d482651bd5e98179a172350233187219fd412a3b197a1ca94cbf2cfe4663125302306092a864886f70d010915311604143c71d4523098273f7a5ff55545a1b19cf78335d230493031300d060960864801650304020105000420b1742be17d6b142aa1636472ace347ddd367b7e512615203240fb368da93eb950410e47ab021316c3f8754c405f71359d8de02020800";

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
            resolved_target: test_listener_node(),
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

        let mut client =
            TcpStream::connect(handle.local_addr()).await.expect("client should connect");

        client.write_all(&[0x05, 0x01, 0x00]).await.expect("write greeting");

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

        client.write_all(b"ping through proxy").await.expect("write proxied bytes");

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
        let join = handle.join().await.expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn accept_loop_returns_connection_refused_for_failed_upstream_dial() {
        let listener = prepare_listener(ListenerPlan {
            name: "local-socks".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::Socks5,
            target: TargetRef::Node("node-a".to_string()),
            resolved_target: test_listener_node(),
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

        let mut client =
            TcpStream::connect(handle.local_addr()).await.expect("client should connect");

        client.write_all(&[0x05, 0x01, 0x00]).await.expect("write greeting");

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
        let join = handle.join().await.expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn accept_loop_relays_successful_http_connect_session() {
        let (target_addr, target_task) = match spawn_echo_server().await {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("echo listener should bind: {error}"),
        };
        let listener = prepare_listener(ListenerPlan {
            name: "local-connect".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::HttpConnect,
            target: TargetRef::Node("node-a".to_string()),
            resolved_target: test_listener_node(),
            handler: crate::listener::ListenerHandler::HttpConnect,
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

        let mut client =
            TcpStream::connect(handle.local_addr()).await.expect("client should connect");
        let payload = b"ping through proxy";

        client
            .write_all(
                format!(
                    "CONNECT 127.0.0.1:{} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                    target_addr.port(),
                    target_addr.port()
                )
                .as_bytes(),
            )
            .await
            .expect("write connect request");
        client.write_all(payload).await.expect("write pipelined tunnel bytes");

        let expected_response = b"HTTP/1.1 200 Connection Established\r\n\r\n";
        let mut response = vec![0u8; expected_response.len()];
        timeout(Duration::from_secs(1), client.read_exact(&mut response))
            .await
            .expect("success response should arrive")
            .expect("success response should read");
        assert_eq!(&response, expected_response);

        let mut echoed = vec![0u8; payload.len()];
        timeout(Duration::from_secs(1), client.read_exact(&mut echoed))
            .await
            .expect("echoed bytes should arrive")
            .expect("echoed bytes should read");
        assert_eq!(&echoed, payload);

        client.shutdown().await.expect("client should shut down");
        timeout(Duration::from_secs(1), target_task)
            .await
            .expect("echo task should finish")
            .expect("echo task should join")
            .expect("echo task should succeed");

        handle.abort();
        let join = handle.join().await.expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn accept_loop_returns_bad_gateway_for_failed_http_connect_upstream_dial() {
        let listener = prepare_listener(ListenerPlan {
            name: "local-connect".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::HttpConnect,
            target: TargetRef::Node("node-a".to_string()),
            resolved_target: test_listener_node(),
            handler: crate::listener::ListenerHandler::HttpConnect,
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

        let mut client =
            TcpStream::connect(handle.local_addr()).await.expect("client should connect");
        let refused_port = closed_local_port().await;

        client
            .write_all(
                format!(
                    "CONNECT 127.0.0.1:{} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                    refused_port, refused_port
                )
                .as_bytes(),
            )
            .await
            .expect("write connect request");

        let expected_response =
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let mut response = vec![0u8; expected_response.len()];
        timeout(Duration::from_secs(1), client.read_exact(&mut response))
            .await
            .expect("failure response should arrive")
            .expect("failure response should read");
        assert_eq!(&response, expected_response);

        handle.abort();
        let join = handle.join().await.expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn accept_loop_relays_successful_socks5_connect_session_through_trojan() {
        let (target_addr, target_task) = match spawn_echo_server().await {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("echo listener should bind: {error}"),
        };
        let (trojan_addr, trojan_task) =
            match spawn_trojan_mock_server(target_addr, TROJAN_TEST_PASSWORD).await {
                Ok(server) => server,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
                Err(error) => panic!("trojan listener should bind: {error}"),
            };
        let listener = prepare_listener(ListenerPlan {
            name: "local-socks".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::Socks5,
            target: TargetRef::Group("entry".to_string()),
            resolved_target: trojan_listener_node(trojan_addr, TROJAN_TEST_PASSWORD),
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

        let mut client =
            TcpStream::connect(handle.local_addr()).await.expect("client should connect");

        client.write_all(&[0x05, 0x01, 0x00]).await.expect("write greeting");

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

        client.write_all(b"ping through proxy").await.expect("write proxied bytes");

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
        timeout(Duration::from_secs(1), trojan_task)
            .await
            .expect("trojan task should finish")
            .expect("trojan task should join")
            .expect("trojan task should succeed");

        handle.abort();
        let join = handle.join().await.expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn accept_loop_relays_successful_http_connect_session_through_trojan() {
        let (target_addr, target_task) = match spawn_echo_server().await {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("echo listener should bind: {error}"),
        };
        let (trojan_addr, trojan_task) =
            match spawn_trojan_mock_server(target_addr, TROJAN_TEST_PASSWORD).await {
                Ok(server) => server,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
                Err(error) => panic!("trojan listener should bind: {error}"),
            };
        let listener = prepare_listener(ListenerPlan {
            name: "local-connect".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::HttpConnect,
            target: TargetRef::Group("entry".to_string()),
            resolved_target: trojan_listener_node(trojan_addr, TROJAN_TEST_PASSWORD),
            handler: crate::listener::ListenerHandler::HttpConnect,
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

        let mut client =
            TcpStream::connect(handle.local_addr()).await.expect("client should connect");
        let payload = b"ping through proxy";

        client
            .write_all(
                format!(
                    "CONNECT 127.0.0.1:{} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                    target_addr.port(),
                    target_addr.port()
                )
                .as_bytes(),
            )
            .await
            .expect("write connect request");

        let expected_response = b"HTTP/1.1 200 Connection Established\r\n\r\n";
        let mut response = vec![0u8; expected_response.len()];
        timeout(Duration::from_secs(1), client.read_exact(&mut response))
            .await
            .expect("success response should arrive")
            .expect("success response should read");
        assert_eq!(&response, expected_response);

        client.write_all(payload).await.expect("write pipelined tunnel bytes");

        let mut echoed = vec![0u8; payload.len()];
        timeout(Duration::from_secs(1), client.read_exact(&mut echoed))
            .await
            .expect("echoed bytes should arrive")
            .expect("echoed bytes should read");
        assert_eq!(&echoed, payload);

        client.shutdown().await.expect("client should shut down");
        timeout(Duration::from_secs(1), target_task)
            .await
            .expect("echo task should finish")
            .expect("echo task should join")
            .expect("echo task should succeed");
        timeout(Duration::from_secs(1), trojan_task)
            .await
            .expect("trojan task should finish")
            .expect("trojan task should join")
            .expect("trojan task should succeed");

        handle.abort();
        let join = handle.join().await.expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn accept_loop_returns_general_failure_for_trojan_tls_failure() {
        let (trojan_addr, trojan_task) = match spawn_plaintext_trojan_server().await {
            Ok(server) => server,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("plaintext trojan listener should bind: {error}"),
        };
        let listener = prepare_listener(ListenerPlan {
            name: "local-socks".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::Socks5,
            target: TargetRef::Group("entry".to_string()),
            resolved_target: trojan_listener_node(trojan_addr, TROJAN_TEST_PASSWORD),
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

        let mut client =
            TcpStream::connect(handle.local_addr()).await.expect("client should connect");

        client.write_all(&[0x05, 0x01, 0x00]).await.expect("write greeting");

        let mut selection = [0u8; 2];
        timeout(Duration::from_secs(1), client.read_exact(&mut selection))
            .await
            .expect("selection should arrive")
            .expect("selection should read");
        assert_eq!(selection, [0x05, 0x00]);

        client
            .write_all(&[0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90])
            .await
            .expect("write connect request");

        let mut response = [0u8; 10];
        timeout(Duration::from_secs(1), client.read_exact(&mut response))
            .await
            .expect("failure response should arrive")
            .expect("failure response should read");
        assert_eq!(response, [0x05, 0x01, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);

        timeout(Duration::from_secs(1), trojan_task)
            .await
            .expect("plaintext trojan task should finish")
            .expect("plaintext trojan task should join")
            .expect("plaintext trojan task should succeed");

        handle.abort();
        let join = handle.join().await.expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn accept_loop_returns_general_failure_for_trojan_password_mismatch() {
        let (trojan_addr, trojan_task) =
            match spawn_trojan_password_rejecting_server(TROJAN_TEST_PASSWORD).await {
                Ok(server) => server,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
                Err(error) => panic!("trojan listener should bind: {error}"),
            };
        let listener = prepare_listener(ListenerPlan {
            name: "local-socks".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::Socks5,
            target: TargetRef::Group("entry".to_string()),
            resolved_target: trojan_listener_node(trojan_addr, "wrong-password"),
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

        let mut client =
            TcpStream::connect(handle.local_addr()).await.expect("client should connect");

        client.write_all(&[0x05, 0x01, 0x00]).await.expect("write greeting");

        let mut selection = [0u8; 2];
        timeout(Duration::from_secs(1), client.read_exact(&mut selection))
            .await
            .expect("selection should arrive")
            .expect("selection should read");
        assert_eq!(selection, [0x05, 0x00]);

        client
            .write_all(&[0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1, 0x00, 0x50])
            .await
            .expect("write connect request");

        let mut response = [0u8; 10];
        timeout(Duration::from_secs(1), client.read_exact(&mut response))
            .await
            .expect("failure response should arrive")
            .expect("failure response should read");
        assert_eq!(response, [0x05, 0x01, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);

        timeout(Duration::from_secs(1), trojan_task)
            .await
            .expect("trojan task should finish")
            .expect("trojan task should join")
            .expect("trojan task should succeed");

        handle.abort();
        let join = handle.join().await.expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn accept_loop_returns_bad_gateway_for_trojan_handshake_failure_after_tls() {
        let (trojan_addr, trojan_task) =
            match spawn_trojan_protocol_failure_server(TROJAN_TEST_PASSWORD).await {
                Ok(server) => server,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
                Err(error) => panic!("trojan listener should bind: {error}"),
            };
        let listener = prepare_listener(ListenerPlan {
            name: "local-connect".to_string(),
            bind: "127.0.0.1:0".to_string(),
            protocol: ProtocolKind::HttpConnect,
            target: TargetRef::Group("entry".to_string()),
            resolved_target: trojan_listener_node(trojan_addr, TROJAN_TEST_PASSWORD),
            handler: crate::listener::ListenerHandler::HttpConnect,
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

        let mut client =
            TcpStream::connect(handle.local_addr()).await.expect("client should connect");

        client
            .write_all(b"CONNECT 127.0.0.1:80 HTTP/1.1\r\nHost: 127.0.0.1:80\r\n\r\n")
            .await
            .expect("write connect request");

        let expected_response =
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let mut response = vec![0u8; expected_response.len()];
        timeout(Duration::from_secs(1), client.read_exact(&mut response))
            .await
            .expect("failure response should arrive")
            .expect("failure response should read");
        assert_eq!(&response, expected_response);

        timeout(Duration::from_secs(1), trojan_task)
            .await
            .expect("trojan task should finish")
            .expect("trojan task should join")
            .expect("trojan task should succeed");

        handle.abort();
        let join = handle.join().await.expect_err("accept loop should be cancelled");
        assert!(join.is_cancelled());
    }

    #[tokio::test]
    async fn registry_spawn_releases_earlier_bindings_when_a_later_listener_fails() {
        let Some(first_port) = available_local_port().await else {
            return;
        };
        let occupied_listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("occupied listener should bind: {error}"),
        };
        let occupied_port =
            occupied_listener.local_addr().expect("occupied listener should expose addr").port();

        let active_config = crate::config::internal::ActiveConfig::from_external(ExternalConfig {
            listeners: vec![
                ListenerInput {
                    name: "local-socks".to_string(),
                    bind: format!("127.0.0.1:{first_port}"),
                    protocol: ListenerProtocolInput::Socks5,
                    target: TargetRefInput::node("node-a"),
                },
                ListenerInput {
                    name: "local-connect".to_string(),
                    bind: format!("127.0.0.1:{occupied_port}"),
                    protocol: ListenerProtocolInput::HttpConnect,
                    target: TargetRefInput::group("group-a"),
                },
            ],
            nodes: vec![
                NodeInput {
                    name: "node-a".to_string(),
                    kind: NodeKindInput::DirectTcp,
                    address: None,
                    server: None,
                    port: None,
                    password: None,
                    sni: None,
                    skip_cert_verify: false,
                    provider: None,
                    subscription: None,
                },
                NodeInput {
                    name: "node-b".to_string(),
                    kind: NodeKindInput::DirectTcp,
                    address: None,
                    server: None,
                    port: None,
                    password: None,
                    sni: None,
                    skip_cert_verify: false,
                    provider: None,
                    subscription: None,
                },
            ],
            groups: vec![GroupInput {
                name: "group-a".to_string(),
                strategy: GroupStrategyInput::Fallback,
                members: vec![TargetRefInput::node("node-b")],
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        })
        .expect("serve registry test config should normalize");
        let registry =
            ListenerRegistry::from_active_config(&active_config, &AdmissionControl::new(4));

        let error = spawn_registry_accept_loops(
            &registry,
            &AdmissionControl::new(4),
            SessionPlan::from_limits(&Limits::default()),
        )
        .await
        .expect_err("occupied later listener should fail startup");

        assert!(matches!(error, Error::Io(_)));

        drop(occupied_listener);
        let rebound = TcpListener::bind(format!("127.0.0.1:{first_port}"))
            .await
            .expect("earlier successful bind should be released on failure");
        drop(rebound);
    }

    fn trojan_listener_node(server: std::net::SocketAddr, password: &str) -> NodeConfig {
        NodeConfig {
            name: format!("trojan-{}", server.port()),
            kind: NodeKind::Trojan,
            trojan: Some(TrojanNodeConfig {
                server: server.ip().to_string(),
                port: server.port(),
                password: password.to_string(),
                sni: Some("localhost".to_string()),
                skip_cert_verify: true,
            }),
            origin: ConfigOrigin::Inline,
        }
    }

    fn test_listener_node() -> NodeConfig {
        NodeConfig {
            name: "node-a".to_string(),
            kind: NodeKind::DirectTcp,
            trojan: None,
            origin: ConfigOrigin::Inline,
        }
    }

    fn trojan_identity() -> Identity {
        let pkcs12 = hex_decode(TROJAN_TEST_IDENTITY_P12_HEX)
            .expect("embedded trojan identity should decode");
        Identity::from_pkcs12(&pkcs12, "password").expect("embedded trojan identity should load")
    }

    fn trojan_password_line(password: &str) -> Vec<u8> {
        let digest = Sha224::digest(password.as_bytes());
        let mut line = hex::encode(digest).into_bytes();
        line.extend_from_slice(b"\r\n");
        line
    }

    async fn spawn_trojan_mock_server(
        expected_target: std::net::SocketAddr,
        expected_password: &str,
    ) -> std::io::Result<(std::net::SocketAddr, tokio::task::JoinHandle<std::io::Result<()>>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(
            native_tls::TlsAcceptor::new(trojan_identity())
                .map_err(|error| std::io::Error::other(error.to_string()))?,
        );
        let expected_password_line = trojan_password_line(expected_password);
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let mut stream = acceptor
                .accept(stream)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;

            let password_line = read_crlf_line(&mut stream).await?;
            assert_eq!(password_line, expected_password_line);

            let destination = read_trojan_destination(&mut stream).await?;
            assert_eq!(destination, expected_target);

            let mut upstream = TcpStream::connect(destination).await?;
            copy_bidirectional(&mut stream, &mut upstream)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;

            Ok(())
        });

        Ok((addr, task))
    }

    async fn spawn_trojan_password_rejecting_server(
        expected_password: &str,
    ) -> std::io::Result<(std::net::SocketAddr, tokio::task::JoinHandle<std::io::Result<()>>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(
            native_tls::TlsAcceptor::new(trojan_identity())
                .map_err(|error| std::io::Error::other(error.to_string()))?,
        );
        let expected_password_line = trojan_password_line(expected_password);
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let mut stream = acceptor
                .accept(stream)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;

            let password_line = read_crlf_line(&mut stream).await?;
            if password_line == expected_password_line {
                return Err(std::io::Error::other("trojan test expected a password mismatch"));
            }

            stream.shutdown().await?;
            Ok(())
        });

        Ok((addr, task))
    }

    async fn spawn_trojan_protocol_failure_server(
        expected_password: &str,
    ) -> std::io::Result<(std::net::SocketAddr, tokio::task::JoinHandle<std::io::Result<()>>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(
            native_tls::TlsAcceptor::new(trojan_identity())
                .map_err(|error| std::io::Error::other(error.to_string()))?,
        );
        let expected_password_line = trojan_password_line(expected_password);
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let mut stream = acceptor
                .accept(stream)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;

            let password_line = read_crlf_line(&mut stream).await?;
            assert_eq!(password_line, expected_password_line);

            stream.shutdown().await?;
            Ok(())
        });

        Ok((addr, task))
    }

    async fn spawn_plaintext_trojan_server()
    -> std::io::Result<(std::net::SocketAddr, tokio::task::JoinHandle<std::io::Result<()>>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            stream.write_all(b"not tls").await?;
            stream.shutdown().await?;
            Ok(())
        });

        Ok((addr, task))
    }

    async fn read_crlf_line<S>(stream: &mut S) -> std::io::Result<Vec<u8>>
    where
        S: tokio::io::AsyncRead + Unpin,
    {
        let mut buf = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            stream.read_exact(&mut byte).await?;
            buf.push(byte[0]);
            if buf.len() >= 2 && &buf[buf.len() - 2..] == b"\r\n" {
                return Ok(buf);
            }
        }
    }

    async fn read_trojan_destination<S>(stream: &mut S) -> std::io::Result<std::net::SocketAddr>
    where
        S: tokio::io::AsyncRead + Unpin,
    {
        let mut command = [0u8; 1];
        stream.read_exact(&mut command).await?;
        assert_eq!(command[0], 0x01);

        let mut atyp = [0u8; 1];
        stream.read_exact(&mut atyp).await?;
        assert_eq!(atyp[0], 0x01);

        let mut address = [0u8; 4];
        stream.read_exact(&mut address).await?;
        let mut port = [0u8; 2];
        stream.read_exact(&mut port).await?;
        let mut crlf = [0u8; 2];
        stream.read_exact(&mut crlf).await?;
        assert_eq!(&crlf, b"\r\n");

        Ok(std::net::SocketAddr::from((
            std::net::Ipv4Addr::from(address),
            u16::from_be_bytes(port),
        )))
    }

    async fn spawn_echo_server()
    -> std::io::Result<(std::net::SocketAddr, tokio::task::JoinHandle<std::io::Result<()>>)> {
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
        let listener =
            TcpListener::bind("127.0.0.1:0").await.expect("temporary listener should bind");
        let port = listener.local_addr().expect("temporary listener should have addr").port();
        drop(listener);
        port
    }

    async fn available_local_port() -> Option<u16> {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(error) => panic!("temporary listener should bind: {error}"),
        };
        let port = listener.local_addr().expect("temporary listener should have addr").port();
        drop(listener);
        Some(port)
    }
}
