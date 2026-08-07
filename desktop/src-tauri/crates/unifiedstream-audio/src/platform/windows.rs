//! Windows resolution of the audio platform factories, and the WASAPI loopback capture behind
//! the speaker.
//!
//! Partial by design and by phase: the speaker is implemented here, the microphone is borrowed
//! from the fallback until a driver exists to create an audio input endpoint for it, and routing
//! does not exist as a concept on this platform at all.
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

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use windows::core::{implement, PCWSTR};
use windows::Win32::Foundation::{HANDLE, PROPERTYKEY};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IAudioCaptureClient, IAudioClient, IAudioRenderClient,
    IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_LOOPBACK, DEVICE_STATE, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
};

use crate::{
    AudioCapture, AudioConverter, AudioError, AudioFormat, AudioRouting, FrameCallback,
    FrameChunker, SampleType, SourceFormat,
};

/// Borrowed from the fallback: a virtual microphone needs an audio input endpoint, and on this
/// platform only a driver can create one. Phase W4 supplies that driver, and replacing this one
/// line with a WASAPI render sink on its endpoint is what that phase changes in this module.
pub use super::unsupported::audio_sink;

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

    let mut session = Some(Session::open(&enumerator, format)?);
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
            Err(e) => {
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

impl Session {
    /// Open loopback capture and the keep-alive on the current default render endpoint.
    fn open(enumerator: &IMMDeviceEnumerator, format: AudioFormat) -> Result<Self, String> {
        // SAFETY: every call below is a COM method on an interface this function owns, on a
        // thread where COM is initialized. `mix` is owned by `MixFormat` for the whole block.
        unsafe {
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("no default output device: {e}"))?;

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

/// Read the endpoint's format into the portable description the converter takes.
///
/// # Safety
///
/// `mix` must point at a valid `WAVEFORMATEX`, and at a `WAVEFORMATEXTENSIBLE` when it declares
/// itself extensible.
unsafe fn source_format(mix: *const WAVEFORMATEX) -> Result<SourceFormat, String> {
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

    Ok(SourceFormat {
        sample_rate: base.nSamplesPerSec,
        channels: base.nChannels,
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
