//! The TCP control channel: handshake, pairing, heartbeat, teardown.
//!
//! Control traffic is infrequent, small, and must not be lost, so it runs over TCP while media
//! runs over UDP. Splitting the planes keeps head-of-line blocking off the media path and
//! spares us from reimplementing reliable delivery for control.
//!
//! The TCP connection doubles as the liveness signal: if it drops, the session is over, with no
//! timeout guessing required.

use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::error::{NetError, Result};
use crate::protocol::{
    caps, intersect_caps, AudioCodec, AudioParams, ControlMessage, ErrorReason, Hello, HelloAck,
    StreamId, StreamRefusal, TelemetryReport, PROTOCOL_VERSION,
};
use crate::session::{ConnectionState, FailureReason};

/// How long a pairing prompt may sit unanswered before it counts as a rejection.
pub const PAIRING_PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Heartbeat interval.
pub const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// Unanswered heartbeats tolerated before the peer is declared unreachable.
pub const MAX_MISSED_HEARTBEATS: u32 = 3;

/// How long the application layer may take to prepare a sink before a `stream_start` is
/// refused as an internal failure.
pub const STREAM_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Decide how to answer a `stream_start`, without touching the network.
///
/// Pure so every refusal branch is unit-testable. Codec support beyond "a codec this build has
/// never heard of" is the application layer's call — it owns the decoder.
///
/// # Errors
///
/// Returns the [`StreamRefusal`] to put in the `stream_ack` when the stream must be refused.
pub fn evaluate_stream_start(
    stream: u8,
    params: &AudioParams,
    negotiated_caps: &[String],
) -> std::result::Result<(), StreamRefusal> {
    // Streams the desktop can sink, and the capability token that gates each.
    let required_cap = match StreamId(stream) {
        StreamId::CAMERA => caps::CAMERA,
        StreamId::MICROPHONE => caps::MICROPHONE,
        // The desktop is the *source* for the speaker stream; a phone offering to send it —
        // or any id we have never heard of — is refused the same way.
        _ => return Err(StreamRefusal::UnsupportedStream),
    };
    if !negotiated_caps.iter().any(|c| c == required_cap) {
        return Err(StreamRefusal::NotNegotiated);
    }
    if matches!(params.codec, AudioCodec::Unknown) {
        return Err(StreamRefusal::UnsupportedCodec);
    }
    Ok(())
}

/// Device ids the user has already approved.
///
/// Persisted so a phone the user accepted once is not re-prompted on every reconnect — which
/// would make the automatic reconnect path useless.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TrustStore {
    trusted: BTreeSet<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl TrustStore {
    /// Load the trust store from `path`, starting empty if it is absent or unreadable.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let mut store = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok())
            .unwrap_or_default();
        store.path = Some(path.to_path_buf());
        store
    }

    /// An in-memory store that never persists. Used in tests.
    #[must_use]
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Whether this device id has been approved before.
    #[must_use]
    pub fn is_trusted(&self, device_id: &str) -> bool {
        self.trusted.contains(device_id)
    }

    /// Record a device id as approved, and persist.
    ///
    /// # Errors
    ///
    /// Fails if the store is backed by a file that cannot be written.
    pub fn trust(&mut self, device_id: &str) -> Result<()> {
        self.trusted.insert(device_id.to_owned());
        self.persist()
    }

    /// Forget a device id, so it is prompted for again.
    ///
    /// # Errors
    ///
    /// Fails if the store is backed by a file that cannot be written.
    pub fn revoke(&mut self, device_id: &str) -> Result<()> {
        self.trusted.remove(device_id);
        self.persist()
    }

    /// Every trusted device id.
    #[must_use]
    pub fn trusted_ids(&self) -> Vec<String> {
        self.trusted.iter().cloned().collect()
    }

    fn persist(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| NetError::MalformedControl(format!("cannot encode trust store: {e}")))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// Identity of the session currently held, for deciding whether a `hello` may displace it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSessionInfo {
    /// Session identifier issued to the current peer.
    pub session_id: u64,
    /// Device id of the current peer.
    pub device_id: String,
}

/// What the desktop should do with an incoming `hello`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelloVerdict {
    /// Proceed, prompting the user first because this device is unknown.
    PromptUser,
    /// Proceed without prompting; the device is already trusted.
    AcceptTrusted,
    /// The same phone resuming its own session — accept and displace the stale connection.
    Resume,
    /// Refuse, sending this error and closing the connection.
    Reject(ErrorReason),
}

/// Decide how to answer a `hello`, without touching the network.
///
/// Pulled out as a pure function so every branch — version, busy, resume, trusted — is
/// unit-testable without sockets or timing.
#[must_use]
pub fn evaluate_hello(
    hello: &Hello,
    active: Option<&ActiveSessionInfo>,
    trusted: bool,
) -> HelloVerdict {
    if hello.version != PROTOCOL_VERSION {
        return HelloVerdict::Reject(ErrorReason::VersionMismatch);
    }
    if hello.device_id.trim().is_empty() {
        return HelloVerdict::Reject(ErrorReason::Malformed);
    }

    if let Some(active) = active {
        // The same phone resuming its own session displaces the old connection. Without this,
        // a phone reconnecting after a Wi-Fi blip is told "busy" by its own stale socket —
        // which TCP may not have noticed is dead — and the reconnect path dies on a refusal
        // it can never clear.
        let resuming_own_session = hello.resume_session_id == Some(active.session_id)
            && hello.device_id == active.device_id;
        if resuming_own_session {
            return HelloVerdict::Resume;
        }
        // Otherwise a live session wins, trusted or not.
        return HelloVerdict::Reject(ErrorReason::Busy);
    }

    if trusted {
        HelloVerdict::AcceptTrusted
    } else {
        HelloVerdict::PromptUser
    }
}

/// Things the control server tells the application layer about.
#[derive(Debug)]
#[non_exhaustive]
pub enum ControlEvent {
    /// An unknown phone wants to pair. Answer through `respond`.
    PairingRequest {
        /// Phone's stable device id.
        device_id: String,
        /// Phone's display name.
        device_name: String,
        /// Send `true` to accept. Dropping this responder counts as a rejection.
        respond: oneshot::Sender<bool>,
    },
    /// The connection lifecycle advanced.
    StateChanged(ConnectionState),
    /// The peer reported its link quality.
    Telemetry(TelemetryReport),
    /// A round-trip sample was measured locally, in milliseconds.
    RttSample(f64),
    /// The peer disconnected cleanly.
    PeerLeft,
    /// The peer announced a stream it wants to send, and protocol-level checks passed.
    ///
    /// Answer through `respond` once the sink is ready (`Ok`) or has failed (`Err`). Dropping
    /// the responder, or taking longer than [`STREAM_START_TIMEOUT`], refuses the stream as an
    /// internal failure. The accepting `stream_ack` is sent only after a successful answer, so
    /// media never arrives before the sink exists.
    StreamStartRequested {
        /// Stream identifier, protocol §2.2.
        stream: u8,
        /// Format the peer will send.
        params: AudioParams,
        /// Send `Ok(())` to accept, or the refusal to report.
        respond: oneshot::Sender<std::result::Result<(), StreamRefusal>>,
    },
    /// An active stream ended — peer stop, restart with new parameters, or session teardown.
    StreamStopped {
        /// Stream identifier that is no longer active.
        stream: u8,
    },
}

/// Everything the control server needs to answer a handshake.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// This desktop's stable device id.
    pub device_id: String,
    /// This desktop's display name.
    pub device_name: String,
    /// Capability tokens this build supports.
    pub caps: Vec<String>,
    /// TCP port to listen on.
    pub control_port: u16,
    /// UDP port already bound for media, advertised in `hello_ack`.
    pub media_port: u16,
}

#[derive(Debug, Default)]
struct SharedState {
    session: Option<ActiveSession>,
}

#[derive(Debug, Clone)]
struct ActiveSession {
    session_id: u64,
    /// Identifies which connection owns this session, so a connection that has already been
    /// superseded cannot clear a newer one on its way out.
    connection_id: u64,
    #[allow(dead_code, reason = "recorded for the revoke path added with the pairing UI")]
    peer_id: String,
    #[allow(dead_code, reason = "surfaced by the dashboard once the UI reads it directly")]
    peer_name: String,
    negotiated_caps: Vec<String>,
    #[allow(dead_code, reason = "kept for logging and the revoke UI")]
    peer_addr: SocketAddr,
    /// Where to send media to this peer: its TCP address, with the UDP port it advertised.
    peer_media_addr: Option<SocketAddr>,
    /// Outbound queue of the connection serving this session.
    outbound: mpsc::Sender<ControlMessage>,
    /// Streams the peer has started and we have accepted, with their negotiated parameters.
    active_streams: HashMap<u8, AudioParams>,
}

/// Handle to a running control server.
#[derive(Debug, Clone)]
pub struct ControlHandle {
    shared: Arc<Mutex<SharedState>>,
}

impl ControlHandle {
    /// Send a control message to the connected peer.
    ///
    /// # Errors
    ///
    /// Returns [`NetError::NoSession`] if no peer is connected.
    pub async fn send(&self, message: ControlMessage) -> Result<()> {
        let outbound = {
            let state = self.shared.lock().await;
            state
                .session
                .as_ref()
                .map(|s| s.outbound.clone())
                .ok_or(NetError::NoSession)?
        };
        outbound.send(message).await.map_err(|_| NetError::NoSession)
    }

    /// Report this side's link quality to the peer.
    ///
    /// # Errors
    ///
    /// Returns [`NetError::NoSession`] if no peer is connected.
    pub async fn send_telemetry(&self, report: TelemetryReport) -> Result<()> {
        self.send(ControlMessage::Telemetry(report)).await
    }

    /// End the session cleanly.
    ///
    /// # Errors
    ///
    /// Returns [`NetError::NoSession`] if no peer is connected.
    pub async fn disconnect(&self) -> Result<()> {
        self.send(ControlMessage::Bye).await
    }

    /// The active session id, if a session is established.
    pub async fn session_id(&self) -> Option<u64> {
        self.shared
            .lock()
            .await
            .session
            .as_ref()
            .map(|s| s.session_id)
    }

    /// Where to send media for the active session, if the peer advertised a port.
    pub async fn peer_media_addr(&self) -> Option<SocketAddr> {
        self.shared
            .lock()
            .await
            .session
            .as_ref()
            .and_then(|s| s.peer_media_addr)
    }

    /// Capabilities both sides agreed on.
    pub async fn negotiated_caps(&self) -> Vec<String> {
        self.shared
            .lock()
            .await
            .session
            .as_ref()
            .map(|s| s.negotiated_caps.clone())
            .unwrap_or_default()
    }

    /// Ask the peer to start or stop a stream it sources, protocol §3.9.4.
    ///
    /// # Errors
    ///
    /// Returns [`NetError::NoSession`] if no peer is connected.
    pub async fn request_stream(&self, stream: StreamId, active: bool) -> Result<()> {
        self.send(ControlMessage::StreamRequest {
            stream: stream.get(),
            active,
        })
        .await
    }

    /// Parameters of an accepted stream, if it is currently active.
    pub async fn stream_params(&self, stream: StreamId) -> Option<AudioParams> {
        self.shared
            .lock()
            .await
            .session
            .as_ref()
            .and_then(|s| s.active_streams.get(&stream.get()).copied())
    }
}

/// Generate a session identifier.
///
/// Uses a v4 UUID's low bits — already sourced from the platform CSPRNG, so this avoids
/// pulling in a separate RNG crate for one value. The low bit is forced so the value is never
/// zero, which the receive path treats as "no session".
#[must_use]
pub fn new_session_id() -> u64 {
    #[allow(clippy::cast_possible_truncation, reason = "truncation to 64 bits is the point")]
    let low = uuid::Uuid::new_v4().as_u128() as u64;
    low | 1
}

/// Per-connection context threaded through message handling.
struct ConnectionCtx<'a> {
    peer_addr: SocketAddr,
    connection_id: u64,
    out_tx: &'a mpsc::Sender<ControlMessage>,
}

/// The desktop-side control server. Listens, handshakes, and serves one session at a time.
pub struct ControlServer {
    config: ServerConfig,
    trust: Arc<Mutex<TrustStore>>,
    shared: Arc<Mutex<SharedState>>,
    events: mpsc::Sender<ControlEvent>,
}

impl ControlServer {
    /// Bind the control port and start accepting.
    ///
    /// Returns a handle for sending to the peer, and a receiver of everything that happens.
    ///
    /// # Errors
    ///
    /// Fails if the control port cannot be bound.
    pub async fn start(
        config: ServerConfig,
        trust: TrustStore,
    ) -> Result<(ControlHandle, mpsc::Receiver<ControlEvent>)> {
        let listener = TcpListener::bind(("0.0.0.0", config.control_port)).await?;
        tracing::info!(port = config.control_port, "control channel listening");
        Self::start_with_listener(config, trust, listener)
    }

    /// Start on an already-bound listener. Lets tests bind port 0.
    ///
    /// # Errors
    ///
    /// Currently infallible; returns [`Result`] to match [`ControlServer::start`].
    pub fn start_with_listener(
        config: ServerConfig,
        trust: TrustStore,
        listener: TcpListener,
    ) -> Result<(ControlHandle, mpsc::Receiver<ControlEvent>)> {
        let (event_tx, event_rx) = mpsc::channel(64);

        let shared = Arc::new(Mutex::new(SharedState::default()));
        let server = Arc::new(Self {
            config,
            trust: Arc::new(Mutex::new(trust)),
            shared: Arc::clone(&shared),
            events: event_tx,
        });

        let handle = ControlHandle { shared };

        tokio::spawn(async move {
            server.accept_loop(listener).await;
        });

        Ok((handle, event_rx))
    }

    /// Accepts connections concurrently, serving each in its own task.
    ///
    /// Serving sequentially would leave a second phone hanging in the TCP backlog instead of
    /// receiving the `busy` error it is owed — the user would see an indefinite spinner rather
    /// than a reason.
    async fn accept_loop(self: Arc<Self>, listener: TcpListener) {
        let next_connection_id = std::sync::atomic::AtomicU64::new(1);

        loop {
            let (stream, peer_addr) = match listener.accept().await {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::error!(error = %e, "accept failed");
                    continue;
                }
            };
            tracing::info!(%peer_addr, "control connection accepted");

            let connection_id =
                next_connection_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let server = Arc::clone(&self);

            tokio::spawn(async move {
                let (out_tx, out_rx) = mpsc::channel(64);
                if let Err(e) = server
                    .serve(stream, peer_addr, connection_id, out_tx, out_rx)
                    .await
                {
                    tracing::warn!(%peer_addr, error = %e, "control connection ended");
                }
                server.release_session(connection_id).await;
            });
        }
    }

    /// Clear the session, but only if this connection is the one that owns it.
    async fn release_session(&self, connection_id: u64) {
        let mut state = self.shared.lock().await;
        let owns = state
            .session
            .as_ref()
            .is_some_and(|s| s.connection_id == connection_id);
        if !owns {
            return;
        }
        let stopped: Vec<u8> = state
            .session
            .take()
            .map(|s| s.active_streams.into_keys().collect())
            .unwrap_or_default();
        drop(state);

        // A dead session implies a stop for every stream it carried, with no `stream_stop`
        // messages required — the sink must not outlive the peer feeding it.
        for stream in stopped {
            let _ = self.events.send(ControlEvent::StreamStopped { stream }).await;
        }

        let _ = self
            .events
            .send(ControlEvent::StateChanged(ConnectionState::Discovering))
            .await;
    }

    async fn serve(
        &self,
        stream: TcpStream,
        peer_addr: SocketAddr,
        connection_id: u64,
        out_tx: mpsc::Sender<ControlMessage>,
        mut outbound: mpsc::Receiver<ControlMessage>,
    ) -> Result<()> {
        // Nagle would coalesce our tiny control messages and add latency to the handshake.
        let _ = stream.set_nodelay(true);

        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        let mut missed_heartbeats = 0_u32;
        let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Some(line) = line? else {
                        return Ok(()); // peer closed the connection
                    };
                    if line.trim().is_empty() {
                        continue;
                    }
                    missed_heartbeats = 0;

                    match ControlMessage::from_line(&line) {
                        Ok(message) => {
                            let ctx = ConnectionCtx {
                                peer_addr,
                                connection_id,
                                out_tx: &out_tx,
                            };
                            if self.handle_message(message, &ctx, &mut write_half).await? {
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            // A single bad line does not end a session.
                            tracing::warn!(%peer_addr, error = %e, "malformed control line");
                            let reply =
                                ControlMessage::error(ErrorReason::Malformed, e.to_string());
                            write_line(&mut write_half, &reply).await?;
                        }
                    }
                }

                Some(message) = outbound.recv() => {
                    let is_bye = matches!(message, ControlMessage::Bye);
                    write_line(&mut write_half, &message).await?;
                    if is_bye {
                        return Ok(());
                    }
                }

                _ = heartbeat.tick() => {
                    if self.shared.lock().await.session.is_some() {
                        missed_heartbeats += 1;
                        if missed_heartbeats > MAX_MISSED_HEARTBEATS {
                            let _ = self.events.send(ControlEvent::StateChanged(
                                ConnectionState::Failed {
                                    reason: FailureReason::Other(
                                        format!("no heartbeat from {peer_addr}"),
                                    ),
                                },
                            )).await;
                            return Err(NetError::PeerUnreachable(peer_addr));
                        }
                    }
                }
            }
        }
    }

    /// Returns `Ok(true)` when the connection should close.
    async fn handle_message(
        &self,
        message: ControlMessage,
        ctx: &ConnectionCtx<'_>,
        write_half: &mut tokio::net::tcp::OwnedWriteHalf,
    ) -> Result<bool> {
        let peer_addr = ctx.peer_addr;
        match message {
            ControlMessage::Hello(hello) => self.handle_hello(hello, ctx, write_half).await,

            ControlMessage::Ping { timestamp } => {
                // Echoed verbatim: the value is the peer's clock and is opaque to us.
                write_line(write_half, &ControlMessage::Pong { timestamp }).await?;
                Ok(false)
            }

            ControlMessage::Pong { timestamp } => {
                let rtt_us = now_micros().saturating_sub(timestamp);
                #[allow(clippy::cast_precision_loss, reason = "microseconds fit f64 exactly here")]
                let rtt_ms = rtt_us as f64 / 1000.0;
                let _ = self.events.send(ControlEvent::RttSample(rtt_ms)).await;
                Ok(false)
            }

            ControlMessage::Telemetry(report) => {
                let _ = self.events.send(ControlEvent::Telemetry(report)).await;
                Ok(false)
            }

            ControlMessage::Bye => {
                tracing::info!(%peer_addr, "peer said bye");
                let _ = self.events.send(ControlEvent::PeerLeft).await;
                Ok(true)
            }

            ControlMessage::StreamStart { stream, params } => {
                self.handle_stream_start(stream, params, ctx, write_half)
                    .await?;
                Ok(false)
            }

            ControlMessage::StreamStop { stream } => {
                self.handle_stream_stop(stream, ctx.connection_id).await;
                Ok(false)
            }

            ControlMessage::StreamRequest { stream, active } => {
                // The desktop sources only the speaker stream, which is a later change; until
                // then every start request is answered rather than left hanging.
                if active {
                    let reply =
                        ControlMessage::stream_refuse(stream, StreamRefusal::UnsupportedStream);
                    write_line(write_half, &reply).await?;
                }
                Ok(false)
            }

            ControlMessage::StreamAck { stream, accepted, .. } => {
                // Answers a `stream_start` the desktop sent; it sends none until the speaker
                // stream exists.
                tracing::debug!(%peer_addr, stream, accepted, "ignoring stream_ack");
                Ok(false)
            }

            ControlMessage::HelloAck(_) | ControlMessage::Error(_) => {
                // The desktop is the server; these are client-side messages.
                tracing::debug!(%peer_addr, "ignoring client-side message");
                Ok(false)
            }
        }
    }

    /// Validate a `stream_start`, let the application layer stand up the sink, and answer.
    async fn handle_stream_start(
        &self,
        stream: u8,
        params: AudioParams,
        ctx: &ConnectionCtx<'_>,
        write_half: &mut tokio::net::tcp::OwnedWriteHalf,
    ) -> Result<()> {
        let verdict = {
            let state = self.shared.lock().await;
            match state.session.as_ref() {
                // Only the connection that owns the session may open streams on it.
                Some(session) if session.connection_id == ctx.connection_id => {
                    evaluate_stream_start(stream, &params, &session.negotiated_caps)
                }
                _ => Err(StreamRefusal::NotNegotiated),
            }
        };

        if let Err(reason) = verdict {
            tracing::info!(stream, ?reason, "stream refused");
            write_line(write_half, &ControlMessage::stream_refuse(stream, reason)).await?;
            return Ok(());
        }

        // A restart replaces the old stream: tear its sink down before standing up the new one
        // so the application layer never juggles two generations of the same stream.
        let was_active = {
            let mut state = self.shared.lock().await;
            state
                .session
                .as_mut()
                .is_some_and(|s| s.active_streams.remove(&stream).is_some())
        };
        if was_active {
            let _ = self.events.send(ControlEvent::StreamStopped { stream }).await;
        }

        let (respond, ready) = oneshot::channel();
        let requested = ControlEvent::StreamStartRequested {
            stream,
            params,
            respond,
        };
        if self.events.send(requested).await.is_err() {
            // Nobody is listening, so nobody can build a sink.
            write_line(
                write_half,
                &ControlMessage::stream_refuse(stream, StreamRefusal::Internal),
            )
            .await?;
            return Ok(());
        }

        let answer = match tokio::time::timeout(STREAM_START_TIMEOUT, ready).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) | Err(_) => Err(StreamRefusal::Internal),
        };

        match answer {
            Ok(()) => {
                let mut state = self.shared.lock().await;
                if let Some(session) = state.session.as_mut() {
                    session.active_streams.insert(stream, params);
                }
                drop(state);
                tracing::info!(stream, ?params, "stream accepted");
                write_line(write_half, &ControlMessage::stream_accept(stream)).await
            }
            Err(reason) => {
                tracing::warn!(stream, ?reason, "sink refused stream");
                write_line(write_half, &ControlMessage::stream_refuse(stream, reason)).await
            }
        }
    }

    /// Handle a peer's `stream_stop`. Stopping a stream that is not active is a no-op.
    async fn handle_stream_stop(&self, stream: u8, connection_id: u64) {
        let was_active = {
            let mut state = self.shared.lock().await;
            state
                .session
                .as_mut()
                .filter(|s| s.connection_id == connection_id)
                .is_some_and(|s| s.active_streams.remove(&stream).is_some())
        };
        if was_active {
            tracing::info!(stream, "stream stopped by peer");
            let _ = self.events.send(ControlEvent::StreamStopped { stream }).await;
        }
    }

    async fn handle_hello(
        &self,
        hello: Hello,
        ctx: &ConnectionCtx<'_>,
        write_half: &mut tokio::net::tcp::OwnedWriteHalf,
    ) -> Result<bool> {
        let active = self.shared.lock().await.session.as_ref().map(|s| ActiveSessionInfo {
            session_id: s.session_id,
            device_id: s.peer_id.clone(),
        });
        let trusted = self.trust.lock().await.is_trusted(&hello.device_id);

        match evaluate_hello(&hello, active.as_ref(), trusted) {
            HelloVerdict::Reject(ErrorReason::VersionMismatch) => {
                write_line(
                    write_half,
                    &ControlMessage::version_mismatch(PROTOCOL_VERSION),
                )
                .await?;
                Ok(true)
            }
            HelloVerdict::Reject(reason) => {
                let text = match reason {
                    ErrorReason::Busy => "already paired with another device",
                    ErrorReason::Malformed => "hello is missing a device id",
                    _ => "rejected",
                };
                write_line(write_half, &ControlMessage::error(reason, text)).await?;
                Ok(true)
            }
            HelloVerdict::AcceptTrusted | HelloVerdict::Resume => {
                self.establish(hello, ctx, write_half).await?;
                Ok(false)
            }
            HelloVerdict::PromptUser => {
                if self.ask_user(&hello).await {
                    self.trust.lock().await.trust(&hello.device_id)?;
                    self.establish(hello, ctx, write_half).await?;
                    Ok(false)
                } else {
                    let reply =
                        ControlMessage::error(ErrorReason::Rejected, "pairing was declined");
                    write_line(write_half, &reply).await?;
                    let _ = self
                        .events
                        .send(ControlEvent::StateChanged(ConnectionState::Failed {
                            reason: FailureReason::Rejected,
                        }))
                        .await;
                    Ok(true)
                }
            }
        }
    }

    async fn ask_user(&self, hello: &Hello) -> bool {
        let (respond, answer) = oneshot::channel();
        let request = ControlEvent::PairingRequest {
            device_id: hello.device_id.clone(),
            device_name: hello.device_name.clone(),
            respond,
        };
        if self.events.send(request).await.is_err() {
            // Nobody is listening for prompts, so nobody can approve one.
            return false;
        }

        // A prompt nobody answers is a prompt the user walked away from.
        match tokio::time::timeout(PAIRING_PROMPT_TIMEOUT, answer).await {
            Ok(Ok(accepted)) => accepted,
            Ok(Err(_)) => false, // responder dropped
            Err(_) => {
                tracing::info!(device = %hello.device_name, "pairing prompt timed out");
                false
            }
        }
    }

    async fn establish(
        &self,
        hello: Hello,
        ctx: &ConnectionCtx<'_>,
        write_half: &mut tokio::net::tcp::OwnedWriteHalf,
    ) -> Result<()> {
        // Resuming reuses the id so the phone's in-flight media stays valid across a blip.
        let session_id = hello.resume_session_id.unwrap_or_else(new_session_id);
        let negotiated_caps = intersect_caps(&self.config.caps, &hello.caps);

        let ack = HelloAck {
            version: PROTOCOL_VERSION,
            device_id: self.config.device_id.clone(),
            device_name: self.config.device_name.clone(),
            caps: self.config.caps.clone(),
            session_id,
            media_port: self.config.media_port,
        };
        write_line(write_half, &ControlMessage::HelloAck(ack)).await?;

        // A resume displaces the previous connection's session. Its streams stop with it: the
        // phone re-announces anything it is still sending, and a fresh `stream_start` rebuilds
        // the sink, so nothing is left wired to a dead connection.
        let displaced: Vec<u8> = {
            let mut state = self.shared.lock().await;
            let displaced = state
                .session
                .take()
                .map(|s| s.active_streams.into_keys().collect())
                .unwrap_or_default();
            state.session = Some(ActiveSession {
                session_id,
                connection_id: ctx.connection_id,
                peer_id: hello.device_id.clone(),
                peer_name: hello.device_name.clone(),
                negotiated_caps: negotiated_caps.clone(),
                peer_addr: ctx.peer_addr,
                peer_media_addr: hello
                    .media_port
                    .map(|port| SocketAddr::new(ctx.peer_addr.ip(), port)),
                outbound: ctx.out_tx.clone(),
                active_streams: HashMap::new(),
            });
            displaced
        };
        for stream in displaced {
            let _ = self.events.send(ControlEvent::StreamStopped { stream }).await;
        }

        tracing::info!(
            session_id,
            peer = %hello.device_name,
            caps = ?negotiated_caps,
            "session established"
        );

        let _ = self
            .events
            .send(ControlEvent::StateChanged(ConnectionState::Connected {
                peer_name: hello.device_name,
                session_id,
            }))
            .await;

        Ok(())
    }
}

async fn write_line(
    write_half: &mut tokio::net::tcp::OwnedWriteHalf,
    message: &ControlMessage,
) -> Result<()> {
    let line = message.to_line()?;
    write_half.write_all(line.as_bytes()).await?;
    write_half.flush().await?;
    Ok(())
}

/// Monotonic microseconds since first call, for heartbeat timestamps.
#[must_use]
pub fn now_micros() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Hello {
        Hello {
            version: PROTOCOL_VERSION,
            device_id: "phone-1".to_owned(),
            device_name: "Pixel 8".to_owned(),
            caps: vec!["cam".to_owned(), "mic".to_owned()],
            resume_session_id: None,
            media_port: Some(crate::DEFAULT_MEDIA_PORT),
        }
    }

    fn active_session() -> ActiveSessionInfo {
        ActiveSessionInfo {
            session_id: 4_242,
            device_id: "phone-1".to_owned(),
        }
    }

    #[test]
    fn an_unknown_device_should_prompt_the_user() {
        assert_eq!(
            evaluate_hello(&hello(), None, false),
            HelloVerdict::PromptUser
        );
    }

    #[test]
    fn a_trusted_device_should_skip_the_prompt() {
        assert_eq!(
            evaluate_hello(&hello(), None, true),
            HelloVerdict::AcceptTrusted
        );
    }

    #[test]
    fn a_second_device_should_be_refused_as_busy() {
        let other = Hello {
            device_id: "phone-2".to_owned(),
            ..hello()
        };
        assert_eq!(
            evaluate_hello(&other, Some(&active_session()), false),
            HelloVerdict::Reject(ErrorReason::Busy)
        );
    }

    #[test]
    fn even_a_trusted_device_should_be_refused_while_a_session_is_active() {
        let other = Hello {
            device_id: "phone-2".to_owned(),
            ..hello()
        };
        assert_eq!(
            evaluate_hello(&other, Some(&active_session()), true),
            HelloVerdict::Reject(ErrorReason::Busy)
        );
    }

    #[test]
    fn a_phone_resuming_its_own_session_should_displace_the_stale_connection() {
        let resuming = Hello {
            resume_session_id: Some(4_242),
            ..hello()
        };
        assert_eq!(
            evaluate_hello(&resuming, Some(&active_session()), true),
            HelloVerdict::Resume
        );
    }

    #[test]
    fn a_different_phone_may_not_steal_a_session_by_guessing_its_id() {
        let impostor = Hello {
            device_id: "phone-2".to_owned(),
            resume_session_id: Some(4_242),
            ..hello()
        };
        assert_eq!(
            evaluate_hello(&impostor, Some(&active_session()), true),
            HelloVerdict::Reject(ErrorReason::Busy)
        );
    }

    #[test]
    fn resuming_the_wrong_session_id_should_still_be_refused_as_busy() {
        let wrong = Hello {
            resume_session_id: Some(9_999),
            ..hello()
        };
        assert_eq!(
            evaluate_hello(&wrong, Some(&active_session()), true),
            HelloVerdict::Reject(ErrorReason::Busy)
        );
    }

    #[test]
    fn an_unsupported_version_should_be_refused() {
        let stale = Hello {
            version: PROTOCOL_VERSION + 1,
            ..hello()
        };
        assert_eq!(
            evaluate_hello(&stale, None, true),
            HelloVerdict::Reject(ErrorReason::VersionMismatch)
        );
    }

    #[test]
    fn a_version_mismatch_should_outrank_a_busy_session() {
        let stale = Hello {
            version: PROTOCOL_VERSION + 1,
            ..hello()
        };
        assert_eq!(
            evaluate_hello(&stale, Some(&active_session()), false),
            HelloVerdict::Reject(ErrorReason::VersionMismatch)
        );
    }

    #[test]
    fn a_hello_without_a_device_id_should_be_refused() {
        let anonymous = Hello {
            device_id: "   ".to_owned(),
            ..hello()
        };
        assert_eq!(
            evaluate_hello(&anonymous, None, false),
            HelloVerdict::Reject(ErrorReason::Malformed)
        );
    }

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "unifiedstream-trust-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        path
    }

    #[test]
    fn an_empty_trust_store_should_trust_nobody() {
        assert!(!TrustStore::in_memory().is_trusted("phone-1"));
    }

    #[test]
    fn trusting_a_device_should_be_remembered() {
        let mut store = TrustStore::in_memory();
        store.trust("phone-1").expect("in-memory store cannot fail");
        assert!(store.is_trusted("phone-1"));
    }

    #[test]
    fn revoking_a_device_should_prompt_again() {
        let mut store = TrustStore::in_memory();
        store.trust("phone-1").expect("in-memory");
        store.revoke("phone-1").expect("in-memory");
        assert!(!store.is_trusted("phone-1"));
    }

    #[test]
    fn a_trust_store_should_survive_a_reload() {
        let path = temp_path("persist");
        {
            let mut store = TrustStore::load(&path);
            store.trust("phone-1").expect("must persist");
        }
        let reloaded = TrustStore::load(&path);
        assert!(reloaded.is_trusted("phone-1"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_corrupt_trust_store_should_load_as_empty() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "not json").expect("write fixture");

        let store = TrustStore::load(&path);
        assert!(store.trusted_ids().is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_trust_store_should_load_as_empty() {
        assert!(TrustStore::load(&temp_path("absent"))
            .trusted_ids()
            .is_empty());
    }

    #[test]
    fn session_ids_should_differ_between_sessions() {
        assert_ne!(new_session_id(), new_session_id());
    }

    #[test]
    fn a_session_id_should_never_be_zero() {
        // Zero is the "no session" sentinel on the receive path.
        for _ in 0..1_000 {
            assert_ne!(new_session_id(), 0);
        }
    }

    #[test]
    fn now_micros_should_advance() {
        let first = now_micros();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(now_micros() > first);
    }

    fn negotiated() -> Vec<String> {
        vec!["cam".to_owned(), "mic".to_owned(), "spk".to_owned()]
    }

    #[test]
    fn a_negotiated_microphone_stream_should_be_accepted() {
        assert_eq!(
            evaluate_stream_start(2, &AudioParams::MICROPHONE_PCM, &negotiated()),
            Ok(())
        );
    }

    #[test]
    fn a_stream_without_its_capability_should_be_refused_as_not_negotiated() {
        let caps = vec!["cam".to_owned()];
        assert_eq!(
            evaluate_stream_start(2, &AudioParams::MICROPHONE_PCM, &caps),
            Err(StreamRefusal::NotNegotiated)
        );
    }

    #[test]
    fn an_unknown_codec_should_be_refused_as_unsupported_codec() {
        let params = AudioParams {
            codec: AudioCodec::Unknown,
            ..AudioParams::MICROPHONE_PCM
        };
        assert_eq!(
            evaluate_stream_start(2, &params, &negotiated()),
            Err(StreamRefusal::UnsupportedCodec)
        );
    }

    #[test]
    fn a_stream_the_desktop_cannot_sink_should_be_refused_as_unsupported() {
        // Stream 3 is sourced by the desktop, and 200 is unassigned; a phone may offer neither.
        for stream in [3, 200] {
            assert_eq!(
                evaluate_stream_start(stream, &AudioParams::MICROPHONE_PCM, &negotiated()),
                Err(StreamRefusal::UnsupportedStream),
                "stream {stream} must be refused"
            );
        }
    }

    #[test]
    fn heartbeat_constants_should_match_the_protocol_document() {
        assert_eq!(HEARTBEAT_INTERVAL, std::time::Duration::from_secs(1));
        assert_eq!(MAX_MISSED_HEARTBEATS, 3);
        assert_eq!(PAIRING_PROMPT_TIMEOUT, std::time::Duration::from_secs(30));
    }
}
