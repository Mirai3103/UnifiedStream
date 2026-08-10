//! Windows resolution of the audio platform factories: the WASAPI loopback capture behind the
//! speaker, and the WASAPI render sink behind the microphone.
//!
//! Partial only in routing, which does not exist as a concept on this platform at all.
//!
//! The capture reads the endpoint the user already chose, with
//! `AUDCLNT_STREAMFLAGS_LOOPBACK`. It creates no device, changes no setting, and audio keeps
//! playing on the user's own speakers while it is transmitted. Three things about that are
//! easy to miss and are load-bearing:
//!
//! - **Loopback does not combine with event callbacks**, so the stream is polled from a
//!   dedicated COM-initialized thread rather than driven by an event.
//! - **An idle render endpoint produces no loopback packets at all** — not silence, nothing. A
//!   second render client on the same endpoint plays silence for the lifetime of the capture, so
//!   the phone's jitter buffer keeps being fed while the PC is quiet.
//! - **The endpoint decides the format.** `GetMixFormat` reports what the shared mixer runs at,
//!   which is commonly 32-bit float and not always 48 kHz; [`crate::AudioConverter`] turns it
//!   into the negotiated wire format so neither the protocol nor the phone has to care.
//!
//! The sink renders into an endpoint **somebody else's driver created**: VB-CABLE's `CABLE Input`,
//! whose other half applications select as an ordinary microphone under the name `CABLE Output`.
//! Nothing here creates a device or destroys one; the endpoint exists whether or not a stream
//! does. Two things about *that* are load-bearing, and both are the opposite of the capture's:
//!
//! - **The destination is resolved by identity and never followed.** Rendering the phone's audio
//!   into whatever endpoint happens to be default would play the user's own voice aloud on their
//!   speakers, and feed it back to the phone where the speaker stream is also up. The capture
//!   follows the user's choice because that is its whole job; the sink must not follow anything.
//! - **Rendering is event-driven, not polled.** The constraint that forces the capture to poll is
//!   loopback's, not WASAPI's, and it does not apply to an ordinary render client.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use windows::core::{implement, GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IAudioCaptureClient, IAudioClient, IAudioRenderClient,
    IMMDevice, IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl,
    MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_E_DEVICE_INVALIDATED,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    DEVICE_STATE, DEVICE_STATEMASK_ALL, DEVICE_STATE_ACTIVE, DEVICE_STATE_DISABLED,
    DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW,
    WaitForSingleObject,
};
use windows::Win32::System::Variant::VT_BLOB;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

use crate::{
    AudioCapture, AudioConverter, AudioError, AudioFormat, AudioRouting, AudioSink, EndpointFormat,
    FrameCallback, FrameChunker, JitterBuffer, RenderConverter, SampleType, SourceFormat,
};

/// How long [`AudioCapture::start`] waits for the capture thread to report success or failure, so
/// a machine with no usable output endpoint surfaces as a refusal rather than a silent stream.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);

/// Endpoint buffer to ask both clients for, in 100-nanosecond units: 100 ms. Generous, because
/// the loop polls rather than being woken, and an overrun loses audio.
const BUFFER_DURATION_100NS: i64 = 1_000_000;

/// Used when the device will not report its period. Half a conventional 10 ms period.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Bounds on the polling interval, whatever the device claims its period is.
const MIN_POLL_INTERVAL: Duration = Duration::from_millis(1);
const MAX_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// How long the single recovery path keeps trying before it gives up and fails the stream.
/// Twenty attempts a tenth of a second apart: a device change settles well inside that, and a
/// machine with no output device at all is not going to grow one.
const REOPEN_ATTEMPTS: u32 = 20;
const REOPEN_BACKOFF: Duration = Duration::from_millis(100);

/// `WAVE_FORMAT_EXTENSIBLE`, and the leading word of the two `KSDATAFORMAT_SUBTYPE` GUIDs a
/// shared-mode mixer reports. Spelled out rather than pulling in two more `windows` features for
/// three numbers.
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const WAVE_FORMAT_PCM: u32 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u32 = 3;

/// WASAPI loopback capture of the current default render endpoint.
pub fn audio_capture(on_frame: FrameCallback) -> Box<dyn AudioCapture> {
    Box::new(WasapiLoopback {
        on_frame: Some(on_frame),
        worker: None,
    })
}

/// WASAPI rendering into VB-CABLE's `CABLE Input`, whose other half applications select as the
/// microphone `CABLE Output`.
pub fn audio_sink(buffer: Arc<JitterBuffer>) -> Box<dyn AudioSink> {
    Box::new(WasapiRender {
        buffer,
        worker: None,
    })
}

/// Absent, not unsupported. Capturing system audio here means reading the endpoint the user
/// already chose, so there is no default output to take over, nothing to remember, and nothing
/// to repair — the desktop offers no control rather than one that could only fail.
pub fn audio_routing(_config_dir: &Path) -> Option<Box<dyn AudioRouting>> {
    None
}

/// An [`AudioCapture`] over WASAPI loopback.
///
/// Holds a join handle and a stop flag and nothing else. That is what makes it `Send` with no
/// COM object crossing a thread boundary — every interface pointer is created, used, and
/// released on the capture thread — and it mirrors how the PipeWire implementations are
/// structured, so the application layer sees identical lifecycles on both platforms.
struct WasapiLoopback {
    on_frame: Option<FrameCallback>,
    worker: Option<Worker>,
}

struct Worker {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl AudioCapture for WasapiLoopback {
    fn start(&mut self, format: AudioFormat) -> Result<(), AudioError> {
        if self.worker.is_some() {
            return Ok(());
        }
        if format.channels == 0 || format.sample_rate == 0 || format.frame_samples == 0 {
            return Err(AudioError::UnsupportedFormat(format!(
                "{}ch @ {}Hz",
                format.channels, format.sample_rate
            )));
        }
        let Some(on_frame) = self.on_frame.take() else {
            return Err(AudioError::Unavailable(
                "capture was already consumed by a previous start".to_owned(),
            ));
        };

        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let thread_stop = Arc::clone(&stop);

        let thread = std::thread::Builder::new()
            .name("wasapi-loopback".to_owned())
            .spawn(move || run(on_frame, format, &thread_stop, &ready_tx))
            .map_err(|e| AudioError::Unavailable(format!("could not spawn audio thread: {e}")))?;

        // Wait for the first endpoint to open, so a machine with no usable default output
        // refuses the stream instead of accepting one that never carries audio.
        match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(())) => {
                self.worker = Some(Worker { stop, thread });
                tracing::info!("WASAPI loopback capture started");
                Ok(())
            }
            Ok(Err(message)) => {
                let _ = thread.join();
                Err(AudioError::Unavailable(message))
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                let _ = thread.join();
                Err(AudioError::Unavailable(
                    "timed out opening the default output device".to_owned(),
                ))
            }
        }
    }

    fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop.store(true, Ordering::Release);
            // The thread releases both clients and uninitializes COM on its way out; joining is
            // what makes that ordering observable to the caller.
            if worker.thread.join().is_err() {
                tracing::warn!("WASAPI capture thread panicked during shutdown");
            }
            tracing::info!("WASAPI loopback capture stopped");
        }
    }
}

impl Drop for WasapiLoopback {
    fn drop(&mut self) {
        // Neither client may outlive the capture: a loopback client left open holds the audio
        // engine awake, and the keep-alive would keep the endpoint rendering silence forever.
        self.stop();
    }
}

/// Everything that happens on the dedicated capture thread, COM initialization included.
fn run(
    on_frame: FrameCallback,
    format: AudioFormat,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<(), String>>,
) {
    // SAFETY: paired with the CoUninitialize below, on this thread, with every interface
    // pointer created and dropped in between.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() {
        let _ = ready.send(Err(format!(
            "COM could not be initialized: {initialized:?}"
        )));
        return;
    }

    if let Err(message) = capture(on_frame, format, stop, ready) {
        tracing::error!(error = %message, "WASAPI loopback capture failed");
        // Before the ready signal this send *is* the failure report; after it the receiver is
        // gone and the send is a harmless no-op. Either way the frame callback is dropped as
        // this function returns, which closes the desktop's frame channel and is what turns a
        // capture that cannot recover into a visible speaker error rather than a dead stream.
        let _ = ready.send(Err(message));
    }

    // SAFETY: paired with the CoInitializeEx above; `capture` has returned, so every interface
    // pointer it created has been dropped.
    unsafe { CoUninitialize() };
}

/// The capture proper: open, report, poll, recover, tear down.
fn capture(
    mut on_frame: FrameCallback,
    format: AudioFormat,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    // SAFETY: COM is initialized on this thread for the whole of this function.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|e| format!("no audio endpoint enumerator: {e}"))?;

    let changed = Arc::new(AtomicBool::new(false));
    // A guard rather than a bare call, so the callback is unregistered on every path out of this
    // function — including the one where the very first open fails. Declared after `enumerator`
    // and before `session`, which is the order they have to be torn down in.
    let _registration = NotificationRegistration::register(&enumerator, Arc::clone(&changed));

    let _mmcss = MmcssThread::register();

    let mut session = Some(Session::open(&enumerator, format).map_err(OpenFailure::into_message)?);
    ready
        .send(Ok(()))
        .map_err(|e| format!("startup reporter gone: {e}"))?;

    let mut chunker = FrameChunker::new(format.total_samples());
    let mut reopen = false;

    let outcome = loop {
        if stop.load(Ordering::Acquire) {
            break Ok(());
        }

        // The notification flag and an invalidated client mean the same thing, and are answered
        // the same way: the endpoint being captured is no longer the one that should be.
        if changed.swap(false, Ordering::AcqRel) {
            reopen = true;
        }
        if reopen {
            reopen = false;
            // Both clients go before the replacements open — the new endpoint may be the old
            // one, and a stale loopback client on it is not competition the new one should have.
            session = None;
            // A partial frame from the old endpoint must not be spliced onto the front of the
            // new one, which is very likely running at a different rate.
            chunker.clear();
            match open_with_retry(&enumerator, format, stop) {
                Ok(Some(fresh)) => {
                    session = Some(fresh);
                    tracing::info!("capture re-opened on the current default output");
                }
                Ok(None) => break Ok(()),
                Err(e) => break Err(e),
            }
        }

        let Some(active) = session.as_mut() else {
            break Ok(());
        };
        let interval = active.poll_interval;
        match active.pump(&mut chunker, &mut on_frame) {
            Ok(()) => {}
            Err(PumpOutcome::Reopen) => {
                reopen = true;
                continue;
            }
            Err(PumpOutcome::Fatal(message)) => break Err(message),
        }

        std::thread::sleep(interval);
    };

    // Both clients stop here rather than at the end of scope, so they are gone before the
    // enumerator that resolved them and before the callback watching it.
    drop(session);
    outcome
}

/// The endpoint notification callback, registered for as long as this value lives.
struct NotificationRegistration<'a> {
    enumerator: &'a IMMDeviceEnumerator,
    watcher: IMMNotificationClient,
}

impl<'a> NotificationRegistration<'a> {
    /// Registers a watcher that raises `changed` when the default output changes.
    ///
    /// `None` when registration fails, which is not fatal: capture still follows a *disappearing*
    /// endpoint through `AUDCLNT_E_DEVICE_INVALIDATED`. It would only stop noticing a deliberate
    /// device change.
    fn register(enumerator: &'a IMMDeviceEnumerator, changed: Arc<AtomicBool>) -> Option<Self> {
        let watcher: IMMNotificationClient = DefaultOutputWatcher { changed }.into();
        // SAFETY: `watcher` is owned by the returned guard, which unregisters it before dropping
        // it, so the API never holds a pointer to a released object.
        match unsafe { enumerator.RegisterEndpointNotificationCallback(&watcher) } {
            Ok(()) => Some(Self {
                enumerator,
                watcher,
            }),
            Err(e) => {
                tracing::warn!(error = %e, "default-output changes will not be followed");
                None
            }
        }
    }
}

impl Drop for NotificationRegistration<'_> {
    fn drop(&mut self) {
        // SAFETY: the same enumerator and the same watcher this guard registered, both still
        // alive; the watcher is released immediately afterwards.
        unsafe {
            let _ = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.watcher);
        }
    }
}

/// Re-resolve the default endpoint and open on it, retrying while the change settles.
///
/// `Ok(None)` means a stop was requested while retrying; `Err` means no output device could be
/// captured at all, which fails the stream rather than leaving it silently dead.
fn open_with_retry(
    enumerator: &IMMDeviceEnumerator,
    format: AudioFormat,
    stop: &AtomicBool,
) -> Result<Option<Session>, String> {
    let mut last = String::new();
    for attempt in 1..=REOPEN_ATTEMPTS {
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        match Session::open(enumerator, format) {
            Ok(session) => return Ok(Some(session)),
            // The user selecting the microphone's own destination as their system output, mid
            // stream. Waiting cannot resolve it and the capture must stop rather than go on
            // transmitting what it finds there, so this returns instead of retrying.
            Err(OpenFailure::Conflict(message)) => return Err(message),
            Err(OpenFailure::Transient(e)) => {
                tracing::debug!(attempt, error = %e, "re-opening capture");
                last = e;
            }
        }
        std::thread::sleep(REOPEN_BACKOFF);
    }
    Err(format!(
        "could not re-open capture on any output device: {last}"
    ))
}

/// One open capture: the loopback client, the silent keep-alive on the same endpoint, and the
/// converter built for that endpoint's mix format.
struct Session {
    capture_client: IAudioClient,
    capture: IAudioCaptureClient,
    keepalive_client: IAudioClient,
    keepalive: IAudioRenderClient,
    /// The keep-alive's whole buffer, in frames — what it is topped back up to each pass.
    keepalive_frames: u32,
    converter: AudioConverter,
    /// Bytes one source frame occupies, for sizing a captured packet.
    source_frame_bytes: usize,
    poll_interval: Duration,
}

/// Why a poll pass stopped.
enum PumpOutcome {
    /// The endpoint went away. Answered by the one recovery path.
    Reopen,
    /// Anything else, which fails the stream visibly rather than stalling it.
    Fatal(String),
}

fn classify(error: &windows::core::Error) -> PumpOutcome {
    if error.code() == AUDCLNT_E_DEVICE_INVALIDATED {
        PumpOutcome::Reopen
    } else {
        PumpOutcome::Fatal(format!("system audio capture failed: {error}"))
    }
}

/// Why opening the capture failed.
///
/// The distinction is about whether waiting helps. A device change in flight settles on its own,
/// which is what the retry loop is for; the microphone conflict does not settle until the user
/// changes something, so retrying it twenty times only delays the message that tells them to.
enum OpenFailure {
    /// The endpoint to be captured is the one the microphone renders into.
    Conflict(String),
    /// Anything else, which may still settle.
    Transient(String),
}

impl OpenFailure {
    fn into_message(self) -> String {
        match self {
            Self::Conflict(message) | Self::Transient(message) => message,
        }
    }
}

/// Whether the endpoint about to be loopback-captured is the microphone's destination.
///
/// Pure, and separated from the COM that supplies its two arguments, so the comparison itself is
/// covered on a runner with no audio device. IDs are compared case-insensitively: MMDevice hands
/// the same endpoint's ID back in whatever case it was stored in, and a case difference between
/// two spellings of one device is not a difference between two devices.
fn conflicts_with_microphone(capture_id: &str, microphone_id: Option<&str>) -> bool {
    microphone_id.is_some_and(|microphone| microphone.eq_ignore_ascii_case(capture_id))
}

/// The refusal message for the feedback loop, naming the device and what resolves it.
fn feedback_conflict_message(label: &str) -> String {
    format!(
        "the system output is set to \"{label}\", which is where the microphone plays the phone's \
         audio — capturing it would send the phone its own microphone back as a howl. Choose a \
         real playback device as the system output, then start the speaker again"
    )
}

/// Compare the endpoint about to be captured against the microphone's destination.
///
/// Windows-only by construction, and that is the whole of task "inert elsewhere": a platform whose
/// virtual microphone is not also an entry in the user's output list cannot reach this
/// configuration, so the check lives in this module and no portable code knows it exists. Nothing
/// compiles into the Linux build, and nothing runs there.
///
/// # Safety
///
/// COM must be initialized on the calling thread.
unsafe fn feedback_conflict(
    enumerator: &IMMDeviceEnumerator,
    device: &IMMDevice,
) -> Option<String> {
    // SAFETY: a device the caller owns; the returned block is freed by `own`.
    let capture_id = unsafe { device.GetId() }
        .ok()
        .map(|id| unsafe { own(id) })?;
    // SAFETY: the caller's contract. `None` when there is no unambiguous microphone destination —
    // which is also when there is no microphone to conflict with.
    let microphone = unsafe { microphone_endpoint_id(enumerator) };
    if !conflicts_with_microphone(&capture_id, microphone.as_deref()) {
        return None;
    }
    // SAFETY: the same device, read on the same thread.
    let label = unsafe { describe_endpoint(device) }
        .map_or_else(|| "the virtual microphone's device".to_owned(), |e| e.label);
    tracing::warn!(device = %label, "refusing to capture the microphone's own destination");
    Some(feedback_conflict_message(&label))
}

impl Session {
    /// Open loopback capture and the keep-alive on the current default render endpoint.
    fn open(enumerator: &IMMDeviceEnumerator, format: AudioFormat) -> Result<Self, OpenFailure> {
        // SAFETY: COM methods on an interface this function owns, on a COM-initialized thread.
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .map_err(|e| OpenFailure::Transient(format!("no default output device: {e}")))?;

        // Before anything is opened. Every part of this configuration is supported and the user
        // may have arrived at it deliberately, so it is detected and reported rather than
        // prevented or silently corrected — and the refusal falls on the speaker because audio the
        // desktop is rendering for the microphone is not system audio under any reading. Refusing
        // the microphone instead would leave a working speaker quietly transmitting the user's own
        // echo, which is the harder fault to diagnose and still sounds like a network problem.
        //
        // SAFETY: the enumerator and the device above, on the thread that owns both.
        if let Some(message) = unsafe { feedback_conflict(enumerator, &device) } {
            return Err(OpenFailure::Conflict(message));
        }

        Self::open_on(&device, format).map_err(OpenFailure::Transient)
    }

    /// Open both clients on an endpoint the caller has already resolved and cleared.
    fn open_on(device: &IMMDevice, format: AudioFormat) -> Result<Self, String> {
        // SAFETY: every call below is a COM method on an interface this function owns, on a
        // thread where COM is initialized. `mix` is owned by `MixFormat` for the whole block.
        unsafe {
            let capture_client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("could not open the default output device: {e}"))?;

            let mix = MixFormat::of(&capture_client)?;
            let source = source_format(mix.as_ptr())?;
            let source_frame_bytes = mix.block_align();
            let converter = AudioConverter::new(source, format)
                .map_err(|e| format!("the device's format cannot be converted: {e}"))?;

            capture_client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    BUFFER_DURATION_100NS,
                    0,
                    mix.as_ptr(),
                    None,
                )
                .map_err(|e| format!("the device refused loopback capture: {e}"))?;
            let capture: IAudioCaptureClient = capture_client
                .GetService()
                .map_err(|e| format!("no capture service on the device: {e}"))?;

            // The keep-alive: a second, ordinary render client on the same endpoint. Without it
            // an idle endpoint delivers no loopback packets at all, and the stream would stall
            // exactly when the PC is quiet — which looks like a network fault.
            let keepalive_client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("could not open the keep-alive stream: {e}"))?;
            keepalive_client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    0,
                    BUFFER_DURATION_100NS,
                    0,
                    mix.as_ptr(),
                    None,
                )
                .map_err(|e| format!("the device refused the keep-alive stream: {e}"))?;
            let keepalive: IAudioRenderClient = keepalive_client
                .GetService()
                .map_err(|e| format!("no render service on the device: {e}"))?;
            let keepalive_frames = keepalive_client
                .GetBufferSize()
                .map_err(|e| format!("could not size the keep-alive buffer: {e}"))?;

            // Filled before starting so the engine has something to render from the first
            // period. Released with the silent flag, so the buffer is never read and there is
            // nothing to write into it.
            keepalive
                .GetBuffer(keepalive_frames)
                .map_err(|e| format!("could not fill the keep-alive buffer: {e}"))?;
            keepalive
                .ReleaseBuffer(keepalive_frames, silent_flag())
                .map_err(|e| format!("could not fill the keep-alive buffer: {e}"))?;

            keepalive_client
                .Start()
                .map_err(|e| format!("could not start the keep-alive stream: {e}"))?;
            capture_client
                .Start()
                .map_err(|e| format!("could not start loopback capture: {e}"))?;

            let mut period = 0_i64;
            let poll_interval = match capture_client.GetDevicePeriod(Some(&mut period), None) {
                // Half the device period, per the polling pattern loopback requires. The period
                // is in 100-nanosecond units.
                Ok(()) if period > 0 => Duration::from_nanos(period.unsigned_abs() * 100 / 2)
                    .clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL),
                _ => DEFAULT_POLL_INTERVAL,
            };

            tracing::info!(
                sample_rate = source.sample_rate,
                channels = source.channels,
                resampling = converter.is_resampling(),
                poll_ms = poll_interval.as_millis(),
                "loopback capture open on the default output"
            );

            Ok(Self {
                capture_client,
                capture,
                keepalive_client,
                keepalive,
                keepalive_frames,
                converter,
                source_frame_bytes,
                poll_interval,
            })
        }
    }

    /// Drain every packet the endpoint has ready, then top the keep-alive back up.
    fn pump(
        &mut self,
        chunker: &mut FrameChunker,
        on_frame: &mut FrameCallback,
    ) -> Result<(), PumpOutcome> {
        let Self {
            capture,
            converter,
            source_frame_bytes,
            ..
        } = self;

        loop {
            // SAFETY: `capture` is this session's own capture client, used only on this thread.
            let packet = unsafe { capture.GetNextPacketSize() }.map_err(|e| classify(&e))?;
            if packet == 0 {
                break;
            }

            let mut data: *mut u8 = std::ptr::null_mut();
            let mut frames: u32 = 0;
            let mut flags: u32 = 0;
            // SAFETY: the three out-parameters are live for the call, and the buffer they
            // describe stays valid until the ReleaseBuffer below.
            unsafe { capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) }
                .map_err(|e| classify(&e))?;

            if frames > 0 {
                // The API requires the silent flag be honoured by writing zeros rather than
                // reading the buffer — and this is the path that runs whenever only the
                // keep-alive is playing, which is to say whenever the PC is quiet.
                let converted = if flags & silent_flag() != 0 || data.is_null() {
                    converter.convert_silence(frames as usize)
                } else {
                    // SAFETY: WASAPI guarantees `frames * nBlockAlign` readable bytes at `data`
                    // until ReleaseBuffer, which has not been called yet.
                    let bytes = unsafe {
                        std::slice::from_raw_parts(data, frames as usize * *source_frame_bytes)
                    };
                    converter.convert(bytes)
                };
                chunker.push(converted, &mut *on_frame);
            }

            // SAFETY: releases exactly what GetBuffer reported, before the next pass.
            unsafe { capture.ReleaseBuffer(frames) }.map_err(|e| classify(&e))?;
        }

        self.top_up_keepalive()
    }

    /// Refill the keep-alive's buffer with silence, so the endpoint never goes idle.
    fn top_up_keepalive(&mut self) -> Result<(), PumpOutcome> {
        // SAFETY: this session's own render client, on this thread.
        let padding =
            unsafe { self.keepalive_client.GetCurrentPadding() }.map_err(|e| classify(&e))?;
        let available = self.keepalive_frames.saturating_sub(padding);
        if available == 0 {
            return Ok(());
        }
        // SAFETY: `available` frames are free by the padding just read, and the buffer is handed
        // straight back flagged silent, so its contents are never read.
        unsafe {
            self.keepalive
                .GetBuffer(available)
                .map_err(|e| classify(&e))?;
            self.keepalive
                .ReleaseBuffer(available, silent_flag())
                .map_err(|e| classify(&e))
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: both clients belong to this session and are stopped on the thread that started
        // them. Failures here are not actionable — the interfaces are about to be released.
        unsafe {
            let _ = self.capture_client.Stop();
            let _ = self.keepalive_client.Stop();
        }
    }
}

/// `AUDCLNT_BUFFERFLAGS_SILENT` as the `u32` both buffer APIs take.
fn silent_flag() -> u32 {
    AUDCLNT_BUFFERFLAGS_SILENT.0.unsigned_abs()
}

/// The mix format `GetMixFormat` allocated, freed on drop.
///
/// A guard rather than a bare pointer because it is read, then passed to two `Initialize` calls,
/// any of which may fail — and the block-allocated format leaks on every early return otherwise.
struct MixFormat(*mut WAVEFORMATEX);

impl MixFormat {
    fn of(client: &IAudioClient) -> Result<Self, String> {
        // SAFETY: a COM method on a client this thread owns; the returned block becomes ours.
        let format = unsafe { client.GetMixFormat() }
            .map_err(|e| format!("could not read the device's format: {e}"))?;
        if format.is_null() {
            return Err("the device reported no mix format".to_owned());
        }
        Ok(Self(format))
    }

    fn as_ptr(&self) -> *const WAVEFORMATEX {
        self.0
    }

    /// Bytes one interleaved source frame occupies, across all channels.
    fn block_align(&self) -> usize {
        // SAFETY: a non-null block from GetMixFormat; read unaligned because WAVEFORMATEX is
        // declared packed.
        usize::from(unsafe { std::ptr::read_unaligned(self.0) }.nBlockAlign)
    }
}

impl Drop for MixFormat {
    fn drop(&mut self) {
        // SAFETY: allocated by GetMixFormat, freed once, and never used again.
        unsafe { CoTaskMemFree(Some(self.0.cast())) };
    }
}

/// Read an endpoint's format into the portable rate, channel count, and sample type.
///
/// # Safety
///
/// `mix` must point at a valid `WAVEFORMATEX`, and at a `WAVEFORMATEXTENSIBLE` when it declares
/// itself extensible.
unsafe fn wave_format(mix: *const WAVEFORMATEX) -> Result<(u32, u16, SampleType), String> {
    // Read unaligned: both structures are declared packed, so a reference to a field would be
    // unsound even though the block itself is suitably aligned in practice.
    let base = unsafe { std::ptr::read_unaligned(mix) };
    let tag = if base.wFormatTag == WAVE_FORMAT_EXTENSIBLE && base.cbSize >= 22 {
        let extended = unsafe { std::ptr::read_unaligned(mix.cast::<WAVEFORMATEXTENSIBLE>()) };
        // The subtype GUIDs differ only in their first word: 1 is PCM, 3 is IEEE float.
        extended.SubFormat.data1
    } else {
        u32::from(base.wFormatTag)
    };

    let sample_type = match (tag, base.wBitsPerSample) {
        (WAVE_FORMAT_IEEE_FLOAT, 32) => SampleType::Float32,
        (WAVE_FORMAT_PCM, 16) => SampleType::Int16,
        (WAVE_FORMAT_PCM, 32) => SampleType::Int32,
        (tag, bits) => {
            return Err(format!(
                "the device's format is not convertible: type {tag}, {bits}-bit"
            ))
        }
    };

    Ok((base.nSamplesPerSec, base.nChannels, sample_type))
}

/// The same format, described as where captured samples come *from*.
///
/// # Safety
///
/// As [`wave_format`].
unsafe fn source_format(mix: *const WAVEFORMATEX) -> Result<SourceFormat, String> {
    // SAFETY: the caller's contract.
    let (sample_rate, channels, sample_type) = unsafe { wave_format(mix) }?;
    Ok(SourceFormat {
        sample_rate,
        channels,
        sample_type,
    })
}

/// The same format, described as where rendered samples are going *to*.
///
/// # Safety
///
/// As [`wave_format`].
unsafe fn endpoint_format(mix: *const WAVEFORMATEX) -> Result<EndpointFormat, String> {
    // SAFETY: the caller's contract.
    let (sample_rate, channels, sample_type) = unsafe { wave_format(mix) }?;
    Ok(EndpointFormat {
        sample_rate,
        channels,
        sample_type,
    })
}

/// Watches for the user selecting a different default output.
///
/// The callback runs on a system thread. It sets one atomic and returns: it resolves no device,
/// touches no COM object the capture thread owns, and takes no lock the capture thread holds.
/// Anything more is a deadlock waiting for a user who changes their output device while audio is
/// flowing.
#[implement(IMMNotificationClient)]
struct DefaultOutputWatcher {
    changed: Arc<AtomicBool>,
}

#[allow(non_snake_case, reason = "the COM interface names its own methods")]
impl IMMNotificationClient_Impl for DefaultOutputWatcher_Impl {
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _device: &PCWSTR,
    ) -> windows::core::Result<()> {
        if flow == eRender && role == eConsole {
            self.changed.store(true, Ordering::Release);
        }
        Ok(())
    }

    // A device appearing, vanishing, or changing state is only interesting when it changes which
    // endpoint is the default, and that arrives as the notification above.
    fn OnDeviceStateChanged(
        &self,
        _device: &PCWSTR,
        _state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceAdded(&self, _device: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _device: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _device: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

// --- Endpoint resolution ---------------------------------------------------------------------

/// VB-CABLE's adapter name, as its INF writes it into `PKEY_DeviceInterface_FriendlyName`.
///
/// The one name in the endpoint's properties the user cannot change. Renaming a device in the
/// Sound control panel writes the new text into `PKEY_Device_DeviceDesc`, and
/// `PKEY_Device_FriendlyName` is composed as `DeviceDesc (adapter name)` — so a rename moves both
/// of those and neither can be a match key. The adapter name comes from the INF rather than from
/// the endpoint, and survives.
const VB_CABLE_ADAPTER: &str = "VB-Audio Virtual Cable";

/// VB-CABLE's hardware identifier, as it appears inside the driver-identity property.
///
/// Corroboration only. `PKEY_DeviceInterface_FriendlyName` is a documented property key; the
/// driver-identity key below is not, so a match may be *strengthened* by this and must never
/// depend on it alone — a Windows release that stops populating it would otherwise turn a working
/// installation into "not installed".
const VB_CABLE_HARDWARE_ID: &str = "VBAudioVACWDM";

/// `KSNODETYPE_SPEAKER`, the jack subtype VB-CABLE's `CABLE Input` presents.
///
/// **This is what stops `CABLE In 16ch` being selected in place of `CABLE Input`.** Those two
/// endpoints carry the same adapter name, the same INF, the same driver version, the same topology
/// filter, the same declared channel count, and the same mix format; measured on 3.3.1.7, the only
/// things that separate them are this subtype — `CABLE In 16ch` presents `KSNODETYPE_LINE_CONNECTOR`
/// — and the trailing pin index of the KS filter name, which is rejected as a match key because
/// choosing the lower index is choosing by position. MMDevice specifies no enumeration order.
///
/// Rendering into the wrong one **fails silently**: no API returns an error, `CABLE Output` simply
/// stays quiet, and what reaches a user is "the microphone connects and nobody can hear me".
///
/// Provisional by construction, and deliberately so: a release this rule is wrong about produces
/// zero candidates or several, and [`Unresolved`] refuses both with a log naming every device
/// considered. The job of this constant is not to be right about every VB-CABLE version — it
/// cannot be — but to fail loudly on the versions it is wrong about.
const KSNODETYPE_SPEAKER: GUID = GUID::from_u128(0xdff2_1ce1_f70f_11d0_b917_00a0_c922_3196);

/// Widest device format a candidate may declare and still be the cable's input.
///
/// Discriminates nothing on 3.3.1.7 — `CABLE In 16ch` declares two channels, the same as
/// `CABLE Input`, and its name describes what its filter can be configured for rather than what
/// the endpoint reports. Kept regardless: it costs one comparison, and a release that does expose
/// a genuinely wide render endpoint is exactly the case it was reached for.
const CABLE_INPUT_MAX_CHANNELS: u16 = 2;

/// Where a user who has no VB-CABLE gets one. Named in the refusal, because a message that says
/// what is missing without saying where to get it is half a remedy.
const VB_CABLE_DOWNLOAD: &str = "https://vb-audio.com/Cable/";

/// A property key, spelled out rather than pulling in two more `windows` features for a GUID and
/// an integer — the same trade this module already makes for the wave format tags.
const fn pkey(fmtid: u128, pid: u32) -> PROPERTYKEY {
    PROPERTYKEY {
        fmtid: GUID::from_u128(fmtid),
        pid,
    }
}

/// `PKEY_Device_FriendlyName`: `CABLE Input (VB-Audio Virtual Cable)`. Display text only.
const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = pkey(0xa45c_254e_df1c_4efd_8020_67d1_46a8_50e0, 14);

/// `PKEY_Device_DeviceDesc`: `CABLE Input`. Where a rename lands, so it is logged and never
/// matched on.
const PKEY_DEVICE_DESC: PROPERTYKEY = pkey(0xa45c_254e_df1c_4efd_8020_67d1_46a8_50e0, 2);

/// `PKEY_DeviceInterface_FriendlyName`: the adapter name, from the INF.
const PKEY_INTERFACE_FRIENDLY_NAME: PROPERTYKEY =
    pkey(0xb3f8_fa53_0004_438e_9003_51a4_6e13_9bfc, 6);

/// The driver-identity property: `oem13.inf:…:VBCableInst.NTamd64:3.3.1.7:VBAudioVACWDM`.
/// Undocumented, hence corroboration rather than evidence.
const PKEY_DEVICE_DRIVER_IDENTITY: PROPERTYKEY = pkey(0x83da_6326_97a6_4088_9453_a192_3f57_3b29, 3);

/// `PKEY_AudioEngine_DeviceFormat`: the endpoint's format, whose channel count is the secondary
/// discrimination above.
const PKEY_AUDIOENGINE_DEVICE_FORMAT: PROPERTYKEY =
    pkey(0xf19f_064d_082c_4e27_bc73_6882_a1bb_8e4c, 0);

/// `PKEY_AudioEndpoint_JackSubType`: the `KSNODETYPE_*` the driver's topology gives this endpoint,
/// and the discrimination that actually separates VB-CABLE's two render endpoints.
const PKEY_AUDIOENDPOINT_JACK_SUBTYPE: PROPERTYKEY =
    pkey(0x1da5_d803_d492_4edd_8c23_e0c0_ffee_7f0e, 8);

/// One enumerated render endpoint, reduced to the properties the candidate filter examines.
///
/// Portable on purpose: [`examine`] and [`choose`] take these and nothing else, so every outcome
/// this resolver can reach is unit-testable on a CI runner with no audio device — which is every
/// runner this project has.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderEndpoint {
    /// The endpoint ID, which is **not an identity**.
    ///
    /// Its GUID is minted the first time the endpoint is enumerated on a given machine, and
    /// VB-CABLE's hardware identifier lives in an entirely different property. So this can never
    /// become a constant in this repository, and no amount of it looking stable on one machine
    /// makes it stable across installs. It is used for exactly two things: handing back to
    /// `IMMDeviceEnumerator::GetDevice` to re-open *this* endpoint, and comparing two endpoints
    /// *within one running system* for the feedback-loop guard.
    id: String,
    /// `PKEY_DeviceInterface_FriendlyName` — the match key.
    adapter: String,
    /// `PKEY_Device_DeviceDesc`, logged so a rejected candidate is recognisable in a user's log.
    description: String,
    /// `PKEY_Device_FriendlyName` — what the user sees in their device picker, and therefore the
    /// platform-supplied display label this resolver hands back.
    label: String,
    /// The driver-identity property, empty when it could not be read.
    driver: String,
    /// The `KSNODETYPE_*` this endpoint's jack presents, as text. Empty when unreadable, which is
    /// deliberately *not* a rejection — see [`examine`].
    jack: String,
    /// Channels the device format declares, `0` when the format could not be read.
    channels: u16,
    /// `DEVICE_STATE` as enumerated.
    state: u32,
}

/// Why one enumerated endpoint is or is not the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Survived every rule.
    Candidate,
    /// Not this component's endpoint at all.
    ForeignAdapter,
    /// Carries the adapter name, but the driver-identity property names a different driver.
    ForeignDriver,
    /// This component's endpoint, but disabled, unplugged, or not present.
    Inactive,
    /// This component's endpoint, but its jack is not a speaker — the `CABLE In 16ch` case.
    ForeignJack,
    /// This component's endpoint, but too wide to be the cable's input.
    TooManyChannels,
}

impl Verdict {
    /// The phrase the candidate log uses, so a support report reads as prose.
    const fn reason(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::ForeignAdapter => "rejected: not a VB-CABLE adapter",
            Self::ForeignDriver => "rejected: adapter name matches but the driver does not",
            Self::Inactive => "rejected: not active",
            Self::ForeignJack => {
                "rejected: its jack is not a speaker, so it is not the cable input"
            }
            Self::TooManyChannels => "rejected: too many channels to be the cable input",
        }
    }
}

/// Why the destination could not be resolved.
///
/// Four outcomes rather than one, because they have four different remedies and a user cannot act
/// on "the microphone failed". A wrong match would be worse than any of these and is not
/// representable: [`choose`] refuses on anything other than exactly one survivor.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unresolved {
    /// Nothing carrying VB-CABLE's adapter name was enumerated. The component is not installed.
    NotInstalled,
    /// The endpoint is there and is not usable: disabled in Sound settings, or unplugged.
    Unusable { label: String, state: u32 },
    /// Endpoints carrying the adapter name exist, but the discrimination did not leave exactly
    /// one. Distinct from [`Self::NotInstalled`] because "installed and unidentifiable" and "not
    /// installed" send a user to two different places.
    Indistinguishable { matched: usize, survivors: usize },
    /// The endpoint was identified and could not be opened.
    Unopenable { label: String, error: String },
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled => write!(
                f,
                "VB-CABLE is not installed — the microphone renders into its \"CABLE Input\" \
                 playback device, and applications select \"CABLE Output\" as their microphone. \
                 Install VB-CABLE from {VB_CABLE_DOWNLOAD} and start the microphone again"
            ),
            Self::Unusable { label, state } => write!(
                f,
                "VB-CABLE's \"{label}\" playback device is installed but {}: enable it under \
                 Sound settings → More sound settings → Playback, then start the microphone again",
                describe_state(*state)
            ),
            Self::Indistinguishable { matched, survivors } => write!(
                f,
                "VB-CABLE is installed but its \"CABLE Input\" playback device could not be \
                 identified: {matched} VB-Audio playback devices were considered and {survivors} \
                 could not be told apart. This is a VB-CABLE release this build has not seen; the \
                 log lists every device considered and why each was rejected"
            ),
            Self::Unopenable { label, error } => write!(
                f,
                "VB-CABLE's \"{label}\" playback device could not be opened: {error}"
            ),
        }
    }
}

/// A `DEVICE_STATE` in the words a user would use for it.
fn describe_state(state: u32) -> &'static str {
    match DEVICE_STATE(state) {
        DEVICE_STATE_DISABLED => "disabled",
        DEVICE_STATE_UNPLUGGED => "unplugged",
        DEVICE_STATE_NOTPRESENT => "not present",
        _ => "not usable",
    }
}

/// Apply the two match rules to one endpoint.
fn examine(endpoint: &RenderEndpoint) -> Verdict {
    // Rule one: the family, from the one property a Sound control panel rename does not touch.
    if !endpoint.adapter.eq_ignore_ascii_case(VB_CABLE_ADAPTER) {
        return Verdict::ForeignAdapter;
    }
    // Corroboration, and only where the undocumented property was readable.
    if !endpoint.driver.is_empty() && !endpoint.driver.contains(VB_CABLE_HARDWARE_ID) {
        return Verdict::ForeignDriver;
    }
    if DEVICE_STATE(endpoint.state) != DEVICE_STATE_ACTIVE {
        return Verdict::Inactive;
    }
    // Rule two: which of this driver's render endpoints. Both sub-rules treat an unreadable
    // property as *not* a rejection — an unreadable property means the rule cannot be applied, so
    // the endpoint stays a candidate and any resulting ambiguity is refused below rather than
    // guessed through. Refusing here instead would turn a Windows release that stopped populating
    // a property into "VB-CABLE is not installed" on a machine where it plainly is.
    if !endpoint.jack.is_empty() && !is_speaker_jack(&endpoint.jack) {
        return Verdict::ForeignJack;
    }
    if endpoint.channels > CABLE_INPUT_MAX_CHANNELS {
        return Verdict::TooManyChannels;
    }
    Verdict::Candidate
}

/// Whether a jack subtype, as the property store renders it, is `KSNODETYPE_SPEAKER`.
///
/// Compared as text rather than as a `GUID` because the property arrives as a formatted string and
/// converting it back would only add a parser that can fail; the braces and the case are what a
/// `PROPVARIANT` of a GUID always prints, and both ends of this comparison are formatted the same
/// way.
fn is_speaker_jack(jack: &str) -> bool {
    jack.trim_matches(|c| c == '{' || c == '}')
        .eq_ignore_ascii_case(&format!("{KSNODETYPE_SPEAKER:?}"))
}

/// Choose the destination from an enumeration, or say why there is not exactly one.
///
/// Logs every endpoint it considered and the verdict on each, so a wrong or empty result on a
/// VB-CABLE version this project has not seen is diagnosable from a user's log instead of being
/// invisible. **Never takes the first of several**, and never substitutes the default render
/// endpoint for a failure: the natural defensive reflex — fall back to the default output so that
/// something works — produces exactly the failure this feature must not have, which is the user's
/// own voice played aloud in the room the PC is in.
fn choose(endpoints: Vec<RenderEndpoint>) -> Result<RenderEndpoint, Unresolved> {
    let mut candidates = Vec::new();
    let mut matched = 0_usize;
    let mut inactive: Option<RenderEndpoint> = None;

    for endpoint in endpoints {
        let verdict = examine(&endpoint);
        if verdict == Verdict::ForeignAdapter {
            // Every other render endpoint on the machine. Logged at trace so a full picture is
            // still obtainable, without burying the interesting lines.
            tracing::trace!(
                label = %endpoint.label,
                adapter = %endpoint.adapter,
                "render endpoint considered: {}",
                verdict.reason()
            );
            continue;
        }
        tracing::info!(
            label = %endpoint.label,
            description = %endpoint.description,
            adapter = %endpoint.adapter,
            driver = %endpoint.driver,
            jack = %endpoint.jack,
            channels = endpoint.channels,
            state = endpoint.state,
            id = %endpoint.id,
            "VB-CABLE render endpoint considered: {}",
            verdict.reason()
        );
        matched += 1;
        match verdict {
            Verdict::Candidate => candidates.push(endpoint),
            Verdict::Inactive => {
                let _ = inactive.get_or_insert(endpoint);
            }
            Verdict::ForeignJack
            | Verdict::TooManyChannels
            | Verdict::ForeignAdapter
            | Verdict::ForeignDriver => {}
        }
    }

    let survivors = candidates.len();
    if survivors == 1 {
        if let Some(chosen) = candidates.into_iter().next() {
            return Ok(chosen);
        }
    }
    if matched == 0 {
        return Err(Unresolved::NotInstalled);
    }
    if survivors == 0 {
        if let Some(endpoint) = inactive {
            return Err(Unresolved::Unusable {
                label: endpoint.label,
                state: endpoint.state,
            });
        }
    }
    Err(Unresolved::Indistinguishable { matched, survivors })
}

/// Read every render endpoint's properties, in whatever state it is in.
///
/// Disabled and unplugged endpoints are enumerated too, which is what lets "installed but turned
/// off" be reported as its own outcome instead of looking like "not installed".
///
/// # Safety
///
/// COM must be initialized on the calling thread, and `enumerator` must be valid on it.
unsafe fn enumerate_render_endpoints(
    enumerator: &IMMDeviceEnumerator,
) -> Result<Vec<RenderEndpoint>, String> {
    // SAFETY: a COM method on an interface the caller owns, on a COM-initialized thread.
    let collection =
        unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE(DEVICE_STATEMASK_ALL)) }
            .map_err(|e| format!("could not enumerate playback devices: {e}"))?;
    // SAFETY: the collection just returned.
    let count = unsafe { collection.GetCount() }
        .map_err(|e| format!("could not count playback devices: {e}"))?;

    let mut endpoints = Vec::with_capacity(count as usize);
    for index in 0..count {
        // SAFETY: `index` is below the count the collection reported.
        let Ok(device) = (unsafe { collection.Item(index) }) else {
            continue;
        };
        // SAFETY: a device from this collection, used and released on this thread.
        if let Some(endpoint) = unsafe { describe_endpoint(&device) } {
            endpoints.push(endpoint);
        }
    }
    Ok(endpoints)
}

/// Read one endpoint's properties, or `None` when even its ID could not be obtained.
///
/// # Safety
///
/// COM must be initialized on the calling thread, and `device` must be valid on it.
unsafe fn describe_endpoint(device: &IMMDevice) -> Option<RenderEndpoint> {
    // SAFETY: COM methods on a device this thread owns. `id` is a block this function frees.
    let id = unsafe { device.GetId() }
        .ok()
        .map(|id| unsafe { own(id) })?;
    let state = unsafe { device.GetState() }.map(|state| state.0).ok()?;
    let store = unsafe { device.OpenPropertyStore(STGM_READ) }.ok();

    let read = |key: &PROPERTYKEY| {
        store.as_ref().map_or_else(String::new, |store| {
            // SAFETY: a property store this function owns; a key that is read, not written.
            unsafe { store.GetValue(key) }.map_or_else(|_| String::new(), |value| value.to_string())
        })
    };

    Some(RenderEndpoint {
        id,
        adapter: read(&PKEY_INTERFACE_FRIENDLY_NAME),
        description: read(&PKEY_DEVICE_DESC),
        label: read(&PKEY_DEVICE_FRIENDLY_NAME),
        driver: read(&PKEY_DEVICE_DRIVER_IDENTITY),
        jack: read(&PKEY_AUDIOENDPOINT_JACK_SUBTYPE),
        // SAFETY: the same property store, read on the thread that opened it.
        channels: store
            .as_ref()
            .map_or(0, |store| unsafe { declared_channels(store) }),
        state,
    })
}

/// Channels the endpoint's device format declares, or `0` when it cannot be read.
///
/// # Safety
///
/// `store` must be valid on the calling thread.
unsafe fn declared_channels(store: &IPropertyStore) -> u16 {
    // SAFETY: reading a property from a store the caller owns.
    let Ok(value) = (unsafe { store.GetValue(&PKEY_AUDIOENGINE_DEVICE_FORMAT) }) else {
        return 0;
    };
    if value.vt() != VT_BLOB {
        return 0;
    }
    // SAFETY: the variant declares itself a blob, so the blob arm of the union is the live one.
    let blob = unsafe { value.Anonymous.Anonymous.Anonymous.blob };
    if blob.pBlobData.is_null() || (blob.cbSize as usize) < std::mem::size_of::<WAVEFORMATEX>() {
        return 0;
    }
    // SAFETY: at least a whole WAVEFORMATEX of blob, read unaligned because it is declared packed.
    unsafe { std::ptr::read_unaligned(blob.pBlobData.cast::<WAVEFORMATEX>()) }.nChannels
}

/// Take ownership of a `PWSTR` the API allocated, copying it out and freeing the block.
///
/// # Safety
///
/// `raw` must be a COM-allocated, NUL-terminated wide string that the caller owns.
unsafe fn own(raw: PWSTR) -> String {
    if raw.is_null() {
        return String::new();
    }
    // SAFETY: a NUL-terminated block the caller owns and this function frees.
    let text = unsafe { raw.to_string() }.unwrap_or_default();
    // SAFETY: allocated by the API with CoTaskMemAlloc, freed once, never used again.
    unsafe { CoTaskMemFree(Some(raw.as_ptr().cast())) };
    text
}

/// Resolve the microphone's destination and open it.
///
/// # Safety
///
/// COM must be initialized on the calling thread.
unsafe fn resolve_destination(
    enumerator: &IMMDeviceEnumerator,
) -> Result<(IMMDevice, RenderEndpoint), Unresolved> {
    // SAFETY: the caller's contract.
    let endpoints = unsafe { enumerate_render_endpoints(enumerator) }.map_err(|error| {
        Unresolved::Unopenable {
            label: "the playback device list".to_owned(),
            error,
        }
    })?;
    let chosen = choose(endpoints)?;

    let wide: Vec<u16> = chosen.id.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call. The ID is used here for the one
    // thing it is good for — re-opening this endpoint on this machine, in this process.
    match unsafe { enumerator.GetDevice(PCWSTR(wide.as_ptr())) } {
        Ok(device) => {
            tracing::info!(
                label = %chosen.label,
                channels = chosen.channels,
                "rendering the microphone into VB-CABLE"
            );
            Ok((device, chosen))
        }
        Err(e) => Err(Unresolved::Unopenable {
            label: chosen.label,
            error: e.to_string(),
        }),
    }
}

/// The endpoint ID the microphone would render into, without starting a microphone stream.
///
/// What the feedback-loop guard compares the speaker's capture target against. `None` when there
/// is no unambiguous destination — which is also when there is no microphone to conflict with, so
/// the guard has nothing to refuse.
///
/// # Safety
///
/// COM must be initialized on the calling thread.
unsafe fn microphone_endpoint_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
    // SAFETY: the caller's contract.
    let endpoints = unsafe { enumerate_render_endpoints(enumerator) }.ok()?;
    choose(endpoints).ok().map(|endpoint| endpoint.id)
}

// --- The microphone: WASAPI rendering into VB-CABLE ---------------------------------------------

/// How long the render loop waits on the engine's event before checking the stop flag anyway.
///
/// The event fires every engine period — a few milliseconds — so this timeout is never reached in
/// a healthy stream. It exists so that a stalled engine cannot hold the thread past a `stop`.
const RENDER_WAIT_MS: u32 = 200;

/// An [`AudioSink`] rendering the jitter buffer into a specific WASAPI render endpoint.
///
/// A join handle and a stop flag and nothing else, the same shape as [`WasapiLoopback`] above and
/// as the PipeWire source on Linux: that is what makes it `Send` with no COM object crossing a
/// thread boundary, since every interface pointer is created, used, and released on the render
/// thread.
struct WasapiRender {
    buffer: Arc<JitterBuffer>,
    worker: Option<Worker>,
}

impl AudioSink for WasapiRender {
    fn start(&mut self, format: AudioFormat) -> Result<(), AudioError> {
        if self.worker.is_some() {
            return Ok(());
        }
        if format.channels == 0 || format.sample_rate == 0 || format.frame_samples == 0 {
            return Err(AudioError::UnsupportedFormat(format!(
                "{}ch @ {}Hz",
                format.channels, format.sample_rate
            )));
        }

        self.buffer.clear();

        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let thread_stop = Arc::clone(&stop);
        let buffer = Arc::clone(&self.buffer);

        let thread = std::thread::Builder::new()
            .name("wasapi-render".to_owned())
            .spawn(move || render_thread(&buffer, format, &thread_stop, &ready_tx))
            .map_err(|e| AudioError::Unavailable(format!("could not spawn audio thread: {e}")))?;

        // Wait for the endpoint to open before accepting the stream. An unresolvable endpoint has
        // to refuse the microphone rather than accept one that never carries audio: the phone
        // would show a working microphone, and nobody would ever hear it.
        match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(())) => {
                self.worker = Some(Worker { stop, thread });
                tracing::info!("WASAPI microphone render started");
                Ok(())
            }
            Ok(Err(message)) => {
                let _ = thread.join();
                Err(AudioError::Unavailable(message))
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                let _ = thread.join();
                Err(AudioError::Unavailable(
                    "timed out opening VB-CABLE's playback device".to_owned(),
                ))
            }
        }
    }

    fn push(&mut self, frame: &[i16]) {
        if self.worker.is_some() {
            self.buffer.push(frame.to_vec());
        }
    }

    fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop.store(true, Ordering::Release);
            // The thread releases the render client and uninitializes COM on its way out; joining
            // is what makes that ordering observable to the caller.
            if worker.thread.join().is_err() {
                tracing::warn!("WASAPI render thread panicked during shutdown");
            }
            tracing::info!("WASAPI microphone render stopped");
        }
        // Audio queued but never rendered must not be played into the next stream.
        self.buffer.clear();
    }
}

impl Drop for WasapiRender {
    fn drop(&mut self) {
        // The render client must never outlive the sink: one left open keeps the audio engine
        // awake on an endpoint nothing is feeding.
        self.stop();
    }
}

/// Everything that happens on the dedicated render thread, COM initialization included.
fn render_thread(
    buffer: &Arc<JitterBuffer>,
    format: AudioFormat,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<(), String>>,
) {
    // SAFETY: paired with the CoUninitialize below, on this thread, with every interface pointer
    // created and dropped in between.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() {
        let _ = ready.send(Err(format!(
            "COM could not be initialized: {initialized:?}"
        )));
        return;
    }

    if let Err(message) = render(buffer, format, stop, ready) {
        tracing::error!(error = %message, "WASAPI microphone render failed");
        // Before the ready signal this send *is* the failure report; after it the receiver is gone
        // and the send is a harmless no-op.
        let _ = ready.send(Err(message));
    }

    // SAFETY: paired with the CoInitializeEx above; `render` has returned, so every interface
    // pointer it created has been dropped.
    unsafe { CoUninitialize() };
}

/// The render proper: resolve, open, report, then feed the engine until stopped.
fn render(
    buffer: &Arc<JitterBuffer>,
    format: AudioFormat,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    // SAFETY: COM is initialized on this thread for the whole of this function.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|e| format!("no audio endpoint enumerator: {e}"))?;

    // SAFETY: the enumerator above, on the thread that created it.
    let (device, endpoint) =
        unsafe { resolve_destination(&enumerator) }.map_err(|e| e.to_string())?;
    let label = endpoint.label.clone();

    // Registered for the lifetime of the stream: a renderer that misses its deadline produces a
    // gap in someone's call, and a busy machine is exactly when that happens.
    let _mmcss = MmcssThread::register();

    let mut session = RenderSession::open(&device, format, &label)?;
    ready
        .send(Ok(()))
        .map_err(|e| format!("startup reporter gone: {e}"))?;

    let outcome = session.pump(buffer, stop, &label);
    // The client stops here rather than at the end of scope, so it is gone before the device and
    // the enumerator that resolved it.
    drop(session);
    outcome
}

/// One open render stream: the client, its service, the event it is woken by, and the converter
/// built for this endpoint's mix format.
struct RenderSession {
    client: IAudioClient,
    render: IAudioRenderClient,
    event: Event,
    /// The endpoint buffer, in frames — the ceiling on what one pass may write.
    buffer_frames: u32,
    /// Bytes one endpoint frame occupies, for sizing the write.
    block_align: usize,
    converter: RenderConverter,
}

impl RenderSession {
    /// Open shared-mode, event-driven rendering on `device`.
    fn open(device: &IMMDevice, format: AudioFormat, label: &str) -> Result<Self, String> {
        // SAFETY: every call below is a COM method on an interface this function owns, on a thread
        // where COM is initialized. `mix` is owned by `MixFormat` for the whole block.
        unsafe {
            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("could not open \"{label}\": {e}"))?;

            let mix = MixFormat::of(&client)?;
            let target = endpoint_format(mix.as_ptr())?;
            let block_align = mix.block_align();
            let converter = RenderConverter::new(format, target)
                .map_err(|e| format!("\"{label}\" runs a format the wire cannot fill: {e}"))?;

            let event = Event::create()?;

            // Event-driven rather than polled. The capture in this same module polls, and that is
            // not an inconsistency to reconcile: loopback capture cannot combine with event
            // callbacks, which is a constraint on *that* path and not on an ordinary render
            // client. Waking exactly when the engine needs data beats a guessed interval that is
            // either wasteful or late.
            //
            // Shared mode, not exclusive: exclusive would fail whenever anything else holds the
            // endpoint, and would bypass the mixer for no benefit when the format conversion is
            // happening regardless.
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    BUFFER_DURATION_100NS,
                    0,
                    mix.as_ptr(),
                    None,
                )
                .map_err(|e| format!("\"{label}\" refused a render stream: {e}"))?;
            client
                .SetEventHandle(event.0)
                .map_err(|e| format!("\"{label}\" refused the render event: {e}"))?;

            let render: IAudioRenderClient = client
                .GetService()
                .map_err(|e| format!("no render service on \"{label}\": {e}"))?;
            let buffer_frames = client
                .GetBufferSize()
                .map_err(|e| format!("could not size \"{label}\"'s buffer: {e}"))?;

            // Primed with silence before starting, so the engine has something to render from the
            // first period rather than glitching while the first frames arrive from the network.
            render
                .GetBuffer(buffer_frames)
                .map_err(|e| format!("could not prime \"{label}\"'s buffer: {e}"))?;
            render
                .ReleaseBuffer(buffer_frames, silent_flag())
                .map_err(|e| format!("could not prime \"{label}\"'s buffer: {e}"))?;

            client
                .Start()
                .map_err(|e| format!("could not start rendering to \"{label}\": {e}"))?;

            tracing::info!(
                device = label,
                sample_rate = target.sample_rate,
                channels = target.channels,
                resampling = converter.is_resampling(),
                buffer_frames,
                "microphone render open"
            );

            Ok(Self {
                client,
                render,
                event,
                buffer_frames,
                block_align,
                converter,
            })
        }
    }

    /// Feed the engine until a stop is asked for or the endpoint fails.
    fn pump(
        &mut self,
        buffer: &Arc<JitterBuffer>,
        stop: &AtomicBool,
        label: &str,
    ) -> Result<(), String> {
        while !stop.load(Ordering::Acquire) {
            // SAFETY: this session's own event, waited on by the thread that created it. A
            // timeout is not a failure — it only means the stop flag gets checked.
            if unsafe { WaitForSingleObject(self.event.0, RENDER_WAIT_MS) } != WAIT_OBJECT_0 {
                continue;
            }
            if stop.load(Ordering::Acquire) {
                break;
            }

            // SAFETY: this session's own client, on the thread that opened it.
            let padding = unsafe { self.client.GetCurrentPadding() }
                .map_err(|e| render_failure(&e, label))?;
            let available = self.buffer_frames.saturating_sub(padding);
            if available == 0 {
                continue;
            }

            if buffer.depth() == 0 {
                // Underrun. The buffer is handed straight back flagged silent rather than filled
                // with zeros by hand: the engine then never reads it, which is both cheaper and
                // the API's own way of saying "nothing to play". Playback resumes on the next
                // pass that finds frames, with no gap beyond the silence itself.
                //
                // SAFETY: `available` frames are free by the padding just read, and the buffer is
                // returned unread.
                unsafe {
                    self.render
                        .GetBuffer(available)
                        .map_err(|e| render_failure(&e, label))?;
                    self.render
                        .ReleaseBuffer(available, silent_flag())
                        .map_err(|e| render_failure(&e, label))?;
                }
                continue;
            }

            // SAFETY: `available` frames are free by the padding just read, and WASAPI guarantees
            // `available * block_align` writable bytes at the returned pointer until ReleaseBuffer.
            let data = unsafe { self.render.GetBuffer(available) }
                .map_err(|e| render_failure(&e, label))?;
            let wanted = available as usize * self.block_align;
            let converted = self
                .converter
                .render(available as usize, |samples| buffer.pop_into(samples));
            if data.is_null() || converted.len() != wanted {
                // The converter's frame count is a contract, so this cannot happen; releasing
                // silent rather than writing a short buffer is what keeps it from being audible
                // if it ever does.
                debug_assert!(false, "the render converter produced a short buffer");
                // SAFETY: releasing exactly what GetBuffer reported, unread.
                unsafe { self.render.ReleaseBuffer(available, silent_flag()) }
                    .map_err(|e| render_failure(&e, label))?;
                continue;
            }
            // SAFETY: `converted` is exactly `wanted` bytes, and `data` has that many writable.
            unsafe { std::ptr::copy_nonoverlapping(converted.as_ptr(), data, wanted) };
            // SAFETY: releases exactly what GetBuffer reported, now written.
            unsafe { self.render.ReleaseBuffer(available, 0) }
                .map_err(|e| render_failure(&e, label))?;
        }
        Ok(())
    }
}

impl Drop for RenderSession {
    fn drop(&mut self) {
        // SAFETY: this session's own client, stopped on the thread that started it. A failure here
        // is not actionable — the interface is about to be released.
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

/// Turn a mid-stream WASAPI failure into the message the user sees.
///
/// **`AUDCLNT_E_DEVICE_INVALIDATED` fails the stream; it does not re-resolve and does not retry.**
/// The capture in this module does exactly the opposite, and that difference is deliberate rather
/// than an oversight to tidy up: the capture's job is to follow whatever endpoint the user has
/// chosen, so re-resolving is how it stays correct. This sink's job is to render into one
/// specific endpoint and no other, so the only thing there would be to re-resolve *to* is the
/// device that just went away — and the natural next step after a failed re-resolve, falling back
/// to something that works, is the user's own voice played aloud in the room their PC is in.
fn render_failure(error: &windows::core::Error, label: &str) -> String {
    if error.code() == AUDCLNT_E_DEVICE_INVALIDATED {
        format!(
            "VB-CABLE's \"{label}\" playback device went away while the microphone was streaming \
             — it was disabled, removed, or reinstalled. Start the microphone again once it is \
             back"
        )
    } else {
        format!("rendering the microphone to \"{label}\" failed: {error}")
    }
}

/// An auto-reset event owned for the lifetime of a render session.
struct Event(HANDLE);

impl Event {
    fn create() -> Result<Self, String> {
        // SAFETY: default security, auto-reset, initially unset, unnamed.
        let handle = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|e| format!("could not create the render event: {e}"))?;
        Ok(Self(handle))
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: a handle this value owns, closed once. The client that held it is stopped and
        // released first, because `RenderSession` declares it after the client.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Registers the capture thread with the Multimedia Class Scheduler for the lifetime of the
/// capture, so a busy machine does not starve a thread that must poll on time.
struct MmcssThread(Option<HANDLE>);

impl MmcssThread {
    fn register() -> Self {
        let task: Vec<u16> = "Audio\0".encode_utf16().collect();
        let mut index = 0_u32;
        // SAFETY: `task` is NUL-terminated and outlives the call.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(PCWSTR(task.as_ptr()), &mut index) };
        if let Err(e) = handle.as_ref() {
            tracing::debug!(error = %e, "capture thread will run at ordinary priority");
        }
        Self(handle.ok())
    }
}

impl Drop for MmcssThread {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            // SAFETY: the handle this thread registered, reverted once.
            unsafe {
                let _ = AvRevertMmThreadCharacteristics(handle);
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `KSNODETYPE_LINE_CONNECTOR`, which is what `CABLE In 16ch` presents.
    const KSNODETYPE_LINE_CONNECTOR: GUID =
        GUID::from_u128(0xdff2_1fe3_f70f_11d0_b917_00a0_c922_3196);

    /// A jack subtype rendered the way a property store renders one.
    fn jack(node_type: GUID) -> String {
        format!("{{{node_type:?}}}")
    }

    /// One of VB-CABLE's render endpoints, as 3.3.1.7 presents it.
    fn vb_cable(id: &str, description: &str, node_type: GUID, channels: u16) -> RenderEndpoint {
        RenderEndpoint {
            id: id.to_owned(),
            adapter: VB_CABLE_ADAPTER.to_owned(),
            description: description.to_owned(),
            label: format!("{description} ({VB_CABLE_ADAPTER})"),
            driver: "oem13.inf:c14ce8840c48fa1f:VBCableInst.NTamd64:3.3.1.7:VBAudioVACWDM"
                .to_owned(),
            jack: jack(node_type),
            channels,
            state: DEVICE_STATE_ACTIVE.0,
        }
    }

    /// The destination, as measured.
    fn cable_input() -> RenderEndpoint {
        vb_cable(
            "{0.0.0.00000000}.{b5e55cc9}",
            "CABLE Input",
            KSNODETYPE_SPEAKER,
            2,
        )
    }

    /// The decoy, as measured: the same adapter name, the same INF, the same driver version, the
    /// same declared channel count, and the same mix format. Only its jack differs.
    fn cable_in_16ch() -> RenderEndpoint {
        vb_cable(
            "{0.0.0.00000000}.{2ba58530}",
            "CABLE In 16ch",
            KSNODETYPE_LINE_CONNECTOR,
            2,
        )
    }

    /// Somebody else's playback device — and, pointedly, the sort of thing that would be the
    /// default output the resolver must never fall back to.
    fn ordinary_speakers() -> RenderEndpoint {
        RenderEndpoint {
            id: "{0.0.0.00000000}.{11111111}".to_owned(),
            adapter: "Realtek(R) Audio".to_owned(),
            description: "Speakers".to_owned(),
            label: "Speakers (Realtek(R) Audio)".to_owned(),
            driver: "oem7.inf:0000000000000000:RTKVHDInst.NTamd64:6.0.9600.1:RTKVHD64".to_owned(),
            jack: jack(KSNODETYPE_SPEAKER),
            channels: 2,
            state: DEVICE_STATE_ACTIVE.0,
        }
    }

    #[test]
    fn the_cable_input_should_be_chosen_over_the_sixteen_channel_decoy() {
        let chosen = choose(vec![cable_in_16ch(), cable_input(), ordinary_speakers()])
            .expect("exactly one candidate survives");
        assert_eq!(chosen.description, "CABLE Input");
    }

    /// MMDevice specifies no enumeration order, so the order endpoints happen to arrive in must
    /// not be able to change the answer. This is the failure the discrimination exists to prevent,
    /// and it is silent when it happens: rendering into `CABLE In 16ch` succeeds at every API call
    /// and is simply never heard.
    #[test]
    fn the_choice_should_not_depend_on_the_order_the_endpoints_were_enumerated_in() {
        let forwards = choose(vec![cable_input(), cable_in_16ch()]).expect("one survivor");
        let backwards = choose(vec![cable_in_16ch(), cable_input()]).expect("one survivor");
        assert_eq!(forwards, backwards);
        assert_eq!(forwards.description, "CABLE Input");
    }

    #[test]
    fn an_enumeration_with_no_vb_cable_should_be_reported_as_not_installed() {
        assert_eq!(choose(Vec::new()), Err(Unresolved::NotInstalled));
        assert_eq!(
            choose(vec![ordinary_speakers()]),
            Err(Unresolved::NotInstalled)
        );
        assert!(Unresolved::NotInstalled
            .to_string()
            .contains("not installed"));
    }

    #[test]
    fn a_disabled_cable_input_should_be_reported_as_unusable_rather_than_absent() {
        // Installed but turned off in the Sound control panel. A different remedy from installing
        // it, and a user sent to the wrong one gets nowhere.
        let mut disabled = cable_input();
        disabled.state = DEVICE_STATE_DISABLED.0;
        let expected = Unresolved::Unusable {
            label: "CABLE Input (VB-Audio Virtual Cable)".to_owned(),
            state: DEVICE_STATE_DISABLED.0,
        };
        assert_eq!(
            choose(vec![disabled, cable_in_16ch(), ordinary_speakers()]),
            Err(expected.clone())
        );
        assert!(expected.to_string().contains("disabled"));
    }

    #[test]
    fn two_indistinguishable_candidates_should_be_refused_rather_than_guessed_between() {
        // The shape a future VB-CABLE release would break the discrimination into: two endpoints
        // this build cannot tell apart. Never the first of them.
        let mut twin = cable_in_16ch();
        twin.jack = jack(KSNODETYPE_SPEAKER);
        assert_eq!(
            choose(vec![cable_input(), twin]),
            Err(Unresolved::Indistinguishable {
                matched: 2,
                survivors: 2,
            })
        );
    }

    #[test]
    fn a_release_where_nothing_survives_should_be_refused_distinguishably_from_absence() {
        // The other direction: VB-CABLE is installed and its destination was rejected anyway.
        // "Installed and unidentifiable" and "not installed" send a user to two different places.
        let expected = Unresolved::Indistinguishable {
            matched: 1,
            survivors: 0,
        };
        assert_eq!(
            choose(vec![cable_in_16ch(), ordinary_speakers()]),
            Err(expected.clone())
        );
        assert_ne!(expected, Unresolved::NotInstalled);
        assert!(expected.to_string().contains("could not be identified"));
    }

    #[test]
    fn an_unreadable_jack_should_leave_a_lone_endpoint_resolvable() {
        // A property that cannot be read means the rule cannot be applied, not that the endpoint
        // is wrong. Refusing here would turn a Windows release that stopped populating the
        // property into "VB-CABLE is not installed" on a machine where it plainly is.
        let mut blank = cable_input();
        blank.jack = String::new();
        assert_eq!(examine(&blank), Verdict::Candidate);
        assert_eq!(
            choose(vec![blank]).expect("one survivor").description,
            "CABLE Input"
        );
    }

    #[test]
    fn a_wide_endpoint_should_still_be_rejected_by_the_channel_rule() {
        // The channel rule discriminates nothing on 3.3.1.7, and is kept for the release that does
        // expose a genuinely wide render endpoint.
        let mut wide = cable_input();
        wide.id = "{0.0.0.00000000}.{99999999}".to_owned();
        wide.channels = 16;
        assert_eq!(examine(&wide), Verdict::TooManyChannels);
        assert_eq!(
            choose(vec![wide, cable_input()])
                .expect("one survivor")
                .channels,
            2
        );
    }

    #[test]
    fn the_adapter_name_carried_by_another_driver_should_not_match() {
        let mut impostor = cable_input();
        impostor.driver = "oem99.inf:0:SomeOtherInst.NTamd64:1.0.0.0:SOMEOTHERWDM".to_owned();
        assert_eq!(examine(&impostor), Verdict::ForeignDriver);
    }

    #[test]
    fn an_inactive_endpoint_should_never_be_a_candidate_whatever_its_jack_says() {
        for state in [
            DEVICE_STATE_DISABLED,
            DEVICE_STATE_UNPLUGGED,
            DEVICE_STATE_NOTPRESENT,
        ] {
            let mut endpoint = cable_input();
            endpoint.state = state.0;
            assert_eq!(examine(&endpoint), Verdict::Inactive);
            assert_ne!(describe_state(state.0), "not usable");
        }
    }

    /// The rule the rest of this feature's safety rests on: a resolution failure is an error, and
    /// never a substitution. Falling back to "whatever plays" would render the phone's microphone
    /// — the user's own voice, and whatever their phone can hear — aloud in the room the PC is in,
    /// and feed it back to the phone wherever the speaker stream is also up.
    #[test]
    fn a_failure_should_never_substitute_another_endpoint() {
        let mut disabled = cable_input();
        disabled.state = DEVICE_STATE_DISABLED.0;
        let mut twin = cable_in_16ch();
        twin.jack = jack(KSNODETYPE_SPEAKER);

        let enumerations = vec![
            Vec::new(),
            vec![ordinary_speakers()],
            vec![cable_in_16ch(), ordinary_speakers()],
            vec![disabled, ordinary_speakers()],
            vec![cable_input(), twin, ordinary_speakers()],
        ];
        for endpoints in enumerations {
            if let Ok(chosen) = choose(endpoints) {
                assert_eq!(
                    chosen.description, "CABLE Input",
                    "only the cable input may ever be returned, got {chosen:?}"
                );
            }
        }

        // And the case that matters most, stated on its own: with an ordinary default-looking
        // output present and no cable at all, the answer is a refusal rather than that output.
        assert!(choose(vec![ordinary_speakers()]).is_err());
    }

    #[test]
    fn capturing_the_microphones_own_destination_should_be_recognised_as_a_conflict() {
        let cable = cable_input().id;
        assert!(conflicts_with_microphone(&cable, Some(&cable)));
        // MMDevice is not consistent about the case it hands an ID back in, and two spellings of
        // one device are not two devices.
        assert!(conflicts_with_microphone(
            &cable.to_uppercase(),
            Some(&cable)
        ));
    }

    #[test]
    fn capturing_an_ordinary_output_should_not_be_a_conflict() {
        assert!(!conflicts_with_microphone(
            &ordinary_speakers().id,
            Some(&cable_input().id)
        ));
    }

    #[test]
    fn no_resolvable_microphone_should_mean_no_conflict_to_report() {
        // Nothing for the speaker to conflict *with*: the microphone could not be resolved, so it
        // is not rendering anywhere and the speaker has no reason to be refused. The microphone's
        // own refusal is where that gets reported.
        assert!(!conflicts_with_microphone(&ordinary_speakers().id, None));
        assert!(!conflicts_with_microphone(&cable_input().id, None));
    }

    #[test]
    fn the_conflict_message_should_name_the_device_and_the_remedy() {
        let message = feedback_conflict_message("CABLE Input (VB-Audio Virtual Cable)");
        assert!(message.contains("CABLE Input (VB-Audio Virtual Cable)"));
        assert!(message.contains("system output"));
    }
}
