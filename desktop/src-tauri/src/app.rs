//! Application state and the Tauri command surface.
//!
//! Everything network-facing lives in `unifiedstream-net`; this layer only bridges it to the
//! webview — commands in, events out.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{oneshot, Mutex};

use unifiedstream_audio::{AudioFormat, AudioSink, JitterBuffer, PipeWireSource};
use unifiedstream_net::control::{
    ControlEvent, ControlHandle, ControlServer, ServerConfig, TrustStore,
};
use unifiedstream_net::discovery::{AdvertiseConfig, Advertiser, DeviceIdentity};
use unifiedstream_net::protocol::{
    caps, AudioCodec, AudioParams, StreamId, StreamRefusal, TelemetryReport,
};
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
    /// Microphone sink status changed.
    pub const MIC_STATUS: &str = "mic-status";
    /// Live microphone level, ~15 Hz while audio flows.
    pub const MIC_LEVEL: &str = "mic-level";
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

/// Microphone sink status, as the UI renders it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MicStatus {
    /// Whether the virtual source node exists and audio may flow.
    pub active: bool,
    /// Why the sink is unavailable, when it is.
    pub error: Option<String>,
    /// Negotiated stream parameters while active.
    pub params: Option<AudioParams>,
}

/// One live-level sample sent to the UI meter. Both fields 0..1.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MicLevel {
    /// Root-mean-square level of the most recent frames.
    pub rms: f32,
    /// Peak magnitude of the most recent frames.
    pub peak: f32,
}

/// The microphone sink and its bookkeeping.
#[derive(Default)]
struct MicSink {
    /// The PipeWire source while a mic stream is accepted.
    sink: Option<PipeWireSource>,
    /// What the UI shows.
    status: MicStatus,
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
    /// Microphone sink status.
    pub mic: MicStatus,
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

    mic: Arc<Mutex<MicSink>>,
    /// Shared with the PipeWire process callback; the receive path pushes decoded frames.
    mic_buffer: Arc<JitterBuffer>,
    /// The one live media receiver. Replaced — not accumulated — on session change.
    media_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// Register/unregister requests for the receiver's demultiplexer.
    media_cmds: Mutex<Option<tokio::sync::mpsc::Sender<MediaCmd>>>,
}

/// Demultiplexer changes sent into the live receiver task.
#[derive(Debug, Clone, Copy)]
enum MediaCmd {
    /// Begin accepting a stream.
    Register(StreamId),
    /// Stop accepting a stream; later packets are counted as unknown-stream drops.
    Unregister(StreamId),
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
            mic: Arc::new(Mutex::new(MicSink::default())),
            mic_buffer: Arc::new(JitterBuffer::default()),
            media_task: Mutex::new(None),
            media_cmds: Mutex::new(None),
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
        mic: state.mic.lock().await.status.clone(),
    })
}

/// Ask the phone to start or stop its microphone, protocol §3.9.4.
///
/// The result of the request arrives asynchronously: the phone answers with `stream_start`
/// (or a refusal), and the UI hears about it through the mic status event.
#[tauri::command]
pub async fn set_mic_enabled(enabled: bool, state: State<'_, Arc<AppState>>) -> AppResult<()> {
    let control = state.control.lock().await.clone();
    let control = control.ok_or_else(|| AppError::State("connect a phone first".to_owned()))?;
    control
        .request_stream(StreamId::MICROPHONE, enabled)
        .await?;
    Ok(())
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
    stop_mic_sink(&app, &state).await;
    if let Some(task) = state.media_task.lock().await.take() {
        task.abort();
    }
    *state.media_cmds.lock().await = None;
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
    stop_mic_sink(&app, &state).await;
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
                        spawn_media_receiver(app.clone(), Arc::clone(&state), session_id).await;
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

                ControlEvent::StreamStartRequested {
                    stream,
                    params,
                    respond,
                } => {
                    handle_stream_start(&app, &state, stream, params, respond).await;
                }

                ControlEvent::StreamStopped { stream } => {
                    if stream == StreamId::MICROPHONE.get() {
                        stop_mic_sink(&app, &state).await;
                    }
                }

                _ => {}
            }
        }
    });
}

/// Answer a phone's `stream_start`: stand the sink up, then let the control layer ack.
async fn handle_stream_start(
    app: &AppHandle,
    state: &Arc<AppState>,
    stream: u8,
    params: AudioParams,
    respond: oneshot::Sender<std::result::Result<(), StreamRefusal>>,
) {
    if stream != StreamId::MICROPHONE.get() {
        // The camera capability is advertised but its sink is a later change.
        let _ = respond.send(Err(StreamRefusal::UnsupportedStream));
        return;
    }
    if params.codec != AudioCodec::PcmS16le {
        // Opus decode is the stretch task; until it lands, PCM is the one codec we play.
        let _ = respond.send(Err(StreamRefusal::UnsupportedCodec));
        return;
    }

    let format = AudioFormat {
        sample_rate: params.sample_rate,
        channels: params.channels,
        frame_samples: (params.sample_rate / 1000) as usize
            * params.frame_ms as usize
            * usize::from(params.channels),
    };

    // Replace any previous sink: a restart re-negotiates parameters.
    let mic = Arc::clone(&state.mic);
    let buffer = Arc::clone(&state.mic_buffer);

    // Sink creation talks to the PipeWire daemon and can block for seconds when it is
    // wedged; keep that off the event pump so control traffic stays responsive.
    let started = tauri::async_runtime::spawn_blocking(move || {
        let mut sink = PipeWireSource::new(buffer);
        sink.start(format).map(|()| sink)
    })
    .await;

    let mut mic_state = mic.lock().await;
    match started {
        Ok(Ok(sink)) => {
            if let Some(mut old) = mic_state.sink.take() {
                old.stop();
            }
            mic_state.sink = Some(sink);
            mic_state.status = MicStatus {
                active: true,
                error: None,
                params: Some(params),
            };
            drop(mic_state);

            // Accept media for the stream only now that the sink exists.
            send_media_cmd(state, MediaCmd::Register(StreamId::MICROPHONE)).await;
            let _ = respond.send(Ok(()));
            emit_mic_status(app, state).await;
        }
        Ok(Err(e)) => {
            mic_state.status = MicStatus {
                active: false,
                error: Some(format!("Virtual source unavailable: {e}")),
                params: None,
            };
            drop(mic_state);
            tracing::warn!(error = %e, "mic sink failed to start");
            let _ = respond.send(Err(StreamRefusal::Internal));
            emit_mic_status(app, state).await;
        }
        Err(join_error) => {
            drop(mic_state);
            tracing::error!(error = %join_error, "mic sink task panicked");
            let _ = respond.send(Err(StreamRefusal::Internal));
        }
    }
}

/// Tear the mic sink down and tell the UI. Idempotent.
async fn stop_mic_sink(app: &AppHandle, state: &Arc<AppState>) {
    send_media_cmd(state, MediaCmd::Unregister(StreamId::MICROPHONE)).await;

    let sink = {
        let mut mic = state.mic.lock().await;
        let had_sink = mic.sink.is_some();
        mic.status = MicStatus::default();
        let sink = mic.sink.take();
        drop(mic);
        if !had_sink {
            return;
        }
        sink
    };

    // stop() joins the PipeWire thread; keep the block off the async pump.
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Some(mut sink) = sink {
            sink.stop();
        }
    })
    .await;

    emit_mic_status(app, state).await;
}

async fn send_media_cmd(state: &Arc<AppState>, cmd: MediaCmd) {
    let sender = state.media_cmds.lock().await.clone();
    if let Some(sender) = sender {
        let _ = sender.send(cmd).await;
    }
}

async fn emit_mic_status(app: &AppHandle, state: &Arc<AppState>) {
    let status = state.mic.lock().await.status.clone();
    let _ = app.emit(events::MIC_STATUS, status);
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

/// How many mic frames between level events: 3 x 20 ms ≈ 15 Hz, smooth for a meter without
/// hammering the webview with events.
const MIC_LEVEL_EVERY_FRAMES: u32 = 3;

/// Receive loop for the media socket: demultiplexes the synthetic test stream and the
/// microphone, and applies register/unregister commands from the stream lifecycle.
///
/// Started once a session exists so it can filter on the session id. Replaces — never
/// accumulates — the previous receiver, so a resumed session does not leave two tasks
/// racing on one socket.
pub async fn spawn_media_receiver(app: AppHandle, state: Arc<AppState>, session_id: u64) {
    let socket = Arc::clone(&state.media);
    let telemetry = Arc::clone(&state.telemetry);
    let report = Arc::clone(&state.test_report);
    let mic_buffer = Arc::clone(&state.mic_buffer);

    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<MediaCmd>(8);

    let task = tauri::async_runtime::spawn(async move {
        let mut demux = MediaDemux::new(session_id);
        demux.register(StreamId::TEST);
        let mut verifier = TestStreamVerifier::new();
        let mut buf = [0_u8; MAX_DATAGRAM];
        let mut frames_since_level: u32 = 0;

        loop {
            tokio::select! {
                received = socket.recv(&mut buf) => {
                    let Ok((len, _from)) = received else {
                        break;
                    };
                    let Some(datagram) = buf.get(..len) else {
                        continue;
                    };

                    telemetry.lock().await.record_received(len as u64);

                    // The demux returns frames without their stream id; read it from the
                    // header before handing the datagram over.
                    let stream = unifiedstream_net::MediaHeader::decode(datagram)
                        .map(|h| h.stream)
                        .unwrap_or(StreamId::TEST);

                    if let Ok(frames) = demux.accept(datagram) {
                        for frame in &frames {
                            if stream == StreamId::MICROPHONE {
                                let samples = decode_s16le(&frame.payload);
                                if let Some(level) = mic_level(&samples) {
                                    frames_since_level += 1;
                                    if frames_since_level >= MIC_LEVEL_EVERY_FRAMES {
                                        frames_since_level = 0;
                                        let _ = app.emit(events::MIC_LEVEL, level);
                                    }
                                }
                                mic_buffer.push(samples);
                            } else {
                                let _ = verifier.verify(&frame.payload);
                            }
                        }
                        let stats = demux.total_stats();
                        telemetry
                            .lock()
                            .await
                            .record_packets(stats.received, stats.lost);
                        if stream == StreamId::TEST && !frames.is_empty() {
                            *report.lock().await = verifier.report();
                        }
                    }
                }

                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(MediaCmd::Register(stream)) => demux.register(stream),
                        Some(MediaCmd::Unregister(stream)) => demux.unregister(stream),
                        None => break,
                    }
                }
            }
        }
    });

    let previous = {
        let mut slot = state.media_task.lock().await;
        slot.replace(task)
    };
    if let Some(previous) = previous {
        previous.abort();
    }
    *state.media_cmds.lock().await = Some(cmd_tx);
}

/// Decode a §5.1 PCM S16LE payload into host-order samples.
fn decode_s16le(payload: &[u8]) -> Vec<i16> {
    payload
        .chunks_exact(2)
        .map(|pair| {
            if let [lo, hi] = pair {
                i16::from_le_bytes([*lo, *hi])
            } else {
                0 // unreachable: chunks_exact(2) yields only pairs
            }
        })
        .collect()
}

/// RMS and peak of a decoded frame, both 0..1. `None` for an empty frame.
fn mic_level(samples: &[i16]) -> Option<MicLevel> {
    if samples.is_empty() {
        return None;
    }
    let mut sum_squares = 0.0_f64;
    let mut peak = 0_i32;
    for &sample in samples {
        let value = i32::from(sample);
        sum_squares += f64::from(value) * f64::from(value);
        peak = peak.max(value.abs());
    }
    #[allow(clippy::cast_precision_loss, reason = "sample counts are tiny")]
    let rms = (sum_squares / samples.len() as f64).sqrt() / f64::from(i16::MAX);
    #[allow(clippy::cast_possible_truncation, reason = "both values are within 0..=1")]
    Some(MicLevel {
        rms: (rms as f32).clamp(0.0, 1.0),
        peak: (f64::from(peak) / f64::from(i16::MAX)).clamp(0.0, 1.0) as f32,
    })
}

/// Register the state and command handlers on the Tauri builder.
pub fn init(app: &AppHandle, state: Arc<AppState>) {
    app.manage(state);
}
