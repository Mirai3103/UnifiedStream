//! Manual smoke test: stand the virtual source up for a few seconds so `pw-cli ls Node` /
//! pavucontrol can confirm "UnifiedStream Microphone" appears, plays a tone, and disappears
//! cleanly on exit.
//!
//! Run with: `cargo run -p unifiedstream-audio --example pw_smoke`

use std::sync::Arc;

use unifiedstream_audio::{AudioFormat, AudioSink, JitterBuffer, PipeWireSource};

fn main() {
    let buffer = Arc::new(JitterBuffer::default());
    let mut source = PipeWireSource::new(Arc::clone(&buffer));

    match source.start(AudioFormat::MICROPHONE) {
        Ok(()) => {
            println!("virtual source up; check `pw-cli ls Node` for UnifiedStream Microphone")
        }
        Err(e) => {
            eprintln!("could not start virtual source: {e}");
            std::process::exit(1);
        }
    }

    // Feed a 440 Hz tone in 20 ms frames for 5 seconds, like the receive path would.
    let format = AudioFormat::MICROPHONE;
    let mut phase: f32 = 0.0;
    let step = 440.0 * std::f32::consts::TAU / format.sample_rate as f32;
    for _ in 0..250 {
        let frame: Vec<i16> = (0..format.frame_samples)
            .map(|_| {
                phase += step;
                if phase > std::f32::consts::TAU {
                    phase -= std::f32::consts::TAU;
                }
                (phase.sin() * 8000.0) as i16
            })
            .collect();
        source.push(&frame);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let stats = buffer.stats();
    println!("jitter stats after 5s of tone: {stats:?}");
    source.stop();
    println!("virtual source stopped; the node must be gone now");
}
