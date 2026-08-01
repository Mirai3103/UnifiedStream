//! The PipeWire virtual sink: a node named "UnifiedStream Speaker" that ordinary applications
//! can select as an output device, whose audio this process captures for the wire.
//!
//! Implemented as a PipeWire input stream whose `media.class` is `Audio/Sink` — the capture
//! half of the same trick the virtual source uses. Whatever the graph routes into the node
//! arrives in the process callback, is chunked into exact 20 ms frames, and is handed to the
//! caller's frame callback for transmission.
//!
//! All libpipewire objects live on one dedicated thread, exactly as in
//! [`crate::PipeWireSource`]: they are neither `Send` nor `Sync`, and the main loop wants to
//! own its thread anyway.

use std::sync::mpsc;

use pipewire::{self as pw, spa};

use crate::{AudioCapture, AudioError, AudioFormat, FrameChunker};

/// Node description applications see in their device pickers.
pub const SINK_NODE_NAME: &str = "UnifiedStream Speaker";

/// Machine-readable node name, used by routing tools (`pactl set-default-sink`).
pub const SINK_NODE_ID: &str = "unifiedstream-speaker";

/// How long to wait for the PipeWire thread to report startup success or failure.
const STARTUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Called from the PipeWire thread with each complete frame of captured samples.
pub type FrameCallback = Box<dyn FnMut(Vec<i16>) + Send>;

struct Worker {
    quit: pw::channel::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

/// An [`AudioCapture`] backed by a PipeWire `Audio/Sink` node.
pub struct PipeWireSpeakerSink {
    on_frame: Option<FrameCallback>,
    worker: Option<Worker>,
}

impl PipeWireSpeakerSink {
    /// A sink that will deliver captured frames to `on_frame` once started.
    #[must_use]
    pub fn new(on_frame: FrameCallback) -> Self {
        Self {
            on_frame: Some(on_frame),
            worker: None,
        }
    }
}

impl AudioCapture for PipeWireSpeakerSink {
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

        let (quit_tx, quit_rx) = pw::channel::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        let thread = std::thread::Builder::new()
            .name("pipewire-sink".to_owned())
            .spawn(move || run_loop(on_frame, format, quit_rx, &ready_tx))
            .map_err(|e| AudioError::Unavailable(format!("could not spawn audio thread: {e}")))?;

        // Wait for `connect` to succeed on the loop thread, so a dead PipeWire daemon surfaces
        // as a stream refusal rather than a virtual sink that silently never appears.
        match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(())) => {
                self.worker = Some(Worker {
                    quit: quit_tx,
                    thread,
                });
                tracing::info!(node = SINK_NODE_NAME, "virtual sink created");
                Ok(())
            }
            Ok(Err(message)) => {
                let _ = thread.join();
                Err(AudioError::Unavailable(message))
            }
            Err(_) => {
                // The thread is stuck talking to a wedged daemon; ask it to stop and move on.
                let _ = quit_tx.send(());
                Err(AudioError::Unavailable(
                    "timed out waiting for PipeWire".to_owned(),
                ))
            }
        }
    }

    fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.quit.send(());
            if worker.thread.join().is_err() {
                tracing::warn!("pipewire sink thread panicked during shutdown");
            }
            tracing::info!(node = SINK_NODE_NAME, "virtual sink destroyed");
        }
    }
}

impl Drop for PipeWireSpeakerSink {
    fn drop(&mut self) {
        // The node must never outlive the app: an orphaned "UnifiedStream Speaker" in a device
        // picker would swallow whatever audio is routed into it.
        self.stop();
    }
}

/// Everything that happens on the dedicated PipeWire thread.
fn run_loop(
    on_frame: FrameCallback,
    format: AudioFormat,
    quit_rx: pw::channel::Receiver<()>,
    ready_tx: &mpsc::Sender<Result<(), String>>,
) {
    match build_and_run(on_frame, format, quit_rx, ready_tx) {
        Ok(()) => {}
        Err(message) => {
            // If setup failed before the ready signal, this send is the failure report; if it
            // failed after, the receiver is gone and the send is a harmless no-op.
            let _ = ready_tx.send(Err(message));
        }
    }
}

fn build_and_run(
    mut on_frame: FrameCallback,
    format: AudioFormat,
    quit_rx: pw::channel::Receiver<()>,
    ready_tx: &mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("main loop: {e}"))?;
    let context =
        pw::context::ContextRc::new(&mainloop, None).map_err(|e| format!("context: {e}"))?;
    let core = context
        .connect_rc(None)
        .map_err(|e| format!("PipeWire is not running: {e}"))?;

    let _quit_attachment = quit_rx.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |()| mainloop.quit()
    });

    let latency = format!("{}/{}", format.frame_samples, format.sample_rate);

    let stream = pw::stream::StreamBox::new(
        &core,
        SINK_NODE_NAME,
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Music",
            // What makes this an output device rather than a recording stream.
            *pw::keys::MEDIA_CLASS => "Audio/Sink",
            *pw::keys::NODE_NAME => SINK_NODE_ID,
            *pw::keys::NODE_DESCRIPTION => SINK_NODE_NAME,
            // Hint the graph toward our 20 ms cadence so pushes match frames.
            *pw::keys::NODE_LATENCY => latency.as_str(),
        },
    )
    .map_err(|e| format!("stream: {e}"))?;

    let _listener = stream
        .add_local_listener_with_user_data(FrameChunker::new(format.total_samples()))
        .process(move |stream, chunker| {
            let Some(mut pw_buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = pw_buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };

            let offset = data.chunk().offset() as usize;
            let size = data.chunk().size() as usize;
            let Some(slice) = data.data() else {
                return;
            };
            let Some(valid) = slice.get(offset..offset + size) else {
                return;
            };

            // S16LE bytes off the graph, into host-order samples, into exact frames.
            let samples: Vec<i16> = valid
                .chunks_exact(2)
                .filter_map(|pair| match pair {
                    [low, high] => Some(i16::from_le_bytes([*low, *high])),
                    _ => None,
                })
                .collect();
            chunker.push(&samples, &mut on_frame);
        })
        .register()
        .map_err(|e| format!("listener: {e}"))?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::S16LE);
    audio_info.set_rate(format.sample_rate);
    audio_info.set_channels(u32::from(format.channels));

    let values = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: audio_info.into(),
        }),
    )
    .map_err(|e| format!("format pod: {e:?}"))?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&values)
        .ok_or_else(|| "format pod did not round-trip".to_owned())?];

    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| format!("connect: {e}"))?;

    ready_tx
        .send(Ok(()))
        .map_err(|e| format!("startup reporter gone: {e}"))?;

    mainloop.run();

    // Explicit teardown order: the stream must die before the core and context it borrows.
    drop(stream);
    Ok(())
}
