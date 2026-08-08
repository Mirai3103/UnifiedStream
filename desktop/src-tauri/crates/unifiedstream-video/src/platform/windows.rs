//! Windows resolution of the video platform factory: the producing half of the shared-memory
//! frame transport.
//!
//! Unlike Linux, where the desktop writes to a device node and the kernel presents a camera, the
//! virtual camera here is a DirectShow filter that each consuming application loads into its own
//! process. This module publishes frames into the section those filters read, and it refuses the
//! stream — with the command that fixes it — while no filter is registered to read them.
//!
//! Three things are load-bearing and easy to get wrong.
//!
//! - **The mandatory label on the section's security descriptor.** Windows gives a new object the
//!   creator's integrity level, so a Medium-integrity desktop produces a section that a
//!   Low-integrity consuming process — a browser's media process, say — cannot open at all, even
//!   though it belongs to the same user. Omitting the label fails only under a sandbox, which is
//!   exactly the case a same-integrity test cannot reproduce.
//! - **A named section is refcounted.** `CreateFileMappingW` against a name a surviving consumer
//!   still holds returns the *existing* section, not a fresh one. The producer has to recognise
//!   that and reclaim it, which is what the init mutex guards.
//! - **Availability is a registry question, not a section question.** The section is ours to
//!   create whether or not any filter exists, so its presence proves nothing about whether an
//!   application could ever see the camera.
//!
//! Everything about the layout and the sequence protocol lives in [`crate::transport`], which has
//! no Win32 in it and is tested on every leg. This module is the shell: the mapping, the
//! descriptor, the mutex, the probe, and the worker thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, HANDLE, HLOCAL, WAIT_ABANDONED,
    WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
    MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CLASSES_ROOT, KEY_READ,
    KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_DWORD, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateMutexW, ReleaseMutex, WaitForSingleObject, INFINITE,
};

use crate::transport::{
    RingProducer, RingRegion, FILTER_CLSID, FILTER_DLL_X64, FILTER_DLL_X86, FILTER_FRIENDLY_NAME,
    FILTER_INSTALL_DIR, FILTER_VERSION_VALUE, HEARTBEAT_INTERVAL_MS, RING_BYTES, TRANSPORT_VERSION,
};
use crate::{SetupHint, VideoError, VideoFormat, VideoSink};

/// Name of the section the desktop produces into and every filter consumes from.
///
/// Session-local rather than `Global\`: the desktop and the applications consuming its camera are
/// the same logged-on user's processes, and `Global\` would require `SeCreateGlobalPrivilege`,
/// which a non-elevated desktop does not have.
///
/// **The name deliberately carries no version.** Putting one in it would make a mismatched filter
/// fail to find the mapping at all, which is indistinguishable from the filter not being installed
/// and produces the wrong message. Version lives in the header, so a disagreement can be reported
/// as a disagreement.
const SECTION_NAME: &str = r"Local\UnifiedStream.Camera";

/// Name of the mutex held only while the header is created or re-initialised.
const INIT_MUTEX_NAME: &str = r"Local\UnifiedStream.Camera.Init";

/// The section's security descriptor, in SDDL.
///
/// ```text
///   D:(A;;GA;;;OW)(A;;GR;;;AU)(A;;GR;;;AC)S:(ML;;NW;;;LW)
///      │            │            │          └ label the object Low integrity, no-write-up: a
///      │            │            │            sandboxed consumer may read, may not write
///      │            │            └ all application packages: read
///      │            └ authenticated users: read
///      └ owner rights: full
/// ```
///
/// Fixed here rather than left to a default, because a consumer that cannot open the mapping fails
/// in a way that looks like the feature being broken rather than like a permissions problem.
///
/// Two of these are easy to omit and hard to diagnose.
///
/// **The mandatory label.** Windows gives a new object the creator's integrity level, so a
/// Medium-integrity desktop produces a section a Low-integrity consumer cannot open at all. `LW`
/// lowers the label so a sandboxed process may read; `NW` keeps it read-only from there, which is
/// all a consumer needs.
///
/// **`OW`, not `CO`.** `CREATOR OWNER` is a placeholder that is substituted for a real SID only
/// when an ACE is *inherited* from a container. Applied directly to a kernel object it matches
/// nobody, and the creating user's implicit owner rights are `READ_CONTROL` and `WRITE_DAC` — not
/// map-write. The creating process never notices, because creating an object returns a handle
/// without an access check; what breaks is the *second* `CreateFileMappingW` against the same
/// name, which is exactly the reclaim path decision 4 requires. `OWNER RIGHTS` (S-1-3-4) is the
/// spelling that is evaluated at access-check time against whoever owns the object.
const SECTION_SDDL: &str = "D:(A;;GA;;;OW)(A;;GR;;;AU)(A;;GR;;;AC)S:(ML;;NW;;;LW)";

/// Queue between the receive path and the publishing worker. Two frames, as on Linux: the freshest
/// complete frame is worth more than any backlog.
const FRAME_QUEUE: usize = 2;

/// How long [`VideoSink::start`] waits for the worker to report that the section is up, so a
/// machine that cannot create one refuses the stream rather than accepting a stream nothing reads.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);

/// A shared-memory producer for the DirectShow virtual camera.
pub fn video_sink() -> Box<dyn VideoSink> {
    Box::new(SharedMemoryCameraSink::default())
}

/// Milliseconds on the only clock every process in the session agrees on.
///
/// The heartbeat is compared across a process boundary, so it cannot be an `Instant` — those are
/// meaningful only within one process. `GetTickCount64` is coarse, at roughly the scheduler's
/// resolution, which is far finer than the 250 ms interval and the 2 s staleness bound it is read
/// against.
fn tick_ms() -> u64 {
    // SAFETY: no arguments, no out-parameters, callable from any thread at any time.
    unsafe { GetTickCount64() }
}

/// Counters shared between the worker thread and status queries.
#[derive(Debug, Default)]
struct SinkCounters {
    /// Frames decoded and published into the ring.
    written: AtomicU64,
    /// Frames dropped because they failed to decode.
    decode_failures: AtomicU64,
}

/// A running publishing worker.
struct Worker {
    frames: mpsc::SyncSender<Vec<u8>>,
    handle: std::thread::JoinHandle<()>,
}

/// The shared-memory implementation of [`VideoSink`].
///
/// Holds a join handle and shared counters and nothing else. That is what makes it `Send` with no
/// section handle or mapped view crossing a thread boundary — the mapping is created, used, and
/// released entirely on the worker — and it is the same shape as `V4l2LoopbackSink` and as the
/// WASAPI capture, so the application layer sees identical lifecycles on every platform.
#[derive(Default)]
struct SharedMemoryCameraSink {
    worker: Option<Worker>,
    counters: Arc<SinkCounters>,
    /// Set while a stream is running, for the UI.
    label: Option<String>,
}

impl VideoSink for SharedMemoryCameraSink {
    fn start(&mut self, format: VideoFormat) -> Result<(), VideoError> {
        if self.worker.is_some() {
            return Ok(());
        }
        if format.width % 2 != 0 || format.height % 2 != 0 {
            // I420 subsamples chroma 2x2; odd geometry cannot be represented.
            return Err(VideoError::UnsupportedFormat(format!(
                "dimensions must be even, got {}x{}",
                format.width, format.height
            )));
        }

        // Probe before creating anything. The section is ours to create whether or not a filter
        // exists, so creating one first would prove nothing and would leave twelve megabytes of
        // commit behind a stream no application could ever see.
        match probe_filter() {
            FilterAvailability::Usable => {}
            unusable => return Err(VideoError::Unavailable(unusable.setup_hint())),
        }

        let (frame_tx, frame_rx) = mpsc::sync_channel::<Vec<u8>>(FRAME_QUEUE);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let counters = Arc::clone(&self.counters);
        counters.written.store(0, Ordering::Relaxed);
        counters.decode_failures.store(0, Ordering::Relaxed);

        let handle = std::thread::Builder::new()
            .name("camera-ring-producer".to_owned())
            .spawn(move || run(format, &frame_rx, &counters, &ready_tx))
            .map_err(|e| {
                VideoError::Unavailable(SetupHint::new(format!(
                    "could not spawn the sink worker: {e}"
                )))
            })?;

        // Wait for the section to exist, so a machine that cannot create one refuses here rather
        // than reporting a healthy stream nothing is reading.
        match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(())) => {
                tracing::info!(
                    section = SECTION_NAME,
                    width = format.width,
                    height = format.height,
                    "virtual camera frame transport up"
                );
                self.worker = Some(Worker {
                    frames: frame_tx,
                    handle,
                });
                self.label = Some(FILTER_FRIENDLY_NAME.to_owned());
                Ok(())
            }
            Ok(Err(message)) => {
                drop(frame_tx);
                let _ = handle.join();
                Err(VideoError::Unavailable(SetupHint::new(message)))
            }
            Err(_) => {
                drop(frame_tx);
                let _ = handle.join();
                Err(VideoError::Unavailable(SetupHint::new(
                    "timed out creating the camera frame transport".to_owned(),
                )))
            }
        }
    }

    fn push_frame(&mut self, jpeg: Vec<u8>) {
        if let Some(worker) = self.worker.as_ref() {
            // Full queue: drop the incoming frame. Blocking here would stall the receive path, and
            // a newer frame arrives within max_fps anyway.
            let _ = worker.frames.try_send(jpeg);
        }
    }

    fn frames_written(&self) -> u64 {
        self.counters.written.load(Ordering::Relaxed)
    }

    fn decode_failures(&self) -> u64 {
        self.counters.decode_failures.load(Ordering::Relaxed)
    }

    fn device_label(&self) -> Option<String> {
        // The filter's registered friendly name is this platform's label, exactly as the device
        // node is on Linux; nothing above the trait parses either.
        self.label.clone()
    }

    fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            // Closing the channel ends the worker loop. The worker clears the running flag, bumps
            // the generation, and unmaps on its way out; joining is what makes that ordering
            // observable here, so a consumer sees the stop before `stop` returns.
            drop(worker.frames);
            if worker.handle.join().is_err() {
                tracing::warn!("camera producer thread panicked during shutdown");
            }
        }
        self.label = None;
    }
}

impl Drop for SharedMemoryCameraSink {
    fn drop(&mut self) {
        // The section must not outlive the app: a consumer would keep reading a heartbeat that
        // stopped advancing and show a placeholder rather than discovering the camera is gone.
        self.stop();
    }
}

/// Everything that happens on the publishing thread: create the section, then serve it.
fn run(
    format: VideoFormat,
    frames: &mpsc::Receiver<Vec<u8>>,
    counters: &SinkCounters,
    ready: &mpsc::Sender<Result<(), String>>,
) {
    let section = match CameraSection::open() {
        Ok(section) => section,
        Err(message) => {
            let _ = ready.send(Err(message));
            return;
        }
    };

    let mut producer = match RingProducer::create(section.region(), format, tick_ms()) {
        Ok(producer) => producer,
        Err(e) => {
            let _ = ready.send(Err(format!(
                "could not initialise the frame transport: {e}"
            )));
            return;
        }
    };

    if ready.send(Ok(())).is_err() {
        return;
    }

    // Frame timestamps are the desktop's own publish times rather than the media header's capture
    // times: `VideoSink::push_frame` carries only bytes, and this change does not alter the trait.
    // A consumer paces on the spacing between them, which is the same either way.
    let started = Instant::now();
    let idle = Duration::from_millis(HEARTBEAT_INTERVAL_MS);
    let mut publish_failed = false;

    loop {
        match frames.recv_timeout(idle) {
            Ok(jpeg) => {
                let frame = match crate::decode_jpeg_to_i420(&jpeg, format.width, format.height) {
                    Ok(frame) => frame,
                    Err(e) => {
                        // One bad frame must not end the stream, §7.2. Counted so the UI's fps
                        // figure reflects reality rather than reading as a healthy stream.
                        counters.decode_failures.fetch_add(1, Ordering::Relaxed);
                        tracing::debug!(error = %e, "camera frame dropped");
                        continue;
                    }
                };
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "microseconds since start; a session would have to run 584,000 years"
                )]
                let timestamp_us = started.elapsed().as_micros() as u64;
                match producer.publish(&frame, timestamp_us, tick_ms()) {
                    Ok(_) => {
                        publish_failed = false;
                        counters.written.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        if !publish_failed {
                            // Log once. Frames keep draining so the sender never blocks, and fps
                            // reads zero in the UI, which is the contract the trait states.
                            publish_failed = true;
                            tracing::warn!(error = %e, "could not publish a camera frame");
                        }
                    }
                }
            }
            // No frame this interval: the stream is running but quiet, and the heartbeat is the
            // only thing that keeps a consumer from concluding the desktop has died.
            Err(mpsc::RecvTimeoutError::Timeout) => producer.tick(tick_ms()),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // A clean stop, observed immediately by every consumer rather than after the staleness bound.
    producer.stop();
}

/// The named section and the view of it this process publishes through.
///
/// Fields drop in declaration order, which is the order these have to be released in: the view
/// before the handle that backs it.
///
/// Public for the same reason `LoopbackDevice` is on Linux: the manual smoke test has to drive the
/// producer over the real kernel objects, and until `add-windows-directshow-camera` installs a
/// filter, `VideoSink::start` refuses on every machine and never reaches them.
pub struct CameraSection {
    view: MappedView,
    _section: SectionHandle,
}

impl CameraSection {
    /// Create or reclaim the section, under the initialisation mutex.
    ///
    /// # Errors
    ///
    /// Returns a message naming what failed when the descriptor, the mutex, the section, or the
    /// view could not be created.
    pub fn open() -> Result<Self, String> {
        // Held across creation and mapping, not only across creation: a second desktop instance,
        // or a restart while a filter still holds the section open, must not race the header into
        // an inconsistent state.
        let mutex = InitMutex::open()?;
        let _held = mutex.acquire()?;

        let descriptor = SecurityDescriptor::from_sddl(SECTION_SDDL)?;
        let mut attributes = SECURITY_ATTRIBUTES {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "the structure is a few dozen bytes"
            )]
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.as_ptr(),
            bInheritHandle: false.into(),
        };

        let name = wide(SECTION_NAME);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "RING_BYTES is a compile-time constant of about twelve megabytes"
        )]
        let (size_high, size_low) = (
            (RING_BYTES as u64 >> 32) as u32,
            (RING_BYTES as u64 & 0xFFFF_FFFF) as u32,
        );

        // SAFETY: `attributes` and `name` outlive the call; INVALID_HANDLE_VALUE asks for a
        // pagefile-backed section rather than one over a file.
        let handle = unsafe {
            CreateFileMappingW(
                windows::Win32::Foundation::INVALID_HANDLE_VALUE,
                Some(&raw mut attributes),
                PAGE_READWRITE,
                size_high,
                size_low,
                PCWSTR(name.as_ptr()),
            )
        }
        .map_err(|e| format!("could not create the camera frame section: {e}"))?;

        // SAFETY: read immediately after the call above, on this thread, before anything else can
        // overwrite it.
        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if existed {
            // Not an error and not rare: a named section is refcounted, so a filter that still
            // holds the previous one keeps it alive and this call returns *that* section. It is
            // reclaimed and re-initialised rather than assumed to be new — which is also why the
            // allocation is a fixed maximum, since a held section cannot be resized.
            tracing::info!(
                section = SECTION_NAME,
                "reclaiming a camera section a consumer still holds"
            );
        }
        let section = SectionHandle(handle);

        // SAFETY: the section was just created at RING_BYTES bytes, and the view is unmapped by
        // the guard below.
        let address = unsafe { MapViewOfFile(section.0, FILE_MAP_ALL_ACCESS, 0, 0, RING_BYTES) };
        if address.Value.is_null() {
            // SAFETY: read immediately after the failing call.
            let error = unsafe { GetLastError() };
            return Err(format!("could not map the camera frame section: {error:?}"));
        }

        Ok(Self {
            view: MappedView(address),
            _section: section,
        })
    }

    /// The mapped bytes, as the transport's portable view of them.
    #[must_use]
    pub fn region(&self) -> RingRegion {
        // SAFETY: the view is RING_BYTES bytes and stays mapped for as long as `self` lives, which
        // is the whole of the worker thread. Other processes may map the same pages, which is the
        // point of the section and exactly what `RingRegion` documents.
        unsafe { RingRegion::new(self.view.0.Value.cast::<u8>(), RING_BYTES) }
    }
}

/// A section handle, closed on drop.
///
/// A guard rather than a bare call because the section is refcounted by the kernel: a leaked handle
/// keeps a stale section alive, and the *next* producer would then reclaim a mapping nobody is
/// reading instead of creating a fresh one.
struct SectionHandle(HANDLE);

impl Drop for SectionHandle {
    fn drop(&mut self) {
        // SAFETY: a handle this guard owns, closed exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// A mapped view, unmapped on drop.
///
/// A guard rather than a bare call because every failure path after mapping — and there are
/// several, since the header is initialised afterwards — would otherwise leak twelve megabytes of
/// address space for the life of the process.
struct MappedView(MEMORY_MAPPED_VIEW_ADDRESS);

impl Drop for MappedView {
    fn drop(&mut self) {
        // SAFETY: the address this guard owns, unmapped exactly once.
        unsafe {
            let _ = UnmapViewOfFile(self.0);
        }
    }
}

/// The named initialisation mutex, closed on drop.
struct InitMutex(HANDLE);

impl InitMutex {
    fn open() -> Result<Self, String> {
        let name = wide(INIT_MUTEX_NAME);
        // SAFETY: `name` outlives the call. Ownership is not taken here — `acquire` waits for it,
        // so the abandoned case is handled in one place.
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
            .map_err(|e| format!("could not open the camera initialisation mutex: {e}"))?;
        Ok(Self(handle))
    }

    /// Wait for the mutex, returning a guard that releases it.
    fn acquire(&self) -> Result<InitMutexGuard<'_>, String> {
        // SAFETY: a handle this value owns.
        let waited = unsafe { WaitForSingleObject(self.0, INFINITE) };
        match waited {
            WAIT_OBJECT_0 => Ok(InitMutexGuard(self)),
            // The previous holder died while initialising, so the header may be half-written. That
            // is not a reason to refuse: ownership *is* granted, and the caller re-initialises the
            // header completely, which is the only thing the mutex protects.
            WAIT_ABANDONED => {
                tracing::warn!("the previous camera producer died while initialising the section");
                Ok(InitMutexGuard(self))
            }
            other => Err(format!(
                "could not take the camera initialisation mutex: {other:?}"
            )),
        }
    }
}

impl Drop for InitMutex {
    fn drop(&mut self) {
        // SAFETY: a handle this guard owns, closed exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Ownership of the initialisation mutex, released on drop.
///
/// A guard rather than a bare `ReleaseMutex` because every early return between taking it and
/// finishing the header would otherwise leave it held — and a held mutex on a named object outlives
/// the function, blocking the next producer until the process exits.
struct InitMutexGuard<'a>(&'a InitMutex);

impl Drop for InitMutexGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: released by the thread that waited for it, exactly once.
        unsafe {
            let _ = ReleaseMutex(self.0 .0);
        }
    }
}

/// A security descriptor built from SDDL, freed on drop.
///
/// A guard rather than a bare pointer because the descriptor is handed to `CreateFileMappingW`,
/// which may fail, and the block is `LocalAlloc`-ed by the conversion — it leaks on every early
/// return otherwise.
struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn from_sddl(sddl: &str) -> Result<Self, String> {
        let text = wide(sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `text` is NUL-terminated and outlives the call; the returned block becomes ours.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(|e| format!("could not build the camera section's security descriptor: {e}"))?;
        Ok(Self(descriptor))
    }

    fn as_ptr(&self) -> *mut core::ffi::c_void {
        self.0 .0
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by the conversion above, freed once, and never used again.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0 .0)));
        }
    }
}

/// What the availability probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FilterAvailability {
    /// Registered for both architectures, both DLLs on disk, both speaking this transport version.
    Usable,
    /// Nothing registered under the filter's CLSID in either view.
    Absent,
    /// Registered for one architecture only. A real state, not a rounding of "installed": a
    /// 32-bit application can only load a 32-bit filter, so half-registered means half the
    /// applications on the machine would never see the camera.
    OneArchitecture {
        /// The architecture that is registered.
        present: Bitness,
        /// The architecture that is not.
        missing: Bitness,
    },
    /// Registered, and speaking a transport version this desktop does not.
    VersionMismatch {
        /// The architecture whose filter disagrees, so the hint can name one command rather than
        /// telling the user to re-register everything and hope.
        bitness: Bitness,
        /// The version the installed filter advertises.
        found: u32,
    },
}

/// `%ProgramFiles%`, expanded now rather than left for a shell to expand later.
///
/// The hint is built on the machine that will run it, so it can name a real path. Leaving
/// `%ProgramFiles%` in the string only works if the user pastes it into `cmd.exe`; in PowerShell —
/// which is what an elevated prompt usually is on Windows 10 and later — `%VAR%` is literal text,
/// and the command fails in a way that looks like the guidance itself being wrong.
fn program_files() -> String {
    std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_owned())
}

/// `%SystemRoot%`, expanded for the same reason as [`program_files`].
fn system_root() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned())
}

/// Where the filter is installed, as a path a shell does not have to expand.
fn install_dir() -> String {
    // The contract is written as `%ProgramFiles%\UnifiedStream`; this is the same location with the
    // one variable resolved.
    FILTER_INSTALL_DIR.replace("%ProgramFiles%", &program_files())
}

/// Which registry view, and therefore which architecture of application, a registration serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bitness {
    X64,
    X86,
}

impl Bitness {
    const fn label(self) -> &'static str {
        match self {
            Self::X64 => "64-bit",
            Self::X86 => "32-bit",
        }
    }

    /// The command that registers this architecture's filter.
    ///
    /// A 32-bit COM DLL must be registered by the 32-bit `regsvr32`, which lives in `SysWOW64` on a
    /// 64-bit Windows — the naming looks backwards and is the single most common way this is got
    /// wrong. Both need an elevated prompt, which the message says.
    ///
    /// **One command, and no shell syntax of any kind.** It has to survive being pasted into
    /// whatever elevated prompt the user happens to open, and the two obvious ways to write this
    /// both fail somewhere: `%VAR%` is literal text in PowerShell, and `&&` is a parse error in
    /// Windows PowerShell 5.1, which is still what "Run as administrator" gives on Windows 10. So
    /// paths are expanded here and the two registrations are two separate hints — the probe already
    /// distinguishes "neither is registered" from "one is", so naming the second only once the
    /// first is done costs nothing and keeps every command a single copyable line.
    fn register_command(self) -> String {
        match self {
            Self::X64 => format!(r#"regsvr32 "{}\{FILTER_DLL_X64}""#, install_dir()),
            Self::X86 => format!(
                r#"{}\SysWOW64\regsvr32.exe "{}\{FILTER_DLL_X86}""#,
                system_root(),
                install_dir()
            ),
        }
    }
}

impl FilterAvailability {
    /// The guidance shown to the user, with the command that resolves it.
    ///
    /// Each outcome gets its own message and its own command, so the UI's copyable box says
    /// something true rather than something generic — the same shape the missing `v4l2loopback`
    /// module already has on Linux.
    fn setup_hint(&self) -> SetupHint {
        match self {
            // Unreachable through `start`, which checks for it before asking for a hint; a hint is
            // still the honest answer rather than a panic.
            Self::Usable => SetupHint::new("the virtual camera is available"),
            Self::Absent => {
                // The 64-bit half first. Both are needed, but naming both at once means either a
                // shell operator or a two-line block, and neither survives every elevated prompt.
                // Once this one is registered the probe reports `OneArchitecture`, which names the
                // other — so the user is walked through it a command at a time.
                let command = Bitness::X64.register_command();
                SetupHint::with_command(
                    format!(
                        "the {FILTER_FRIENDLY_NAME} filter is not installed — register it from an \
                         elevated command prompt with: {command} (the 32-bit filter is needed too, \
                         and the desktop names it once this one is registered)"
                    ),
                    command,
                )
            }
            Self::OneArchitecture { present, missing } => {
                let command = missing.register_command();
                SetupHint::with_command(
                    format!(
                        "the {FILTER_FRIENDLY_NAME} filter is registered for {} applications only, \
                         so {} applications cannot see the camera — register the other half from \
                         an elevated command prompt with: {command}",
                        present.label(),
                        missing.label()
                    ),
                    command,
                )
            }
            Self::VersionMismatch { bitness, found } => {
                let command = bitness.register_command();
                SetupHint::with_command(
                    format!(
                        "the installed {} {FILTER_FRIENDLY_NAME} filter speaks frame format \
                         version {found} and this desktop speaks version {TRANSPORT_VERSION} — \
                         install the matching filter and re-register it from an elevated command \
                         prompt with: {command}",
                        bitness.label()
                    ),
                    command,
                )
            }
        }
    }
}

/// One architecture's registration, as the registry describes it.
struct Registration {
    /// The DLL named by `InprocServer32`, confirmed present on disk.
    dll: PathBuf,
    /// The transport version it advertises, absent if it recorded none.
    transport_version: Option<u32>,
}

/// Decide whether an application on this machine could see the camera at all.
///
/// Reads both registry views. The desktop is a 64-bit process, so the native view says whether
/// 64-bit applications can see the camera and the `WOW6432Node` view says whether 32-bit ones can.
/// Reading `HKEY_CLASSES_ROOT` needs no elevation.
fn probe_filter() -> FilterAvailability {
    let x64 = probe_view(KEY_WOW64_64KEY);
    let x86 = probe_view(KEY_WOW64_32KEY);

    match (&x64, &x86) {
        (None, None) => FilterAvailability::Absent,
        (Some(_), None) => FilterAvailability::OneArchitecture {
            present: Bitness::X64,
            missing: Bitness::X86,
        },
        (None, Some(_)) => FilterAvailability::OneArchitecture {
            present: Bitness::X86,
            missing: Bitness::X64,
        },
        (Some(native), Some(wow)) => {
            // A registration that records no version predates the value, which means it predates
            // this contract: treated as version 0, which this build does not speak.
            for (bitness, registration) in [(Bitness::X64, native), (Bitness::X86, wow)] {
                let version = registration.transport_version.unwrap_or(0);
                if version != TRANSPORT_VERSION {
                    tracing::warn!(
                        dll = %registration.dll.display(),
                        architecture = bitness.label(),
                        version,
                        expected = TRANSPORT_VERSION,
                        "the installed camera filter speaks a different frame format version"
                    );
                    return FilterAvailability::VersionMismatch {
                        bitness,
                        found: version,
                    };
                }
            }
            FilterAvailability::Usable
        }
    }
}

/// Read the filter's registration from one registry view, or `None` if it is not there.
fn probe_view(view: REG_SAM_FLAGS) -> Option<Registration> {
    let clsid_key = RegKey::open(&format!(r"CLSID\{FILTER_CLSID}"), view)?;
    let server_key = RegKey::open(&format!(r"CLSID\{FILTER_CLSID}\InprocServer32"), view)?;

    // The default value of InprocServer32 is the DLL path, which is the registration's whole point.
    let dll = PathBuf::from(server_key.read_string("")?);
    if !dll.is_file() {
        // Registered but not present: a leftover CLSID from an uninstall leaves a broken camera in
        // every application's device list, and reporting it as installed would be a lie the user
        // could not act on.
        tracing::warn!(
            dll = %dll.display(),
            "the camera filter is registered but its DLL is missing"
        );
        return None;
    }

    Some(Registration {
        transport_version: clsid_key.read_u32(FILTER_VERSION_VALUE),
        dll,
    })
}

/// An open registry key, closed on drop.
///
/// A guard rather than a bare call because the probe opens two keys per view and returns early
/// from several places; a leaked `HKEY` is a handle held for the life of the process.
struct RegKey(HKEY);

impl RegKey {
    /// Open a subkey of `HKEY_CLASSES_ROOT` in one architecture's view.
    fn open(subkey: &str, view: REG_SAM_FLAGS) -> Option<Self> {
        let name = wide(subkey);
        let mut key = HKEY::default();
        // SAFETY: `name` is NUL-terminated and outlives the call; `key` is written only on success.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CLASSES_ROOT,
                PCWSTR(name.as_ptr()),
                None,
                KEY_READ | view,
                &raw mut key,
            )
        };
        status.is_ok().then_some(Self(key))
    }

    /// Read a `REG_SZ` value, `""` meaning the key's default value.
    fn read_string(&self, value: &str) -> Option<String> {
        let (kind, bytes) = self.read_value(value)?;
        if kind != REG_SZ {
            return None;
        }
        // The block is UTF-16, usually with a trailing NUL the reported length may or may not
        // include — hence stopping at the first NUL rather than trusting the length.
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .filter_map(|pair| match pair {
                [low, high] => Some(u16::from_le_bytes([*low, *high])),
                _ => None,
            })
            .collect();
        let text = String::from_utf16(units.split(|&unit| unit == 0).next()?).ok()?;
        (!text.is_empty()).then_some(text)
    }

    /// Read a `REG_DWORD` value.
    fn read_u32(&self, value: &str) -> Option<u32> {
        let (kind, bytes) = self.read_value(value)?;
        if kind != REG_DWORD {
            return None;
        }
        match bytes.get(..4) {
            Some([a, b, c, d]) => Some(u32::from_le_bytes([*a, *b, *c, *d])),
            _ => None,
        }
    }

    /// Read one value's raw bytes and type, sizing the buffer from the API's own answer.
    fn read_value(&self, value: &str) -> Option<(REG_VALUE_TYPE, Vec<u8>)> {
        let name = wide(value);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0_u32;

        // SAFETY: `name` outlives both calls. The first asks only for the size, with no data
        // buffer; the second fills a buffer of exactly that size.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&raw mut kind),
                None,
                Some(&raw mut size),
            )
        };
        if status.is_err() || size == 0 {
            return None;
        }

        let mut buffer = vec![0_u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&raw mut kind),
                Some(buffer.as_mut_ptr()),
                Some(&raw mut size),
            )
        };
        if status.is_err() {
            return None;
        }
        buffer.truncate(size as usize);
        Some((kind, buffer))
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: a key this guard opened, closed exactly once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// A NUL-terminated UTF-16 string, as every `…W` entry point wants it.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{ReadOutcome, RingConsumer};
    use windows::Win32::System::Memory::{OpenFileMappingW, FILE_MAP_READ};

    /// Open the section by name, exactly as a filter in another process would.
    ///
    /// Same process, different mapping: the handle and the view are independent of the producer's,
    /// which is what makes this a test of the *name* rather than of a pointer handed over locally.
    /// It cannot reproduce the case a sandboxed consumer is in — that needs a genuinely
    /// Low-integrity process, and confirming the mandatory label against one is
    /// `add-windows-directshow-camera`'s to do.
    struct ConsumerMapping {
        view: MappedView,
        _section: SectionHandle,
    }

    impl ConsumerMapping {
        fn open() -> Self {
            let name = wide(SECTION_NAME);
            // Read-only, because that is all a consumer is granted and all it needs: the descriptor
            // gives authenticated users `GR`, and the mandatory label is no-write-up. A consumer
            // asking for write access is refused, which is the intended shape of the boundary.
            // SAFETY: `name` outlives the call.
            let handle = unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr())) }
                .expect("a consumer must be able to open the section by name");
            // SAFETY: the section is RING_BYTES bytes; the guard unmaps the view.
            let address = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, RING_BYTES) };
            assert!(
                !address.Value.is_null(),
                "a consumer must be able to map the section"
            );
            Self {
                view: MappedView(address),
                _section: SectionHandle(handle),
            }
        }

        fn region(&self) -> RingRegion {
            // SAFETY: RING_BYTES mapped bytes, alive for as long as this value.
            unsafe { RingRegion::new(self.view.0.Value.cast::<u8>(), RING_BYTES) }
        }
    }

    #[test]
    fn a_second_producer_should_reclaim_a_section_that_is_still_held() {
        // Decision 4's requirement, and the one a wrong descriptor breaks silently: a named section
        // is refcounted, so a restart while a consumer still holds it must reopen *that* section
        // with write access rather than being refused.
        let first = CameraSection::open().expect("the section must be creatable");
        let second =
            CameraSection::open().expect("a held section must be reclaimable, not refused");
        drop(second);
        drop(first);
    }

    #[test]
    fn a_frame_should_cross_the_named_section_to_an_independent_mapping() {
        let format = VideoFormat {
            width: 64,
            height: 48,
            max_fps: 30,
        };
        let section = CameraSection::open().expect("the section must be creatable");
        let mut producer =
            RingProducer::create(section.region(), format, tick_ms()).expect("must initialise");

        let consumer_mapping = ConsumerMapping::open();
        let mut consumer =
            RingConsumer::attach(consumer_mapping.region()).expect("a consumer must attach");

        let sent = vec![0x3C_u8; format.i420_frame_bytes()];
        producer.publish(&sent, 7, tick_ms()).expect("must publish");

        let mut received = Vec::new();
        assert!(
            matches!(
                consumer.read(&mut received),
                ReadOutcome::Frame {
                    timestamp_us: 7,
                    width: 64,
                    height: 48,
                    ..
                }
            ),
            "the frame must arrive through the named section"
        );
        assert_eq!(received, sent, "and must arrive byte for byte");
    }

    #[test]
    fn the_init_mutex_should_serialise_two_initialisers() {
        let mutex = InitMutex::open().expect("the mutex must be creatable");
        let held = mutex.acquire().expect("the first waiter must get it");

        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let contender = std::thread::spawn(move || {
            let mutex = InitMutex::open().expect("the mutex must be openable by name");
            let _held = mutex
                .acquire()
                .expect("the second waiter must get it eventually");
            entered_tx
                .send(())
                .expect("the test must still be listening");
        });

        // While the first guard lives, the second waiter cannot be inside.
        assert!(
            entered_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "a second initialiser must not enter while the mutex is held"
        );

        drop(held);
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the second initialiser must enter once the mutex is released");
        contender.join().expect("the contender must not panic");
    }

    #[test]
    fn the_probe_should_report_absent_where_no_filter_is_registered() {
        // True of every machine until `add-windows-directshow-camera` installs one, which is the
        // state this change ships in and the state the refusal below is written for.
        assert_eq!(probe_filter(), FilterAvailability::Absent);
    }

    #[test]
    fn every_refusal_should_carry_a_command_the_user_can_run() {
        for outcome in [
            FilterAvailability::Absent,
            FilterAvailability::OneArchitecture {
                present: Bitness::X64,
                missing: Bitness::X86,
            },
            FilterAvailability::VersionMismatch {
                bitness: Bitness::X86,
                found: 99,
            },
        ] {
            let hint = outcome.setup_hint();
            let command = hint
                .command
                .as_deref()
                .unwrap_or_else(|| panic!("{outcome:?} must name a command"));
            assert!(
                command.contains("regsvr32"),
                "{outcome:?} must name the registration command, got {command}"
            );
            assert!(
                hint.message.contains(command),
                "the message must be actionable on its own, in logs as well as in the UI"
            );

            // The command is copied out of the UI into whatever elevated prompt the user opened,
            // so it must not depend on which shell that is. `%VAR%` is expanded by cmd.exe and is
            // literal text in PowerShell; `&&` is a parse error in Windows PowerShell 5.1, which
            // is still what "Run as administrator" gives on Windows 10. Both produce an error that
            // reads as the guidance being wrong rather than as the shell being different.
            assert!(
                !command.contains('%'),
                "{outcome:?} leaves a variable for the shell to expand: {command}"
            );
            assert!(
                !command.contains("&&") && !command.contains(';'),
                "{outcome:?} needs a shell operator, so it is not one command: {command}"
            );
            assert!(
                command.contains(":\\"),
                "{outcome:?} must name an absolute path, got {command}"
            );
        }
    }

    #[test]
    fn the_two_registration_commands_should_use_the_right_regsvr32_for_each_architecture() {
        // A 32-bit COM DLL registered by the 64-bit regsvr32 silently lands in the wrong registry
        // view, which is the failure this naming exists to avoid — and `SysWOW64` holding the
        // 32-bit tool reads backwards, so it is worth pinning.
        let x64 = Bitness::X64.register_command();
        let x86 = Bitness::X86.register_command();

        assert!(
            !x64.contains("SysWOW64"),
            "the 64-bit filter uses the native regsvr32: {x64}"
        );
        assert!(x64.contains(FILTER_DLL_X64));
        assert!(
            x86.contains(r"SysWOW64\regsvr32.exe"),
            "the 32-bit filter needs the 32-bit regsvr32: {x86}"
        );
        assert!(x86.contains(FILTER_DLL_X86));
    }

    #[test]
    fn a_start_on_a_machine_with_no_filter_should_refuse_with_guidance() {
        let mut sink = SharedMemoryCameraSink::default();
        let err = sink
            .start(VideoFormat::CAMERA_720P)
            .expect_err("no filter is registered, so the stream must be refused");

        match err {
            VideoError::Unavailable(hint) => {
                assert!(
                    hint.command.is_some(),
                    "a missing component must name what installs it"
                );
                assert!(
                    !hint.message.contains("no virtual camera implementation"),
                    "this platform has an implementation; what it lacks is the component"
                );
            }
            other => panic!("expected an availability refusal, got {other:?}"),
        }
        assert_eq!(sink.device_label(), None);
        assert_eq!(sink.frames_written(), 0);
    }

    #[test]
    fn odd_geometry_should_be_refused_before_anything_is_created() {
        let mut sink = SharedMemoryCameraSink::default();
        let err = sink
            .start(VideoFormat {
                width: 641,
                height: 480,
                max_fps: 30,
            })
            .expect_err("odd dimensions cannot be represented in I420");
        assert!(matches!(err, VideoError::UnsupportedFormat(_)));
    }
}
