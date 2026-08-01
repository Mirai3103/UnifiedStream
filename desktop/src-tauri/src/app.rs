//! Application state and the Tauri command surface.
//!
//! Everything network-facing lives in `unifiedstream-net`; this layer only bridges it to the
//! webview — commands in, events out.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{oneshot, Mutex};

use unifiedstream_audio::{
    AudioCapture, AudioFormat, AudioSink, JitterBuffer, PipeWireSource, PipeWireSpeakerSink,
    SINK_NODE_ID,
};
use unifiedstream_net::control::{
    ControlEvent, ControlHandle, ControlServer, ServerConfig, TrustStore,
};
use unifiedstream_net::discovery::{AdvertiseConfig, Advertiser, DeviceIdentity};
use unifiedstream_net::protocol::{
    caps, AudioCodec, AudioParams, StreamId, StreamParams, StreamRefusal, TelemetryReport,
    VideoCodec, VideoParams,
};
use unifiedstream_net::session::ConnectionState;
use unifiedstream_net::telemetry::{LinkQuality, TelemetryCollector, REPORT_INTERVAL};
use unifiedstream_net::transport::{
    MediaDemux, MediaSender, MediaSocket, TestStreamConfig, TestStreamGenerator, TestStreamReport,
    TestStreamVerifier, MAX_DATAGRAM,
};
use unifiedstream_net::{DEFAULT_CONTROL_PORT, DEFAULT_MEDIA_PORT};
use unifiedstream_video::{V4l2LoopbackSink, VideoFormat, VideoSink, MODPROBE_HINT};

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
    /// Speaker stream status changed.
    pub const SPEAKER_STATUS: &str = "speaker-status";
    /// Live speaker level, ~15 Hz while audio flows.
    pub const SPEAKER_LEVEL: &str = "speaker-level";
    /// Camera sink status changed.
    pub const CAMERA_STATUS: &str = "camera-status";
    /// Delivered-frame counters, 1 Hz while the camera stream is active.
    pub const CAMERA_STATS: &str = "camera-stats";
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
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
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

/// One live-level sample sent to a UI meter — microphone or speaker. Both fields 0..1.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AudioLevel {
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

/// Camera sink status, as the UI renders it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CameraStatus {
    /// Whether the virtual camera device is attached and video may flow.
    pub active: bool,
    /// Why the camera is unavailable, when it is.
    pub error: Option<String>,
    /// The command that fixes a missing v4l2loopback module. Present exactly when the last
    /// failure was the module being absent, so the UI can offer it copyable.
    pub hint: Option<String>,
    /// Negotiated stream parameters while active.
    pub params: Option<VideoParams>,
    /// Device node the virtual camera writes to, e.g. `/dev/video10`.
    pub device: Option<String>,
}

/// Delivered-frame counters emitted at 1 Hz while the camera stream is active.
///
/// The UI derives its fps figure from `frames_written` advancing, so a stalled stream reads
/// as 0 fps — visibly distinct from a stopped one.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CameraStats {
    /// Frames decoded and written to the device since the stream started.
    pub frames_written: u64,
    /// Frames dropped because they failed to decode.
    pub decode_failures: u64,
    /// Frames written over the last second.
    pub fps: u32,
}

/// The camera sink and its bookkeeping. Mirrors [`MicSink`]: the desktop is the stream sink.
#[derive(Default)]
struct CameraSink {
    /// The v4l2loopback writer while a camera stream is accepted.
    sink: Option<V4l2LoopbackSink>,
    /// Emits [`CameraStats`] at 1 Hz while the stream is active.
    stats_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// What the UI shows.
    status: CameraStatus,
}

/// Speaker stream status, as the UI renders it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SpeakerStatus {
    /// Whether the stream is accepted and the virtual sink exists.
    pub active: bool,
    /// True between `stream_start` and the phone's answer.
    pub starting: bool,
    /// Whether frame transmission is withheld while the stream stays up.
    pub muted: bool,
    /// Whether the system default output is routed to the virtual sink.
    pub routed: bool,
    /// Why the speaker is unavailable, when it is.
    pub error: Option<String>,
    /// Negotiated stream parameters while active.
    pub params: Option<AudioParams>,
}

/// The speaker source and its bookkeeping. The desktop is the stream *source* here — the
/// inverse of the microphone — so this owns capture, not playback.
#[derive(Default)]
struct SpeakerSource {
    /// The PipeWire virtual sink while the speaker stream lives.
    sink: Option<PipeWireSpeakerSink>,
    /// Sends captured frames to the phone while the stream is accepted.
    send_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Fails the toggle visibly if the phone never answers `stream_start`.
    ack_timeout: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Queue the ack handler will connect to the send task on acceptance.
    pending_frames: Option<tokio::sync::mpsc::Receiver<Vec<i16>>>,
    /// What the UI shows.
    status: SpeakerStatus,
}

/// What `pactl` remembered before the virtual sink took over the default output.
///
/// Persisted *before* switching, so a crash while routed can still restore the user's device
/// on the next launch — a hijacked default that survives our death is unacceptable.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoutingMemo {
    /// `pactl` name of the sink that was the default before routing was enabled.
    previous_default: String,
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
    /// Speaker stream status.
    pub speaker: SpeakerStatus,
    /// Camera sink status.
    pub camera: CameraStatus,
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

    camera: Arc<Mutex<CameraSink>>,

    speaker: Arc<Mutex<SpeakerSource>>,
    /// Read by the speaker send task on every frame; a mute must not wait on a lock.
    speaker_muted: Arc<std::sync::atomic::AtomicBool>,
    /// Where the previous default output is remembered while routing is enabled.
    routing_memo_path: PathBuf,
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
            camera: Arc::new(Mutex::new(CameraSink::default())),
            speaker: Arc::new(Mutex::new(SpeakerSource::default())),
            speaker_muted: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            routing_memo_path: config_dir.join("speaker-routing.json"),
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
        speaker: state.speaker.lock().await.status.clone(),
        camera: state.camera.lock().await.status.clone(),
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

/// Ask the phone to start or stop its camera, protocol §3.9.4.
///
/// Identical in shape to the microphone toggle: the desktop is the sink, so the request is
/// honoured (or refused) by the phone, and the answer arrives through the camera status event.
#[tauri::command]
pub async fn set_camera_enabled(enabled: bool, state: State<'_, Arc<AppState>>) -> AppResult<()> {
    let control = state.control.lock().await.clone();
    let control = control.ok_or_else(|| AppError::State("connect a phone first".to_owned()))?;
    control.request_stream(StreamId::CAMERA, enabled).await?;
    Ok(())
}

/// Turn the speaker stream on or off from the desktop, protocol §3.9.
///
/// The desktop is the stream source: on turns into a `stream_start` (after the virtual sink is
/// stood up), off into a `stream_stop`. The phone's answer arrives asynchronously through the
/// speaker status event.
#[tauri::command]
pub async fn set_speaker_enabled(
    enabled: bool,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    if enabled {
        start_speaker_stream(&app, &state).await
    } else {
        stop_speaker(&app, &state, true).await;
        Ok(())
    }
}

/// Mute or unmute the speaker without touching the stream.
///
/// Mute stops frame transmission entirely — silence at PCM rates would still cost the full
/// bandwidth — and the phone's jitter buffer underruns into silence. Unmute resumes instantly
/// with no renegotiation.
#[tauri::command]
pub async fn set_speaker_muted(
    muted: bool,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    state
        .speaker_muted
        .store(muted, std::sync::atomic::Ordering::Relaxed);
    {
        let mut speaker = state.speaker.lock().await;
        speaker.status.muted = muted;
    }
    if muted {
        // The meter must read zero immediately, not freeze at its last value.
        let _ = app.emit(
            events::SPEAKER_LEVEL,
            AudioLevel {
                rms: 0.0,
                peak: 0.0,
            },
        );
    }
    emit_speaker_status(&app, &state).await;
    Ok(())
}

/// Route the system default output to the virtual sink, or restore the previous device.
#[tauri::command]
pub async fn set_speaker_routing(
    enabled: bool,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    if enabled {
        let active = state.speaker.lock().await.status.active;
        if !active {
            return Err(AppError::State("start the speaker first".to_owned()));
        }
        enable_routing(&state).await.map_err(AppError::State)?;
        state.speaker.lock().await.status.routed = true;
    } else {
        restore_routing(&state).await;
        state.speaker.lock().await.status.routed = false;
    }
    emit_speaker_status(&app, &state).await;
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
    stop_camera_sink(&app, &state).await;
    stop_speaker(&app, &state, false).await;
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
    stop_camera_sink(&app, &state).await;
    // The session is ending anyway; the implicit stream stop covers the peer's side.
    stop_speaker(&app, &state, false).await;
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
                        // Session end implies stream stop, protocol §3.9; the virtual sink
                        // must not outlive the phone it was feeding.
                        stop_speaker(&app, &state, false).await;
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
                    stop_speaker(&app, &state, false).await;
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
                    } else if stream == StreamId::CAMERA.get() {
                        stop_camera_sink(&app, &state).await;
                    } else if stream == StreamId::SPEAKER.get() {
                        // The peer ended it; no `stream_stop` is owed back.
                        stop_speaker(&app, &state, false).await;
                    }
                }

                ControlEvent::StreamAckReceived {
                    stream,
                    accepted,
                    reason,
                } => {
                    if stream == StreamId::SPEAKER.get() {
                        handle_speaker_ack(&app, &state, accepted, reason).await;
                    }
                }

                ControlEvent::StreamRequested { stream, active }
                    if stream == StreamId::SPEAKER.get() =>
                {
                    // The phone's toggle, protocol §3.9.4: honour it exactly as if the
                    // desktop user had toggled locally.
                    if active {
                        if let Err(e) = start_speaker_stream(&app, &state).await {
                            tracing::warn!(error = %e, "phone-requested speaker start failed");
                        }
                    } else {
                        stop_speaker(&app, &state, true).await;
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
    params: StreamParams,
    respond: oneshot::Sender<std::result::Result<(), StreamRefusal>>,
) {
    if stream == StreamId::MICROPHONE.get() {
        handle_mic_stream_start(app, state, params, respond).await;
    } else if stream == StreamId::CAMERA.get() {
        handle_camera_stream_start(app, state, params, respond).await;
    } else {
        let _ = respond.send(Err(StreamRefusal::UnsupportedStream));
    }
}

/// Stand up the PipeWire virtual source for a microphone `stream_start`.
async fn handle_mic_stream_start(
    app: &AppHandle,
    state: &Arc<AppState>,
    params: StreamParams,
    respond: oneshot::Sender<std::result::Result<(), StreamRefusal>>,
) {
    let Some(params) = params.as_audio() else {
        // Video parameters on an audio stream: a format this sink cannot play.
        let _ = respond.send(Err(StreamRefusal::UnsupportedCodec));
        return;
    };
    if params.codec != AudioCodec::PcmS16le {
        // Opus decode is the stretch task; until it lands, PCM is the one codec we play.
        let _ = respond.send(Err(StreamRefusal::UnsupportedCodec));
        return;
    }

    let format = AudioFormat {
        sample_rate: params.sample_rate,
        channels: params.channels,
        // Per channel, matching the AudioFormat contract; total_samples() re-derives the
        // full decoded frame length where needed.
        frame_samples: (params.sample_rate / 1000) as usize * params.frame_ms as usize,
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

// --- Camera (desktop as stream sink) ---------------------------------------------------------

/// Largest geometry the camera sink accepts.
///
/// Ties the offered resolution to the transport's per-frame reassembly cap: 1080p MJPEG
/// frames stay far below `MAX_FRAME_BYTES`, and nothing bigger has a UI to request it.
const MAX_CAMERA_PIXELS: u32 = 1920 * 1080;

/// Stand up the v4l2loopback virtual camera for a camera `stream_start`, protocol §7.
async fn handle_camera_stream_start(
    app: &AppHandle,
    state: &Arc<AppState>,
    params: StreamParams,
    respond: oneshot::Sender<std::result::Result<(), StreamRefusal>>,
) {
    let Some(video) = params.as_video() else {
        // Audio parameters on a video stream: a format this sink cannot present.
        let _ = respond.send(Err(StreamRefusal::UnsupportedCodec));
        return;
    };
    if video.codec != VideoCodec::Mjpeg {
        // MJPEG is the mandatory baseline and, until a later change, the only codec decoded.
        let _ = respond.send(Err(StreamRefusal::UnsupportedCodec));
        return;
    }
    if video.width == 0
        || video.height == 0
        || video.width % 2 != 0
        || video.height % 2 != 0
        || video.width.saturating_mul(video.height) > MAX_CAMERA_PIXELS
    {
        // Odd geometry cannot be 4:2:0 subsampled, and anything past 1080p risks frames the
        // transport's reassembly cap would refuse mid-stream. Refusing up front is kinder.
        let _ = respond.send(Err(StreamRefusal::Internal));
        return;
    }

    let format = VideoFormat {
        width: video.width,
        height: video.height,
        max_fps: video.max_fps,
    };
    let camera = Arc::clone(&state.camera);

    // Device discovery and format negotiation are /dev walks and kernel ioctls; keep them off
    // the event pump, like the PipeWire paths.
    let started = tauri::async_runtime::spawn_blocking(move || {
        let mut sink = V4l2LoopbackSink::new();
        sink.start(format).map(|()| sink)
    })
    .await;

    let mut camera_state = camera.lock().await;
    match started {
        Ok(Ok(sink)) => {
            if let Some(mut old) = camera_state.sink.take() {
                old.stop();
            }
            if let Some(task) = camera_state.stats_task.take() {
                task.abort();
            }
            let device = sink.device_path().map(|p| p.display().to_string());
            camera_state.sink = Some(sink);
            camera_state.status = CameraStatus {
                active: true,
                error: None,
                hint: None,
                params: Some(video),
                device,
            };
            camera_state.stats_task = Some(spawn_camera_stats_task(app.clone(), Arc::clone(state)));
            drop(camera_state);

            // Accept media for the stream only now that the device is attached.
            send_media_cmd(state, MediaCmd::Register(StreamId::CAMERA)).await;
            let _ = respond.send(Ok(()));
            emit_camera_status(app, state).await;
        }
        Ok(Err(e)) => {
            // The missing-module case gets the exact command to fix it, per the spec.
            let hint = matches!(e, unifiedstream_video::VideoError::Unavailable(_))
                .then(|| MODPROBE_HINT.to_owned());
            camera_state.status = CameraStatus {
                active: false,
                error: Some(e.to_string()),
                hint,
                params: None,
                device: None,
            };
            drop(camera_state);
            tracing::warn!(error = %e, "camera sink failed to start");
            let _ = respond.send(Err(StreamRefusal::Internal));
            emit_camera_status(app, state).await;
        }
        Err(join_error) => {
            drop(camera_state);
            tracing::error!(error = %join_error, "camera sink task panicked");
            let _ = respond.send(Err(StreamRefusal::Internal));
        }
    }
}

/// Emit [`CameraStats`] once per second while the camera stream is active.
///
/// Ends itself when the sink goes away, so a stopped stream leaves no ticking task behind.
fn spawn_camera_stats_task(
    app: AppHandle,
    state: Arc<AppState>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_written: u64 = 0;

        loop {
            ticker.tick().await;
            let counters = {
                let camera = state.camera.lock().await;
                camera
                    .sink
                    .as_ref()
                    .map(|sink| (sink.frames_written(), sink.decode_failures()))
            };
            let Some((written, failures)) = counters else {
                break;
            };

            #[allow(
                clippy::cast_possible_truncation,
                reason = "fps over one second is tiny"
            )]
            let fps = written.saturating_sub(last_written).min(1_000) as u32;
            last_written = written;
            let _ = app.emit(
                events::CAMERA_STATS,
                CameraStats {
                    frames_written: written,
                    decode_failures: failures,
                    fps,
                },
            );
        }
    })
}

/// Tear the camera sink down and tell the UI. Idempotent.
async fn stop_camera_sink(app: &AppHandle, state: &Arc<AppState>) {
    send_media_cmd(state, MediaCmd::Unregister(StreamId::CAMERA)).await;

    let sink = {
        let mut camera = state.camera.lock().await;
        if let Some(task) = camera.stats_task.take() {
            task.abort();
        }
        let had_sink = camera.sink.is_some();
        camera.status = CameraStatus::default();
        let sink = camera.sink.take();
        drop(camera);
        if !had_sink {
            return;
        }
        sink
    };

    // stop() joins the device worker; keep the block off the async pump.
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Some(mut sink) = sink {
            sink.stop();
        }
    })
    .await;

    emit_camera_status(app, state).await;
}

async fn emit_camera_status(app: &AppHandle, state: &Arc<AppState>) {
    let status = state.camera.lock().await.status.clone();
    let _ = app.emit(events::CAMERA_STATUS, status);
}

// --- Speaker (desktop as stream source) -----------------------------------------------------

/// Queue between the PipeWire capture callback and the async send task. Small and lossy on
/// purpose: audio is only useful fresh, and a stalled sender must not grow a latency debt.
const SPEAKER_FRAME_QUEUE: usize = 8;

/// How many speaker frames between level events: 3 x 20 ms ≈ 15 Hz.
const SPEAKER_LEVEL_EVERY_FRAMES: u32 = 3;

/// How long the phone may take to answer `stream_start` before the toggle fails visibly.
const SPEAKER_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);

/// Stand the virtual sink up, then announce the speaker stream to the phone.
///
/// The sink is created *before* `stream_start` goes out: a desktop that cannot capture must
/// not announce a stream, per the spec's "does not start the speaker stream" on audio-system
/// unavailability. Media flows only after the phone's accepting `stream_ack` arrives.
async fn start_speaker_stream(app: &AppHandle, state: &Arc<AppState>) -> AppResult<()> {
    let control = state.control.lock().await.clone();
    let control = control.ok_or_else(|| AppError::State("connect a phone first".to_owned()))?;
    if !state.connection.lock().await.is_connected() {
        return Err(AppError::State("connect a phone first".to_owned()));
    }

    {
        let speaker = state.speaker.lock().await;
        if speaker.status.active || speaker.status.starting {
            return Ok(());
        }
    }

    if !control
        .negotiated_caps()
        .await
        .iter()
        .any(|c| c == caps::SPEAKER)
    {
        set_speaker_error(app, state, "The phone does not accept a speaker stream").await;
        return Ok(());
    }

    // The capture callback runs on the PipeWire thread; frames cross into async land through
    // a bounded channel. try_send drops a frame when the sender is stalled, which is the
    // lossy-by-design behavior the jitter budget expects.
    let (frame_tx, frame_rx) = tokio::sync::mpsc::channel::<Vec<i16>>(SPEAKER_FRAME_QUEUE);
    let started = tauri::async_runtime::spawn_blocking(move || {
        let mut sink = PipeWireSpeakerSink::new(Box::new(move |frame| {
            let _ = frame_tx.try_send(frame);
        }));
        sink.start(AudioFormat::SPEAKER).map(|()| sink)
    })
    .await;

    let sink = match started {
        Ok(Ok(sink)) => sink,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "speaker sink failed to start");
            set_speaker_error(app, state, format!("Virtual sink unavailable: {e}")).await;
            return Ok(());
        }
        Err(join_error) => {
            tracing::error!(error = %join_error, "speaker sink task panicked");
            set_speaker_error(app, state, "Virtual sink unavailable").await;
            return Ok(());
        }
    };

    {
        let mut speaker = state.speaker.lock().await;
        speaker.sink = Some(sink);
        speaker.pending_frames = Some(frame_rx);
        speaker.status.starting = true;
        speaker.status.error = None;
    }

    if let Err(e) = control
        .start_stream(StreamId::SPEAKER, AudioParams::SPEAKER_PCM)
        .await
    {
        stop_speaker(app, state, false).await;
        return Err(e.into());
    }

    // A phone that never answers must fail the toggle visibly, not leave a spinner.
    let timeout = {
        let app = app.clone();
        let state = Arc::clone(state);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(SPEAKER_ACK_TIMEOUT).await;
            let starting = state.speaker.lock().await.status.starting;
            if starting {
                tracing::warn!("speaker stream_start was never answered");
                stop_speaker(&app, &state, false).await;
                set_speaker_error(&app, &state, "The phone did not answer").await;
            }
        })
    };
    state.speaker.lock().await.ack_timeout = Some(timeout);

    emit_speaker_status(app, state).await;
    Ok(())
}

/// Handle the phone's answer to our speaker `stream_start`.
async fn handle_speaker_ack(
    app: &AppHandle,
    state: &Arc<AppState>,
    accepted: bool,
    reason: Option<StreamRefusal>,
) {
    let (frame_rx, timeout) = {
        let mut speaker = state.speaker.lock().await;
        if !speaker.status.starting {
            return; // a stale ack for a stream generation already torn down
        }
        (speaker.pending_frames.take(), speaker.ack_timeout.take())
    };
    if let Some(timeout) = timeout {
        timeout.abort();
    }

    if !accepted {
        stop_speaker(app, state, false).await;
        set_speaker_error(app, state, speaker_refusal_text(reason)).await;
        return;
    }

    let session_id = state.connection.lock().await.session_id();
    let (Some(frame_rx), Some(session_id)) = (frame_rx, session_id) else {
        // Accepted, but the session died (or the queue was lost) in the meantime.
        stop_speaker(app, state, true).await;
        return;
    };

    let send_task = spawn_speaker_send_task(app.clone(), Arc::clone(state), session_id, frame_rx);
    {
        let mut speaker = state.speaker.lock().await;
        speaker.send_task = Some(send_task);
        speaker.status.active = true;
        speaker.status.starting = false;
        speaker.status.params = Some(AudioParams::SPEAKER_PCM);
        speaker.status.muted = state
            .speaker_muted
            .load(std::sync::atomic::Ordering::Relaxed);
    }
    tracing::info!("speaker stream accepted");
    emit_speaker_status(app, state).await;
}

/// Pop captured frames and put them on the wire as stream 3, protocol §6.
fn spawn_speaker_send_task(
    app: AppHandle,
    state: Arc<AppState>,
    session_id: u64,
    mut frames: tokio::sync::mpsc::Receiver<Vec<i16>>,
) -> tauri::async_runtime::JoinHandle<()> {
    let socket = Arc::clone(&state.media);
    let telemetry = Arc::clone(&state.telemetry);
    let muted = Arc::clone(&state.speaker_muted);

    tauri::async_runtime::spawn(async move {
        let mut sender = MediaSender::new(session_id);
        let mut frames_since_level: u32 = 0;

        while let Some(frame) = frames.recv().await {
            // Mute stops transmission entirely; the phone's jitter buffer underruns into
            // silence and unmute resumes with no renegotiation.
            if muted.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }

            if let Some(level) = audio_level(&frame) {
                frames_since_level += 1;
                if frames_since_level >= SPEAKER_LEVEL_EVERY_FRAMES {
                    frames_since_level = 0;
                    let _ = app.emit(events::SPEAKER_LEVEL, level);
                }
            }

            let payload = encode_s16le(&frame);
            let datagrams = sender.frame(StreamId::SPEAKER, &payload);
            let bytes: usize = datagrams.iter().map(Vec::len).sum();
            match socket.send_all(&datagrams).await {
                Ok(_) => telemetry.lock().await.record_sent(bytes as u64),
                Err(e) => {
                    tracing::warn!(error = %e, "speaker send failed");
                    break;
                }
            }
        }
    })
}

/// Tear the speaker source down and tell the UI. Idempotent.
///
/// `notify_peer` sends the `stream_stop` owed when this side ends a live stream; it is false
/// when the peer (or the session's death) already ended it.
async fn stop_speaker(app: &AppHandle, state: &Arc<AppState>, notify_peer: bool) {
    let (sink, was_live, was_routed) = {
        let mut speaker = state.speaker.lock().await;
        let was_live = speaker.status.active || speaker.status.starting;
        let was_routed = speaker.status.routed;
        if let Some(task) = speaker.send_task.take() {
            task.abort();
        }
        if let Some(timeout) = speaker.ack_timeout.take() {
            timeout.abort();
        }
        speaker.pending_frames = None;
        let sink = speaker.sink.take();
        speaker.status = SpeakerStatus {
            muted: state
                .speaker_muted
                .load(std::sync::atomic::Ordering::Relaxed),
            ..SpeakerStatus::default()
        };
        (sink, was_live, was_routed)
    };

    // The user's output device comes back before the node it was routed to disappears.
    if was_routed {
        restore_routing(state).await;
    }

    if let Some(mut sink) = sink {
        // stop() joins the PipeWire thread; keep the block off the async pump.
        let _ = tauri::async_runtime::spawn_blocking(move || sink.stop()).await;
    }

    if notify_peer && was_live {
        let control = state.control.lock().await.clone();
        if let Some(control) = control {
            let _ = control.stop_stream(StreamId::SPEAKER).await;
        }
    }

    let _ = app.emit(
        events::SPEAKER_LEVEL,
        AudioLevel {
            rms: 0.0,
            peak: 0.0,
        },
    );
    emit_speaker_status(app, state).await;
}

async fn set_speaker_error(app: &AppHandle, state: &Arc<AppState>, message: impl Into<String>) {
    {
        let mut speaker = state.speaker.lock().await;
        speaker.status.starting = false;
        speaker.status.error = Some(message.into());
    }
    emit_speaker_status(app, state).await;
}

async fn emit_speaker_status(app: &AppHandle, state: &Arc<AppState>) {
    let status = state.speaker.lock().await.status.clone();
    let _ = app.emit(events::SPEAKER_STATUS, status);
}

/// Human-readable text for a phone's refusal of the speaker stream.
fn speaker_refusal_text(reason: Option<StreamRefusal>) -> String {
    match reason {
        Some(StreamRefusal::NotNegotiated) => "The phone does not accept a speaker stream",
        Some(StreamRefusal::UnsupportedCodec) => "The phone cannot decode this audio format",
        Some(StreamRefusal::UnsupportedStream) => "The phone does not recognise the speaker stream",
        Some(StreamRefusal::Busy) => "The phone is busy",
        Some(StreamRefusal::Internal) => "The phone's audio output is unavailable",
        None | Some(_) => "The phone refused the speaker stream",
    }
    .to_owned()
}

/// Encode host-order samples as the wire format: signed 16-bit little-endian, protocol §6.1.
fn encode_s16le(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

// --- System audio routing -------------------------------------------------------------------

/// Run one `pactl` invocation, returning trimmed stdout.
async fn pactl(args: &[&str]) -> std::result::Result<String, String> {
    let output = tokio::process::Command::new("pactl")
        .args(args)
        .output()
        .await
        .map_err(|e| format!("could not run pactl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "pactl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Make the virtual sink the system default output, remembering the previous device first.
///
/// The memo is written *before* the switch: if this process dies while routed, the next launch
/// finds the file and restores the user's device (see [`sweep_stale_routing`]).
async fn enable_routing(state: &Arc<AppState>) -> std::result::Result<(), String> {
    let current = pactl(&["get-default-sink"]).await?;
    if current != SINK_NODE_ID {
        let memo = RoutingMemo {
            previous_default: current,
        };
        let text = serde_json::to_string(&memo)
            .map_err(|e| format!("could not encode routing memo: {e}"))?;
        if let Some(parent) = state.routing_memo_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&state.routing_memo_path, text)
            .map_err(|e| format!("could not remember the previous output: {e}"))?;
    }
    pactl(&["set-default-sink", SINK_NODE_ID]).await?;
    tracing::info!("system audio routed to the virtual sink");
    Ok(())
}

/// Restore the remembered default output and forget the memo. Idempotent.
async fn restore_routing(state: &Arc<AppState>) {
    let path = state.routing_memo_path.clone();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return; // no memo: routing was never enabled, or already restored
    };
    if let Ok(memo) = serde_json::from_str::<RoutingMemo>(&text) {
        match pactl(&["set-default-sink", &memo.previous_default]).await {
            Ok(_) => tracing::info!(sink = %memo.previous_default, "default output restored"),
            Err(e) => tracing::warn!(error = %e, "could not restore the default output"),
        }
    }
    let _ = std::fs::remove_file(&path);
}

/// Repair a stale routing takeover left by an unclean exit.
///
/// Every clean path deletes the memo, so its presence at startup means the last run died while
/// the virtual sink held the default output. The sink itself died with that process — PipeWire
/// already fell back to a real device — but the *remembered* default may still name our node,
/// so the persisted previous device is put back.
pub async fn sweep_stale_routing(state: &Arc<AppState>) {
    if state.routing_memo_path.is_file() {
        tracing::warn!("found a stale routing takeover from a previous run; restoring");
        restore_routing(state).await;
    }
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
    let camera = Arc::clone(&state.camera);

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
                        let delivered = !frames.is_empty();
                        for frame in frames {
                            if stream == StreamId::MICROPHONE {
                                let samples = decode_s16le(&frame.payload);
                                if let Some(level) = audio_level(&samples) {
                                    frames_since_level += 1;
                                    if frames_since_level >= MIC_LEVEL_EVERY_FRAMES {
                                        frames_since_level = 0;
                                        let _ = app.emit(events::MIC_LEVEL, level);
                                    }
                                }
                                mic_buffer.push(samples);
                            } else if stream == StreamId::CAMERA {
                                // Decode and device writes happen on the sink's worker; this
                                // only queues, so the receive loop never blocks on video.
                                let mut camera_sink = camera.lock().await;
                                if let Some(sink) = camera_sink.sink.as_mut() {
                                    sink.push_frame(frame.payload);
                                }
                            } else {
                                let _ = verifier.verify(&frame.payload);
                            }
                        }
                        let stats = demux.total_stats();
                        telemetry
                            .lock()
                            .await
                            .record_packets(stats.received, stats.lost);
                        if stream == StreamId::TEST && delivered {
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

/// RMS and peak of a frame of samples, both 0..1. `None` for an empty frame.
fn audio_level(samples: &[i16]) -> Option<AudioLevel> {
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
    #[allow(
        clippy::cast_possible_truncation,
        reason = "both values are within 0..=1"
    )]
    Some(AudioLevel {
        rms: (rms as f32).clamp(0.0, 1.0),
        peak: (f64::from(peak) / f64::from(i16::MAX)).clamp(0.0, 1.0) as f32,
    })
}

/// Register the state and command handlers on the Tauri builder.
pub fn init(app: &AppHandle, state: Arc<AppState>) {
    app.manage(state);
}
