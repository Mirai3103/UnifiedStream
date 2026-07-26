//! The PipeWire virtual source: a node named "UnifiedStream Microphone" that ordinary
//! applications can select as an input device.
//!
//! Implemented as a PipeWire output stream whose `media.class` is `Audio/Source` — the same
//! trick `pw-loopback` uses — so the graph treats our process as a capture device. PipeWire
//! pulls samples from the process callback, which drains the [`JitterBuffer`]; an empty buffer
//! plays silence, so a muted or momentarily lossy phone never stalls the graph.
//!
//! All libpipewire objects live on one dedicated thread: they are neither `Send` nor `Sync`,
//! and the main loop wants to own its thread anyway. The handle communicates with it through
//! the jitter buffer (audio) and a pipewire channel (shutdown).

use std::sync::mpsc;
use std::sync::Arc;

use pipewire::{self as pw, spa};

use crate::{AudioError, AudioFormat, AudioSink, JitterBuffer};

/// Node name applications see in their device pickers.
pub const NODE_NAME: &str = "UnifiedStream Microphone";

/// How long to wait for the PipeWire thread to report startup success or failure.
const STARTUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

struct Worker {
    quit: pw::channel::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

/// An [`AudioSink`] backed by a PipeWire `Audio/Source` node.
pub struct PipeWireSource {
    buffer: Arc<JitterBuffer>,
    worker: Option<Worker>,
}

impl PipeWireSource {
    /// A source that will drain the given jitter buffer once started.
    #[must_use]
    pub fn new(buffer: Arc<JitterBuffer>) -> Self {
        Self {
            buffer,
            worker: None,
        }
    }

    /// The buffer this source drains — the receive path pushes decoded frames into it.
    #[must_use]
    pub fn buffer(&self) -> Arc<JitterBuffer> {
        Arc::clone(&self.buffer)
    }
}

impl AudioSink for PipeWireSource {
    fn start(&mut self, format: AudioFormat) -> Result<(), AudioError> {
        if self.worker.is_some() {
            return Ok(());
        }
        if format.channels == 0 || format.sample_rate == 0 {
            return Err(AudioError::UnsupportedFormat(format!(
                "{}ch @ {}Hz",
                format.channels, format.sample_rate
            )));
        }

        self.buffer.clear();

        let (quit_tx, quit_rx) = pw::channel::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let buffer = Arc::clone(&self.buffer);

        let thread = std::thread::Builder::new()
            .name("pipewire-source".to_owned())
            .spawn(move || run_loop(&buffer, format, quit_rx, &ready_tx))
            .map_err(|e| AudioError::Unavailable(format!("could not spawn audio thread: {e}")))?;

        // The stream is usable the moment `connect` succeeds on the loop thread; wait for
        // that so a dead PipeWire daemon surfaces as a refusal, not a silent mic.
        match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(())) => {
                self.worker = Some(Worker {
                    quit: quit_tx,
                    thread,
                });
                tracing::info!(node = NODE_NAME, "virtual source created");
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

    fn push(&mut self, frame: &[i16]) {
        if self.worker.is_some() {
            self.buffer.push(frame.to_vec());
        }
    }

    fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.quit.send(());
            if worker.thread.join().is_err() {
                tracing::warn!("pipewire thread panicked during shutdown");
            }
            tracing::info!(node = NODE_NAME, "virtual source destroyed");
        }
        self.buffer.clear();
    }
}

impl Drop for PipeWireSource {
    fn drop(&mut self) {
        // The node must never outlive the app: an orphaned "UnifiedStream Microphone" in a
        // device picker would route silence into whatever selects it.
        self.stop();
    }
}

/// Everything that happens on the dedicated PipeWire thread.
fn run_loop(
    buffer: &Arc<JitterBuffer>,
    format: AudioFormat,
    quit_rx: pw::channel::Receiver<()>,
    ready_tx: &mpsc::Sender<Result<(), String>>,
) {
    match build_and_run(buffer, format, quit_rx, ready_tx) {
        Ok(()) => {}
        Err(message) => {
            // If setup failed before the ready signal, this send is the failure report; if it
            // failed after, the receiver is gone and the send is a harmless no-op.
            let _ = ready_tx.send(Err(message));
        }
    }
}

fn build_and_run(
    buffer: &Arc<JitterBuffer>,
    format: AudioFormat,
    quit_rx: pw::channel::Receiver<()>,
    ready_tx: &mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    pw::init();

    let mainloop =
        pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("main loop: {e}"))?;
    let context =
        pw::context::ContextRc::new(&mainloop, None).map_err(|e| format!("context: {e}"))?;
    let core = context
        .connect_rc(None)
        .map_err(|e| format!("PipeWire is not running: {e}"))?;

    let _quit_attachment = quit_rx.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |()| mainloop.quit()
    });

    let stride = usize::from(format.channels) * std::mem::size_of::<i16>();
    let latency = format!("{}/{}", format.frame_samples, format.sample_rate);

    let stream = pw::stream::StreamBox::new(
        &core,
        NODE_NAME,
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Communication",
            // What makes this a microphone rather than a playback stream.
            *pw::keys::MEDIA_CLASS => "Audio/Source",
            *pw::keys::NODE_NAME => "unifiedstream-microphone",
            *pw::keys::NODE_DESCRIPTION => NODE_NAME,
            // Hint the graph toward our 20 ms cadence so pulls match pushes.
            *pw::keys::NODE_LATENCY => latency.as_str(),
        },
    )
    .map_err(|e| format!("stream: {e}"))?;

    let _listener = stream
        .add_local_listener_with_user_data(Arc::clone(buffer))
        .process(move |stream, buffer| {
            let Some(mut pw_buffer) = stream.dequeue_buffer() else {
                return;
            };
            let requested = pw_buffer.requested();
            let datas = pw_buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };

            let filled_frames = if let Some(slice) = data.data() {
                let capacity_frames = slice.len() / stride;
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "frame counts are far below usize::MAX"
                )]
                let want_frames = if requested == 0 {
                    capacity_frames
                } else {
                    capacity_frames.min(requested as usize)
                };
                let sample_count = want_frames * usize::from(format.channels);

                // Drain the jitter buffer into a scratch vec, then serialize as S16LE.
                let mut samples = vec![0_i16; sample_count];
                buffer.pop_into(&mut samples);
                for (chunk, sample) in slice.chunks_exact_mut(2).zip(samples.iter()) {
                    chunk.copy_from_slice(&sample.to_le_bytes());
                }
                want_frames
            } else {
                0
            };

            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                reason = "stride and sizes are tiny"
            )]
            {
                *chunk.stride_mut() = stride as i32;
                *chunk.size_mut() = (filled_frames * stride) as u32;
            }
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
            spa::utils::Direction::Output,
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
