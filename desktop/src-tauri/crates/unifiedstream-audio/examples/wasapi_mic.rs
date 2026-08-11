//! Manual smoke test for the Windows microphone: resolve VB-CABLE's render endpoint, open it, and
//! play a tone into it for a few seconds so `CABLE Output` can be selected in any recording
//! application and heard.
//!
//! Run with: `cargo run -p unifiedstream-audio --example wasapi_mic`
//!
//! The resolver logs every VB-Audio playback device it considered and why each was accepted or
//! rejected, so this is also how the endpoint discrimination is *measured* on a machine — which
//! VB-CABLE release is installed, what its render endpoints are called, and how many channels each
//! declares. CI cannot do any of that: the Windows runners have no audio device at all.
//!
//! Windows only, and gated rather than simply absent elsewhere: `cargo test --workspace` builds
//! examples, and it runs on every platform the workspace is verified on.

#[cfg(target_os = "windows")]
use std::sync::Arc;

#[cfg(target_os = "windows")]
use unifiedstream_audio::{platform, AudioFormat, JitterBuffer};

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("wasapi_mic exercises the WASAPI render integration and runs on Windows only");
}

#[cfg(target_os = "windows")]
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "unifiedstream_audio=debug".into()),
        )
        .init();

    let format = AudioFormat::MICROPHONE;
    let buffer = Arc::new(JitterBuffer::default());
    let mut sink = platform::audio_sink(Arc::clone(&buffer));

    if let Err(e) = sink.start(format) {
        eprintln!("\nthe microphone refused to start:\n  {e}\n");
        std::process::exit(1);
    }
    println!("\nrendering a 440 Hz tone — select \"CABLE Output\" as a microphone to hear it\n");

    // 20 ms frames of a 440 Hz sine at a third of full scale, pushed at the cadence the network
    // would push them at. Five seconds, then a second of nothing so the underrun path runs too.
    let mut phase = 0.0_f32;
    let step = std::f32::consts::TAU * 440.0 / format.sample_rate as f32;
    for frame_index in 0..300 {
        let frame: Vec<i16> = (0..format.total_samples())
            .map(|_| {
                phase = (phase + step) % std::f32::consts::TAU;
                (phase.sin() * 10_000.0) as i16
            })
            .collect();
        sink.push(&frame);
        std::thread::sleep(std::time::Duration::from_millis(20));
        if frame_index % 50 == 0 {
            println!("  {}s: {:?}", frame_index / 50, buffer.stats());
        }
    }

    println!("\nsilence for one second — the endpoint should stay quiet, not repeat\n");
    std::thread::sleep(std::time::Duration::from_secs(1));
    println!("  final: {:?}", buffer.stats());

    sink.stop();
    println!("\nstopped cleanly");
}
