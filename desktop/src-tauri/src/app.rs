//! Application state and the Tauri command surface.
//!
//! Everything network-facing lives in `unifiedstream-net`; this layer only bridges it to the
//! webview — commands in, events out.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{oneshot, Mutex};

use unifiedstream_net::control::{
    ControlEvent, ControlHandle, ControlServer, ServerConfig, TrustStore,
};
use unifiedstream_net::discovery::{AdvertiseConfig, Advertiser, DeviceIdentity};
use unifiedstream_net::protocol::{caps, StreamId, TelemetryReport};
use unifiedstream_net::session::ConnectionState;
use unifiedstream_net::telemetry::{LinkQuality, TelemetryCollector, REPORT_INTERVAL};
use unifiedstream_net::transport::{
    MediaDemux, MediaSender, MediaSocket, TestStreamConfig, TestStreamGenerator,
    TestStreamReport, TestStreamVerifier, MAX_DATAGRAM,
};
use unifiedstream_net::{DEFAULT_CONTROL_PORT, DEFAULT_MEDIA_PORT};

/// Event names emitted to the webview. Kept in one place so the TypeScript side has a single
/// list to mirror.
pub mod events {
    /// Connection lifecycle changed.
    pub const CONNECTION_STATE: &str = "connection-state";
    /// An unknown phone wants to pair.
    pub const PAIRING_REQUEST: &str = "pairing-request";
    /// 1 Hz link-quality tick.
    pub const TELEMETRY: &str = "telemetry";
    /// Synthetic test stream counters.
    pub const TEST_STREAM: &str = "test-stream";
}

/// Errors surfaced to the webview.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The networking layer failed.
    #[error("{0}")]
    Net(#[from] unifiedstream_net::NetError),
    /// The requested action does not apply in the current state.
    #[error("{0}")]
    State(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Named distinctly from `std::result::Result` so the two-parameter form stays usable.
type AppResult<T> = std::result::Result<T, AppError>;

/// A pairing request awaiting the user's answer.
#[derive(Debug, Clone, Serialize)]
pub struct PairingRequest {
    /// Phone's stable device id.
    pub device_id: String,
    /// Phone's display name.
    pub device_name: String,
}

/// The 1 Hz telemetry tick sent to the webview.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryTick {
    /// Locally measured link quality.
    pub local: TelemetryReport,
    /// What the phone most recently reported, if anything.
    pub peer: Option<TelemetryReport>,
    /// Qualitative rating derived from latency and loss.
    pub quality: LinkQuality,
}

/// What the UI needs to render the whole screen after a reload.
#[derive(Debug, Clone, Serialize)]
pub struct StatusSnapshot {
    /// This desktop's display name.
    pub device_name: String,
    /// This desktop's stable id.
    pub device_id: String,
    /// Whether mDNS advertisement is running.
    pub advertising: bool,
    /// Current connection lifecycle state.
    pub state: ConnectionState,
    /// TCP control port.
    pub control_port: u16,
    /// UDP media port actually bound.
    pub media_port: u16,
    /// Capability tokens this build supports.
    pub caps: Vec<String>,
    /// Whether the synthetic test stream is running.
    pub test_stream_running: bool,
}

/// Knobs for the synthetic test stream, as sent from the UI.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct TestStreamArgs {
    /// Frames per second.
    pub rate_hz: u32,
    /// Frame size in bytes. Above 1200 this exercises fragmentation.
    pub frame_bytes: usize,
}

impl From<TestStreamArgs> for TestStreamConfig {
    fn from(args: TestStreamArgs) -> Self {
        Self {
            rate_hz: args.rate_hz.clamp(1, 240),
            frame_bytes: args.frame_bytes.clamp(8, 65_000),
        }
    }
}

/// Everything the app holds while running.
pub struct AppState {
    identity: DeviceIdentity,
    trust_path: PathBuf,
    control_port: u16,

    advertiser: Mutex<Option<Advertiser>>,
    control: Mutex<Option<ControlHandle>>,
    pending_pairing: Mutex<Option<oneshot::Sender<bool>>>,
    connection: Mutex<ConnectionState>,

    media: Arc<MediaSocket>,
    telemetry: Arc<Mutex<TelemetryCollector>>,
    peer_telemetry: Arc<Mutex<Option<TelemetryReport>>>,

    test_stream: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    test_report: Arc<Mutex<TestStreamReport>>,
}

impl AppState {
    /// Build the state, loading identity and binding the media socket.
    ///
    /// # Errors
    ///
    /// Fails if the config directory is unwritable or the media socket cannot bind.
    pub async fn new(config_dir: PathBuf) -> AppResult<Self> {
        let identity = DeviceIdentity::load_or_create(
            &config_dir.join("identity.json"),
            &unifiedstream_net::discovery::default_device_name(),
        )?;
        let media = MediaSocket::bind_on(DEFAULT_MEDIA_PORT).await?;

        Ok(Self {
            identity,
            trust_path: config_dir.join("trusted-devices.json"),
            control_port: DEFAULT_CONTROL_PORT,
            advertiser: Mutex::new(None),
            control: Mutex::new(None),
            pending_pairing: Mutex::new(None),
            connection: Mutex::new(ConnectionState::Idle),
            media: Arc::new(media),
            telemetry: Arc::new(Mutex::new(TelemetryCollector::new())),
            peer_telemetry: Arc::new(Mutex::new(None)),
            test_stream: Mutex::new(None),
            test_report: Arc::new(Mutex::new(TestStreamReport::default())),
        })
    }

    fn supported_caps() -> Vec<String> {
        // Advertised as the eventual surface; the toggles stay disabled until the media
        // changes land, so the phone can grey them out before connecting.
        caps::ALL.iter().map(|c| (*c).to_owned()).collect()
    }
}

/// Snapshot of everything the UI renders.
#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> AppResult<StatusSnapshot> {
    Ok(StatusSnapshot {
        device_name: state.identity.name.clone(),
        device_id: state.identity.id.clone(),
        advertising: state.advertiser.lock().await.is_some(),
        state: state.connection.lock().await.clone(),
        control_port: state.control_port,
        media_port: state.media.local_port(),
        caps: AppState::supported_caps(),
        test_stream_running: state.test_stream.lock().await.is_some(),
    })
}

/// Start the control listener and begin advertising over mDNS.
#[tauri::command]
pub async fn start_advertising(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<StatusSnapshot> {
    start_advertising_inner(&app, &state).await?;
    get_status(state).await
}

/// Start advertising outside a command, for the launch path.
///
/// # Errors
///
/// Fails if the control port cannot be bound or mDNS registration is refused.
pub async fn start_advertising_now(app: &AppHandle) -> AppResult<()> {
    let state = app.state::<Arc<AppState>>();
    let state = Arc::clone(&state);
    start_advertising_inner(app, &state).await
}

async fn start_advertising_inner(app: &AppHandle, state: &Arc<AppState>) -> AppResult<()> {
    if state.advertiser.lock().await.is_some() {
        return Ok(());
    }

    let config = ServerConfig {
        device_id: state.identity.id.clone(),
        device_name: state.identity.name.clone(),
        caps: AppState::supported_caps(),
        control_port: state.control_port,
        media_port: state.media.local_port(),
    };

    let (handle, events) =
        ControlServer::start(config, TrustStore::load(&state.trust_path)).await?;
    *state.control.lock().await = Some(handle);

    let advertiser = Advertiser::start(&AdvertiseConfig {
        identity: state.identity.clone(),
        control_port: state.control_port,
        caps: AppState::supported_caps(),
    })?;
    *state.advertiser.lock().await = Some(advertiser);

    set_state(app, state, ConnectionState::Discovering).await;
    spawn_event_pump(app.clone(), Arc::clone(state), events);
    spawn_telemetry_pump(app.clone(), Arc::clone(state));

    Ok(())
}

/// Stop advertising and drop any session.
#[tauri::command]
pub async fn stop_advertising(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<StatusSnapshot> {
    if let Some(advertiser) = state.advertiser.lock().await.take() {
        advertiser.shutdown()?;
    }
    if let Some(control) = state.control.lock().await.as_ref() {
        let _ = control.disconnect().await;
    }
    *state.control.lock().await = None;

    stop_test_stream_inner(&state).await;
    state.telemetry.lock().await.reset();
    *state.peer_telemetry.lock().await = None;

    set_state(&app, &state, ConnectionState::Idle).await;
    get_status(state).await
}

/// Answer the outstanding pairing prompt.
#[tauri::command]
pub async fn respond_pairing(accept: bool, state: State<'_, Arc<AppState>>) -> AppResult<()> {
    let responder = state
        .pending_pairing
        .lock()
        .await
        .take()
        .ok_or_else(|| AppError::State("no pairing request is waiting".to_owned()))?;

    responder
        .send(accept)
        .map_err(|_| AppError::State("the phone gave up waiting".to_owned()))
}

/// End the current session cleanly.
#[tauri::command]
pub async fn disconnect(app: AppHandle, state: State<'_, Arc<AppState>>) -> AppResult<()> {
    let control = state.control.lock().await.clone();
    if let Some(control) = control {
        control.disconnect().await?;
    }
    stop_test_stream_inner(&state).await;
    state.telemetry.lock().await.reset();
    *state.peer_telemetry.lock().await = None;
    set_state(&app, &state, ConnectionState::Discovering).await;
    Ok(())
}

/// Forget every paired device, so each is prompted for again.
#[tauri::command]
pub async fn forget_devices(state: State<'_, Arc<AppState>>) -> AppResult<()> {
    let mut store = TrustStore::load(&state.trust_path);
    for id in store.trusted_ids() {
        store.revoke(&id)?;
    }
    Ok(())
}

/// Start the synthetic test stream toward the connected phone.
#[tauri::command]
pub async fn start_test_stream(
    app: AppHandle,
    args: TestStreamArgs,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    let session_id = {
        let connection = state.connection.lock().await;
        connection
            .session_id()
            .ok_or_else(|| AppError::State("connect a phone first".to_owned()))?
    };

    stop_test_stream_inner(&state).await;
    *state.test_report.lock().await = TestStreamReport::default();

    let config: TestStreamConfig = args.into();
    let socket = Arc::clone(&state.media);
    let telemetry = Arc::clone(&state.telemetry);
    let report = Arc::clone(&state.test_report);

    let handle = tauri::async_runtime::spawn(async move {
        let mut generator = TestStreamGenerator::new(config);
        let mut sender = MediaSender::new(session_id);
        let mut ticker = tokio::time::interval(config.interval());
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;
            let frame = generator.next_frame();
            let datagrams = sender.frame(StreamId::TEST, &frame);
            let bytes: usize = datagrams.iter().map(Vec::len).sum();

            match socket.send_all(&datagrams).await {
                Ok(_) => telemetry.lock().await.record_sent(bytes as u64),
                Err(e) => {
                    tracing::warn!(error = %e, "test stream send failed");
                    break;
                }
            }

            let mut current = report.lock().await;
            current.verified = u64::from(generator.produced());
            let snapshot = *current;
            drop(current);
            let _ = app.emit(events::TEST_STREAM, snapshot);
        }
    });

    *state.test_stream.lock().await = Some(handle);
    Ok(())
}

/// Stop the synthetic test stream.
#[tauri::command]
pub async fn stop_test_stream(state: State<'_, Arc<AppState>>) -> AppResult<()> {
    stop_test_stream_inner(&state).await;
    Ok(())
}

async fn stop_test_stream_inner(state: &Arc<AppState>) {
    if let Some(handle) = state.test_stream.lock().await.take() {
        handle.abort();
    }
}

async fn set_state(app: &AppHandle, state: &Arc<AppState>, next: ConnectionState) {
    *state.connection.lock().await = next.clone();
    // Published immediately so the UI never has to infer status from anything else.
    let _ = app.emit(events::CONNECTION_STATE, next);
}

/// Forward control-channel events to the webview.
fn spawn_event_pump(
    app: AppHandle,
    state: Arc<AppState>,
    mut events: tokio::sync::mpsc::Receiver<ControlEvent>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                ControlEvent::PairingRequest {
                    device_id,
                    device_name,
                    respond,
                } => {
                    *state.pending_pairing.lock().await = Some(respond);
                    let _ = app.emit(
                        events::PAIRING_REQUEST,
                        PairingRequest {
                            device_id,
                            device_name,
                        },
                    );
                }

                ControlEvent::StateChanged(next) => {
                    if let Some(session_id) = next.session_id() {
                        // The phone advertised its media port in `hello`; pair it with the TCP
                        // peer's IP so the desktop can send without waiting for an inbound
                        // datagram to learn the address.
                        let control = state.control.lock().await.clone();
                        if let Some(control) = control {
                            if let Some(addr) = control.peer_media_addr().await {
                                state.media.set_peer(addr).await;
                                tracing::info!(%addr, "media peer set");
                            }
                        }
                        spawn_media_receiver(Arc::clone(&state), session_id);
                    }
                    if !next.is_connected() {
                        // A dead session must not leave stale numbers on the dashboard.
                        state.telemetry.lock().await.reset();
                        *state.peer_telemetry.lock().await = None;
                        stop_test_stream_inner(&state).await;
                    }
                    set_state(&app, &state, next).await;
                }

                ControlEvent::Telemetry(report) => {
                    *state.peer_telemetry.lock().await = Some(report);
                }

                ControlEvent::RttSample(ms) => {
                    state.telemetry.lock().await.record_rtt(ms);
                }

                ControlEvent::PeerLeft => {
                    stop_test_stream_inner(&state).await;
                    state.telemetry.lock().await.reset();
                    *state.peer_telemetry.lock().await = None;
                }

                _ => {}
            }
        }
    });
}

/// Emit a telemetry tick once per second, and report ours to the phone.
fn spawn_telemetry_pump(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(REPORT_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;

            let connected = state.connection.lock().await.is_connected();
            if !connected {
                continue;
            }

            let local = state.telemetry.lock().await.sample();
            let quality = state.telemetry.lock().await.quality();
            let peer = *state.peer_telemetry.lock().await;

            let _ = app.emit(
                events::TELEMETRY,
                TelemetryTick {
                    local,
                    peer,
                    quality,
                },
            );

            let control = state.control.lock().await.clone();
            if let Some(control) = control {
                let _ = control.send_telemetry(local).await;
            }
        }
    });
}

/// Receive loop for the media socket, feeding the demultiplexer and the verifier.
///
/// Started once a session exists so it can filter on the session id.
pub fn spawn_media_receiver(state: Arc<AppState>, session_id: u64) {
    let socket = Arc::clone(&state.media);
    let telemetry = Arc::clone(&state.telemetry);
    let report = Arc::clone(&state.test_report);

    tauri::async_runtime::spawn(async move {
        let mut demux = MediaDemux::new(session_id);
        demux.register(StreamId::TEST);
        let mut verifier = TestStreamVerifier::new();
        let mut buf = [0_u8; MAX_DATAGRAM];

        loop {
            let Ok((len, _from)) = socket.recv(&mut buf).await else {
                break;
            };
            let Some(datagram) = buf.get(..len) else {
                continue;
            };

            telemetry.lock().await.record_received(len as u64);

            if let Ok(frames) = demux.accept(datagram) {
                for frame in frames {
                    let _ = verifier.verify(&frame.payload);
                }
                let stats = demux.total_stats();
                telemetry
                    .lock()
                    .await
                    .record_packets(stats.received, stats.lost);
                *report.lock().await = verifier.report();
            }
        }
    });
}

/// Register the state and command handlers on the Tauri builder.
pub fn init(app: &AppHandle, state: Arc<AppState>) {
    app.manage(state);
}
