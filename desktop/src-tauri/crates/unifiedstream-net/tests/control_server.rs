//! End-to-end control channel tests against a real loopback socket.
//!
//! These drive the server the way the phone will: open TCP, exchange newline-delimited JSON,
//! and assert on what comes back.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::OwnedWriteHalf;

use unifiedstream_net::control::{
    ControlEvent, ControlServer, ServerConfig, TrustStore,
};
use unifiedstream_net::protocol::{ControlMessage, ErrorReason, Hello, PROTOCOL_VERSION};

const TIMEOUT: Duration = Duration::from_secs(5);

fn config(media_port: u16) -> ServerConfig {
    ServerConfig {
        device_id: "desktop-1".to_owned(),
        device_name: "cachy-desktop".to_owned(),
        caps: vec!["cam".to_owned(), "mic".to_owned(), "spk".to_owned()],
        control_port: 0,
        media_port,
    }
}

fn hello() -> Hello {
    Hello {
        version: PROTOCOL_VERSION,
        device_id: "phone-1".to_owned(),
        device_name: "Pixel 8".to_owned(),
        caps: vec!["cam".to_owned(), "mic".to_owned()],
        resume_session_id: None,
        media_port: Some(47_812),
    }
}

struct Client {
    lines: tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
    write: OwnedWriteHalf,
}

impl Client {
    async fn connect(addr: std::net::SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).await.expect("connect");
        let (read, write) = stream.into_split();
        Self {
            lines: BufReader::new(read).lines(),
            write,
        }
    }

    async fn send(&mut self, message: &ControlMessage) {
        let line = message.to_line().expect("encode");
        self.write.write_all(line.as_bytes()).await.expect("write");
        self.write.flush().await.expect("flush");
    }

    async fn send_raw(&mut self, raw: &str) {
        self.write.write_all(raw.as_bytes()).await.expect("write");
        self.write.flush().await.expect("flush");
    }

    /// Next message, or None if the server closed the connection.
    async fn recv(&mut self) -> Option<ControlMessage> {
        let line = tokio::time::timeout(TIMEOUT, self.lines.next_line())
            .await
            .expect("server must answer within the timeout")
            .expect("read")?;
        Some(ControlMessage::from_line(&line).expect("server sent a parseable message"))
    }
}

async fn start(trust: TrustStore, media_port: u16) -> (std::net::SocketAddr, tokio::sync::mpsc::Receiver<ControlEvent>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let (_handle, events) =
        ControlServer::start_with_listener(config(media_port), trust, listener).expect("start");
    (addr, events)
}

fn trusting(device_id: &str) -> TrustStore {
    let mut store = TrustStore::in_memory();
    store.trust(device_id).expect("in-memory");
    store
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trusted_phone_should_receive_a_hello_ack() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;
    let mut client = Client::connect(addr).await;

    client.send(&ControlMessage::Hello(hello())).await;

    let Some(ControlMessage::HelloAck(ack)) = client.recv().await else {
        panic!("expected hello_ack");
    };
    assert_eq!(ack.version, PROTOCOL_VERSION);
    assert_eq!(ack.device_name, "cachy-desktop");
    assert_eq!(ack.media_port, 47811);
    assert_ne!(ack.session_id, 0, "session id must be assigned");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_handshake_should_negotiate_the_capability_intersection() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;
    let mut client = Client::connect(addr).await;

    // Phone offers cam+mic; desktop offers cam+mic+spk.
    client.send(&ControlMessage::Hello(hello())).await;

    let Some(ControlMessage::HelloAck(ack)) = client.recv().await else {
        panic!("expected hello_ack");
    };
    // The ack reports the desktop's own capabilities; the intersection is computed by both.
    assert!(ack.caps.contains(&"spk".to_owned()));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_phone_should_raise_a_pairing_request_and_connect_when_accepted() {
    let (addr, mut events) = start(TrustStore::in_memory(), 47811).await;
    let mut client = Client::connect(addr).await;

    client.send(&ControlMessage::Hello(hello())).await;

    let event = tokio::time::timeout(TIMEOUT, events.recv())
        .await
        .expect("prompt must arrive")
        .expect("event channel open");
    let ControlEvent::PairingRequest {
        device_name,
        respond,
        ..
    } = event
    else {
        panic!("expected a pairing request, got {event:?}");
    };
    assert_eq!(device_name, "Pixel 8");
    respond.send(true).expect("responder must be live");

    assert!(
        matches!(client.recv().await, Some(ControlMessage::HelloAck(_))),
        "accepting must complete the handshake"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_pairing_should_error_and_close() {
    let (addr, mut events) = start(TrustStore::in_memory(), 47811).await;
    let mut client = Client::connect(addr).await;

    client.send(&ControlMessage::Hello(hello())).await;

    let event = tokio::time::timeout(TIMEOUT, events.recv())
        .await
        .expect("prompt must arrive")
        .expect("event channel open");
    let ControlEvent::PairingRequest { respond, .. } = event else {
        panic!("expected a pairing request");
    };
    respond.send(false).expect("responder must be live");

    let Some(ControlMessage::Error(error)) = client.recv().await else {
        panic!("expected an error");
    };
    assert_eq!(error.reason, ErrorReason::Rejected);
    assert!(
        client.recv().await.is_none(),
        "server must close after rejecting"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_pairing_responder_should_count_as_a_rejection() {
    let (addr, mut events) = start(TrustStore::in_memory(), 47811).await;
    let mut client = Client::connect(addr).await;

    client.send(&ControlMessage::Hello(hello())).await;

    let event = tokio::time::timeout(TIMEOUT, events.recv())
        .await
        .expect("prompt must arrive")
        .expect("event channel open");
    let ControlEvent::PairingRequest { respond, .. } = event else {
        panic!("expected a pairing request");
    };
    drop(respond);

    let Some(ControlMessage::Error(error)) = client.recv().await else {
        panic!("expected an error");
    };
    assert_eq!(error.reason, ErrorReason::Rejected);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_incompatible_version_should_be_rejected_with_the_supported_version() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;
    let mut client = Client::connect(addr).await;

    let future = Hello {
        version: PROTOCOL_VERSION + 1,
        ..hello()
    };
    client.send(&ControlMessage::Hello(future)).await;

    let Some(ControlMessage::Error(error)) = client.recv().await else {
        panic!("expected an error");
    };
    assert_eq!(error.reason, ErrorReason::VersionMismatch);
    assert_eq!(error.supported_version, Some(PROTOCOL_VERSION));
    assert!(
        client.recv().await.is_none(),
        "server must close on version mismatch"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_phone_should_be_refused_as_busy() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;

    let mut first = Client::connect(addr).await;
    first.send(&ControlMessage::Hello(hello())).await;
    assert!(matches!(
        first.recv().await,
        Some(ControlMessage::HelloAck(_))
    ));

    let mut second = Client::connect(addr).await;
    second.send(&ControlMessage::Hello(hello())).await;

    let Some(ControlMessage::Error(error)) = second.recv().await else {
        panic!("expected a busy error");
    };
    assert_eq!(error.reason, ErrorReason::Busy);

    // The established session must be undisturbed.
    first
        .send(&ControlMessage::Ping { timestamp: 42 })
        .await;
    assert_eq!(
        first.recv().await,
        Some(ControlMessage::Pong { timestamp: 42 }),
        "the existing session must survive a refused second phone"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ping_should_be_answered_with_the_same_timestamp() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;
    let mut client = Client::connect(addr).await;

    client.send(&ControlMessage::Hello(hello())).await;
    let _ = client.recv().await;

    client
        .send(&ControlMessage::Ping {
            timestamp: 1_721_990_400_123_456,
        })
        .await;

    assert_eq!(
        client.recv().await,
        Some(ControlMessage::Pong {
            timestamp: 1_721_990_400_123_456
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_line_should_be_answered_without_closing_the_connection() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;
    let mut client = Client::connect(addr).await;

    client.send_raw("{this is not json}\n").await;

    let Some(ControlMessage::Error(error)) = client.recv().await else {
        panic!("expected a malformed error");
    };
    assert_eq!(error.reason, ErrorReason::Malformed);

    // The connection must still be usable.
    client.send(&ControlMessage::Hello(hello())).await;
    assert!(
        matches!(client.recv().await, Some(ControlMessage::HelloAck(_))),
        "a bad line must not end the session"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_without_a_type_should_be_answered_as_malformed() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;
    let mut client = Client::connect(addr).await;

    client.send_raw("{\"version\":1}\n").await;

    let Some(ControlMessage::Error(error)) = client.recv().await else {
        panic!("expected a malformed error");
    };
    assert_eq!(error.reason, ErrorReason::Malformed);
}

#[tokio::test(flavor = "multi_thread")]
async fn bye_should_end_the_session_and_report_the_peer_left() {
    let (addr, mut events) = start(trusting("phone-1"), 47811).await;
    let mut client = Client::connect(addr).await;

    client.send(&ControlMessage::Hello(hello())).await;
    let _ = client.recv().await;
    client.send(&ControlMessage::Bye).await;

    let mut saw_peer_left = false;
    while let Ok(Some(event)) = tokio::time::timeout(TIMEOUT, events.recv()).await {
        if matches!(event, ControlEvent::PeerLeft) {
            saw_peer_left = true;
            break;
        }
    }
    assert!(saw_peer_left, "bye must raise PeerLeft");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_should_be_released_when_the_phone_disconnects() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;

    {
        let mut first = Client::connect(addr).await;
        first.send(&ControlMessage::Hello(hello())).await;
        assert!(matches!(
            first.recv().await,
            Some(ControlMessage::HelloAck(_))
        ));
    } // dropped: socket closes

    // The next phone must be able to pair rather than hitting a stale "busy".
    let mut second = Client::connect(addr).await;
    second.send(&ControlMessage::Hello(hello())).await;
    assert!(
        matches!(second.recv().await, Some(ControlMessage::HelloAck(_))),
        "a dropped connection must free the session"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_session_should_keep_its_identifier() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;

    let session_id = {
        let mut first = Client::connect(addr).await;
        first.send(&ControlMessage::Hello(hello())).await;
        let Some(ControlMessage::HelloAck(ack)) = first.recv().await else {
            panic!("expected hello_ack");
        };
        ack.session_id
    };

    let mut resumed = Client::connect(addr).await;
    resumed
        .send(&ControlMessage::Hello(Hello {
            resume_session_id: Some(session_id),
            ..hello()
        }))
        .await;

    let Some(ControlMessage::HelloAck(ack)) = resumed.recv().await else {
        panic!("expected hello_ack");
    };
    assert_eq!(
        ack.session_id, session_id,
        "resuming must not invalidate in-flight media"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_line_should_be_ignored() {
    let (addr, _events) = start(trusting("phone-1"), 47811).await;
    let mut client = Client::connect(addr).await;

    client.send_raw("\n\n").await;
    client.send(&ControlMessage::Hello(hello())).await;

    assert!(
        matches!(client.recv().await, Some(ControlMessage::HelloAck(_))),
        "blank lines must not disturb the stream"
    );
}
