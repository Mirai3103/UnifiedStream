//! Manual smoke test: drive the camera frame transport over the real named section, with a
//! consumer reading it, and report what got through.
//!
//! Run with: `cargo run -p unifiedstream-video --example ring_smoke`
//!
//! This exists because the producer path has no other manual exercise. `VideoSink::start` refuses
//! on every machine until a filter is installed to read the ring, so without this the only thing a
//! person can observe by hand is the refusal.
//!
//! # Publish-only mode
//!
//! `cargo run -p unifiedstream-video --example ring_smoke -- --publish-only` publishes the same
//! verifiable pattern for a bounded run and attaches no consumer of its own, so the C++
//! `ring_conform.exe` can be the consumer instead. That pairing is the only thing in either build
//! that observes both halves of the transport at once: a wrong memory ordering in the C++ consumer
//! compiles cleanly, passes every static assertion it has, and tears rarely enough to reach users.
//!
//! It is a test harness, not a production path — nothing in `VideoSink` or `RingProducer` changes
//! for it.
//!
//! Three things are worth watching in the output:
//!
//! - every accepted frame is byte-identical to the one published, which is what the sequence
//!   protocol exists to guarantee;
//! - a consumer that cannot keep up loses whole frames to being lapped and never accepts one
//!   spliced from two — the count of frames "accepted with wrong contents" must always be zero;
//! - the heartbeat tells idle apart from stopped.
//!
//! Windows only, and gated rather than simply absent elsewhere: `cargo test --workspace` builds
//! examples, and it runs on every platform the workspace is verified on.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!(
        "ring_smoke exercises the Windows shared-memory frame transport and runs on Windows only"
    );
}

#[cfg(target_os = "windows")]
fn main() {
    if std::env::args().skip(1).any(|arg| arg == "--publish-only") {
        windows_smoke::publish_only();
    } else {
        windows_smoke::run();
    }
}

#[cfg(target_os = "windows")]
mod windows_smoke {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use unifiedstream_video::platform::CameraSection;
    use unifiedstream_video::transport::{
        Liveness, ReadOutcome, RingConsumer, RingProducer, HEARTBEAT_STALE_MS, RING_BYTES,
        SLOT_COUNT,
    };
    use unifiedstream_video::VideoFormat;

    /// Deliberately small, so the producer laps a slowed consumer within a few frames rather than
    /// after a copy long enough to hide the race.
    const FORMAT: VideoFormat = VideoFormat {
        width: 320,
        height: 240,
        max_fps: 30,
    };

    const FRAMES: u64 = 600;

    /// Geometry published in `--publish-only`. Small on purpose: the copy is short enough that the
    /// producer laps a consumer repeatedly within one run, which is the condition a torn read needs
    /// to be possible at all.
    const CONFORM_FORMAT: VideoFormat = VideoFormat {
        width: 320,
        height: 240,
        max_fps: 30,
    };

    /// Frames published in `--publish-only`. Large enough that a consumer starting a few
    /// milliseconds late still accepts thousands, and that the producer laps it many times over.
    const CONFORM_FRAMES: u64 = 20_000;

    /// How long `--publish-only` holds the section up, ticking, before it starts publishing.
    ///
    /// A conformance tool is started alongside this process rather than by it, so it needs a window
    /// in which the section exists and nothing is racing yet. Without one the whole run could be
    /// over before the consumer attached, and "accepted zero frames" is a failure — correctly, but
    /// for the wrong reason.
    const CONFORM_WARMUP_MS: u64 = 1_500;

    /// How long it keeps ticking after the last frame, so a consumer drains what it has not read
    /// before the stop is published.
    const CONFORM_DRAIN_MS: u64 = 500;

    /// Publish the verifiable pattern for a bounded run, with no consumer of this process's own.
    ///
    /// Deliberately not a variant of [`run`]: that function's value is that it reads what it wrote,
    /// and this one's is that it does not.
    pub fn publish_only() {
        let section = match CameraSection::open() {
            Ok(section) => section,
            Err(e) => {
                eprintln!("could not create the camera section: {e}");
                std::process::exit(1);
            }
        };

        let mut producer = match RingProducer::create(section.region(), CONFORM_FORMAT, tick()) {
            Ok(producer) => producer,
            Err(e) => {
                eprintln!("could not initialise the ring: {e}");
                std::process::exit(1);
            }
        };

        println!(
            "publishing {CONFORM_FRAMES} frames at {}x{} into the named section; \
             every byte of frame n is n mod 256",
            CONFORM_FORMAT.width, CONFORM_FORMAT.height
        );

        // Alive and idle, so a consumer can attach and see a running producer before the race
        // starts. The heartbeat is what keeps it from concluding the desktop is gone.
        let warmup_end = std::time::Instant::now() + Duration::from_millis(CONFORM_WARMUP_MS);
        while std::time::Instant::now() < warmup_end {
            producer.tick(tick());
            std::thread::sleep(Duration::from_millis(50));
        }

        // Then as fast as the copy allows, which is what makes a consumer lose frames to being
        // lapped. Losing them is correct; accepting a spliced one is not, and that is what the
        // conformance tool checks.
        let mut payload = vec![0_u8; CONFORM_FORMAT.i420_frame_bytes()];
        for n in 1..=CONFORM_FRAMES {
            payload.fill(byte_for(n));
            if let Err(e) = producer.publish(&payload, n, tick()) {
                eprintln!("publish failed at frame {n}: {e}");
                std::process::exit(1);
            }
        }

        let drain_end = std::time::Instant::now() + Duration::from_millis(CONFORM_DRAIN_MS);
        while std::time::Instant::now() < drain_end {
            producer.tick(tick());
            std::thread::sleep(Duration::from_millis(50));
        }

        // A clean stop, which is how the consumer knows the run is over rather than waiting out the
        // staleness bound.
        producer.stop();
        println!("published {}, stopped", producer.frames_published());

        // Hold the section open a moment longer so a consumer observes the stop through it rather
        // than through the mapping disappearing.
        std::thread::sleep(Duration::from_millis(500));
    }

    pub fn run() {
        // The real kernel objects: the named section, its security descriptor, and the
        // initialisation mutex. Running over ordinary memory would exercise the protocol the unit
        // tests already cover and none of the Windows-specific part this example exists for.
        let section = match CameraSection::open() {
            Ok(section) => section,
            Err(e) => {
                eprintln!("could not create the camera section: {e}");
                std::process::exit(1);
            }
        };
        let region = section.region();

        let mut producer = match RingProducer::create(region, FORMAT, tick()) {
            Ok(producer) => producer,
            Err(e) => {
                eprintln!("could not initialise the ring: {e}");
                std::process::exit(1);
            }
        };
        let mut consumer = match RingConsumer::attach(region) {
            Ok(consumer) => consumer,
            Err(e) => {
                eprintln!("a consumer could not attach: {e}");
                std::process::exit(1);
            }
        };

        println!(
            "ring up: {}x{}, {SLOT_COUNT} slots, {RING_BYTES} bytes",
            FORMAT.width, FORMAT.height
        );

        // Pass one: a consumer that keeps up. Everything published should be accepted intact.
        let mut accepted = 0_u64;
        let mut torn = 0_u64;
        let mut mismatched = 0_u64;
        let mut buffer = Vec::new();
        for n in 1..=FRAMES {
            let payload = vec![byte_for(n); FORMAT.i420_frame_bytes()];
            if let Err(e) = producer.publish(&payload, n, tick()) {
                eprintln!("publish failed at frame {n}: {e}");
                std::process::exit(1);
            }
            match consumer.read(&mut buffer) {
                ReadOutcome::Frame { sequence, .. } => {
                    if buffer.iter().any(|&b| b != byte_for(sequence)) {
                        mismatched += 1;
                    }
                    accepted += 1;
                }
                ReadOutcome::Torn | ReadOutcome::BeingWritten => torn += 1,
                ReadOutcome::NoFrame => {}
            }
        }
        println!("keeping up:  published {FRAMES}, accepted {accepted}, rejected {torn}");
        println!("             frames accepted with wrong contents: {mismatched}");
        assert_eq!(
            mismatched, 0,
            "an accepted frame must never be a wrong frame"
        );

        // Pass two: a consumer on its own thread, racing the producer.
        //
        // This is the only arrangement in which a torn read can happen at all. A single-threaded
        // reader always reads `latest`, and `latest` names a slot the producer has already
        // finished — so it can never catch one mid-write, and a sequential pass proves nothing
        // about the seqlock however far behind it pretends to be. Frames are 720p here to make
        // each copy long enough for the producer to land inside one.
        let racing = VideoFormat::CAMERA_720P;
        if let Err(e) = producer.set_geometry(racing) {
            eprintln!("could not switch to 720p: {e}");
            std::process::exit(1);
        }

        let finished = Arc::new(AtomicBool::new(false));
        let reader_finished = Arc::clone(&finished);
        let reader_region = region;
        let reader = std::thread::spawn(move || {
            let mut consumer = match RingConsumer::attach(reader_region) {
                Ok(consumer) => consumer,
                Err(e) => {
                    eprintln!("the racing consumer could not attach: {e}");
                    std::process::exit(1);
                }
            };
            let (mut ok, mut rejected, mut wrong) = (0_u64, 0_u64, 0_u64);
            let mut buffer = Vec::new();
            loop {
                match consumer.read(&mut buffer) {
                    ReadOutcome::Frame { sequence, .. } => {
                        if buffer.iter().any(|&b| b != byte_for(sequence)) {
                            wrong += 1;
                        }
                        ok += 1;
                    }
                    ReadOutcome::Torn | ReadOutcome::BeingWritten => rejected += 1,
                    ReadOutcome::NoFrame => {
                        if reader_finished.load(Ordering::Acquire) {
                            break;
                        }
                        std::thread::yield_now();
                    }
                }
            }
            (ok, rejected, wrong)
        });

        let raced_from = producer.frames_published();
        for n in 1..=FRAMES {
            let sequence = raced_from + n;
            let payload = vec![byte_for(sequence); racing.i420_frame_bytes()];
            if let Err(e) = producer.publish(&payload, sequence, tick()) {
                eprintln!("publish failed at frame {sequence}: {e}");
                std::process::exit(1);
            }
        }
        finished.store(true, Ordering::Release);
        let (raced_ok, raced_rejected, raced_wrong) = reader
            .join()
            .unwrap_or_else(|_| panic!("the reader panicked"));

        println!(
            "racing:      published {FRAMES} at 720p, accepted {raced_ok}, \
             rejected as torn or in-flight {raced_rejected}, lapped {}",
            FRAMES.saturating_sub(raced_ok + raced_rejected)
        );
        println!("             frames accepted with wrong contents: {raced_wrong}");
        assert_eq!(
            raced_wrong, 0,
            "a lapped reader must lose frames, never accept a spliced one"
        );

        println!(
            "             (zero torn reads is a normal result: the reader takes the slot `latest` \
             names, which the producer has already finished, so it only ever catches a write by \
             being lapped a full {SLOT_COUNT} slots mid-copy)"
        );

        // Liveness: idle, then stopped, then a producer that simply stopped ticking. Drain first,
        // or the frames pass two left unread would report as Streaming and say nothing about idle.
        while !matches!(consumer.read(&mut buffer), ReadOutcome::NoFrame) {}
        let now = tick();
        println!("while idle:   {:?}", consumer.liveness(now));
        producer.tick(now);
        println!("after a tick: {:?}", consumer.liveness(now));
        println!(
            "heartbeat stale: {:?}",
            consumer.liveness(now + HEARTBEAT_STALE_MS + 1)
        );
        producer.stop();
        let stopped = consumer.liveness(tick());
        println!("after stop:   {stopped:?}");
        assert_eq!(
            stopped,
            Liveness::Stopped,
            "a clean stop must be observed immediately, not waited out"
        );

        std::thread::sleep(Duration::from_millis(50));
        println!("ok");
    }

    /// Every byte of frame n is the same value, so any splice of two frames is visible.
    fn byte_for(sequence: u64) -> u8 {
        #[allow(clippy::cast_possible_truncation)]
        let byte = sequence as u8;
        byte
    }

    /// The same session-wide clock the sink measures its heartbeat on.
    fn tick() -> u64 {
        // SAFETY: no arguments, no out-parameters, callable from any thread at any time.
        unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
    }
}
