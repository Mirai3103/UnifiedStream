//! The v4l2loopback virtual camera: device discovery and the frame-writing sink.
//!
//! The sink owns the device on a dedicated worker thread — decode work and a potentially slow
//! `write()` never run on the receive path. The app never loads kernel modules itself; when no
//! loopback device exists the error carries [`crate::MODPROBE_HINT`] for the user to run.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use v4l::video::Output as _;
use v4l::FourCC;

use crate::{VideoError, VideoFormat, VideoSink, CAMERA_NODE_LABEL, MODPROBE_HINT};

/// Driver name every v4l2loopback device reports in `VIDIOC_QUERYCAP`.
const LOOPBACK_DRIVER: &str = "v4l2 loopback";

/// Queue between the receive path and the device worker. Two frames: the freshest complete
/// frame is worth more than any backlog, so a stalled device drops instead of buffering.
const FRAME_QUEUE: usize = 2;

/// A discovered v4l2loopback device.
#[derive(Debug, Clone)]
pub struct LoopbackDevice {
    /// Device node, e.g. `/dev/video10`.
    pub path: PathBuf,
    /// Card label the module was loaded with.
    pub label: String,
}

/// Find the loopback device to present the camera on.
///
/// Prefers a device labelled [`CAMERA_NODE_LABEL`]; otherwise the first loopback device that
/// still offers video output (with `exclusive_caps=1`, a device already claimed by another
/// writer hides its output capability and is skipped).
///
/// # Errors
///
/// Returns [`VideoError::Unavailable`] — carrying the modprobe guidance — when no usable
/// loopback device exists.
pub fn find_loopback_device() -> Result<LoopbackDevice, VideoError> {
    let mut nodes: Vec<_> = v4l::context::enum_devices()
        .iter()
        .map(|node| (node.index(), node.path().to_path_buf()))
        .collect();
    nodes.sort();

    let mut fallback = None;
    for (_, path) in nodes {
        let Ok(device) = v4l::Device::with_path(&path) else {
            continue;
        };
        let Ok(caps) = device.query_caps() else {
            continue;
        };
        if caps.driver != LOOPBACK_DRIVER {
            continue;
        }
        if !caps.capabilities.contains(v4l::capability::Flags::VIDEO_OUTPUT) {
            // exclusive_caps=1 and someone else is already writing to it.
            continue;
        }
        let found = LoopbackDevice {
            path: path.clone(),
            label: caps.card.clone(),
        };
        if caps.card == CAMERA_NODE_LABEL {
            return Ok(found);
        }
        fallback.get_or_insert(found);
    }

    fallback.ok_or_else(|| {
        VideoError::Unavailable(format!(
            "no v4l2loopback device found — load the module with: {MODPROBE_HINT}"
        ))
    })
}

/// Counters shared between the worker thread and status queries.
#[derive(Debug, Default)]
struct SinkCounters {
    /// Frames decoded and written to the device.
    written: AtomicU64,
    /// Frames dropped because they failed to decode.
    decode_failures: AtomicU64,
}

/// A running device worker.
struct Worker {
    frames: mpsc::SyncSender<Vec<u8>>,
    handle: std::thread::JoinHandle<()>,
}

/// The v4l2loopback implementation of [`VideoSink`].
///
/// `start` claims the device and negotiates the format on the calling thread (callers keep it
/// off their event loops); frames are decoded and written on a dedicated worker. Dropping the
/// sink stops it, so no writer outlives the app.
#[derive(Default)]
pub struct V4l2LoopbackSink {
    worker: Option<Worker>,
    counters: Arc<SinkCounters>,
    /// Where the running worker writes, for logs and the UI.
    device_path: Option<PathBuf>,
}

impl V4l2LoopbackSink {
    /// A sink that is not yet attached to any device.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The device node the running sink writes to.
    #[must_use]
    pub fn device_path(&self) -> Option<&std::path::Path> {
        self.device_path.as_deref()
    }

    /// Frames dropped because they failed to decode since `start`.
    #[must_use]
    pub fn decode_failures(&self) -> u64 {
        self.counters.decode_failures.load(Ordering::Relaxed)
    }
}

impl VideoSink for V4l2LoopbackSink {
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

        let target = find_loopback_device()?;

        // Fix the device's output format before any consumer probes it. The v4l handle is
        // only needed for the ioctl; frames go through a plain file descriptor below, and
        // v4l2loopback keeps the format per device, not per open.
        let device = v4l::Device::with_path(&target.path)
            .map_err(|e| VideoError::Unavailable(format!("{}: {e}", target.path.display())))?;
        let wanted = v4l::Format::new(format.width, format.height, FourCC::new(b"YU12"));
        let actual = device
            .set_format(&wanted)
            .map_err(|e| VideoError::Unavailable(format!("could not set the device format: {e}")))?;
        if (actual.width, actual.height) != (format.width, format.height)
            || actual.fourcc != FourCC::new(b"YU12")
        {
            return Err(VideoError::UnsupportedFormat(format!(
                "device countered with {}x{} {}",
                actual.width, actual.height, actual.fourcc
            )));
        }
        drop(device);

        let mut writer = std::fs::OpenOptions::new()
            .write(true)
            .open(&target.path)
            .map_err(|e| VideoError::Unavailable(format!("{}: {e}", target.path.display())))?;

        let (frame_tx, frame_rx) = mpsc::sync_channel::<Vec<u8>>(FRAME_QUEUE);
        let counters = Arc::clone(&self.counters);
        counters.written.store(0, Ordering::Relaxed);
        counters.decode_failures.store(0, Ordering::Relaxed);

        let path_for_log = target.path.clone();
        let handle = std::thread::Builder::new()
            .name("v4l2-camera-sink".to_owned())
            .spawn(move || {
                let mut write_failed = false;
                while let Ok(jpeg) = frame_rx.recv() {
                    let frame = match crate::decode_jpeg_to_i420(&jpeg, format.width, format.height)
                    {
                        Ok(frame) => frame,
                        Err(e) => {
                            // One bad frame must not end the stream, §7.2. Counted so the
                            // UI's fps figure reflects reality.
                            counters.decode_failures.fetch_add(1, Ordering::Relaxed);
                            tracing::debug!(error = %e, "camera frame dropped");
                            continue;
                        }
                    };
                    if let Err(e) = writer.write_all(&frame) {
                        if !write_failed {
                            // Log once: the module was likely unloaded under us. Frames keep
                            // draining so the sender never blocks; fps reads zero in the UI.
                            write_failed = true;
                            tracing::warn!(
                                device = %path_for_log.display(),
                                error = %e,
                                "virtual camera write failed; holding the stream drained"
                            );
                        }
                        continue;
                    }
                    write_failed = false;
                    counters.written.fetch_add(1, Ordering::Relaxed);
                }
            })
            .map_err(|e| VideoError::Unavailable(format!("could not spawn the sink worker: {e}")))?;

        tracing::info!(
            device = %target.path.display(),
            label = %target.label,
            width = format.width,
            height = format.height,
            "virtual camera attached"
        );
        self.worker = Some(Worker {
            frames: frame_tx,
            handle,
        });
        self.device_path = Some(target.path);
        Ok(())
    }

    fn push_frame(&mut self, jpeg: Vec<u8>) {
        if let Some(worker) = self.worker.as_ref() {
            // Full queue: drop the incoming frame. The device is behind; a newer frame will
            // arrive within max_fps anyway, and blocking here would stall the receive path.
            let _ = worker.frames.try_send(jpeg);
        }
    }

    fn frames_written(&self) -> u64 {
        self.counters.written.load(Ordering::Relaxed)
    }

    fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            // Closing the channel ends the worker loop; joining guarantees the writer fd is
            // closed — and the writer detached from the device — before stop returns.
            drop(worker.frames);
            let _ = worker.handle.join();
        }
        self.device_path = None;
    }
}

impl Drop for V4l2LoopbackSink {
    fn drop(&mut self) {
        self.stop();
    }
}
