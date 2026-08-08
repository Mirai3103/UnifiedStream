//! The frame transport between the desktop and a virtual camera that consuming applications load
//! into their own processes.
//!
//! On Linux the desktop writes frames to a device node and the kernel does the rest. Where the
//! virtual camera is instead a component each application loads — a DirectShow filter on Windows —
//! the frames have to leave this process, and every application that selects the camera becomes an
//! independent reader of the same memory. This module is that boundary, and it is the normative
//! statement of the protocol: a C++ consumer is written against this file.
//!
//! Three properties shape everything below.
//!
//! **The producer never waits.** A consumer may be descheduled mid-copy, may be killed mid-copy,
//! and must never hold the receive path up. So the producer writes unconditionally and the reader
//! detects that it lost the race, through a per-slot seqlock. A lost race costs exactly one frame,
//! which is what the camera pipeline already tolerates everywhere else.
//!
//! **One producer serves consumers of both architectures.** A 32-bit application loads a 32-bit
//! filter, so every offset in the layout must be identical in a 32-bit and a 64-bit process. No
//! pointers, no `usize`, no field whose size or alignment varies — and the layout is asserted at
//! compile time rather than trusted.
//!
//! **There is no Win32 in here.** The header layout, the sequence protocol, the slot arithmetic,
//! and the version and geometry checks are portable code over a plain region of bytes, because a
//! seqlock with a missing memory ordering produces a rare corrupt frame rather than a test
//! failure — it has to be covered by the test leg that runs on every platform, not only by the one
//! that can create a section. Only mapping creation, the security descriptor, the init mutex, and
//! the registry probe are target-gated, and they live in `platform::windows`.

use std::mem::{align_of, offset_of, size_of};
use std::sync::atomic::{fence, AtomicU64, Ordering};

use crate::VideoFormat;

/// COM class identifier of the DirectShow filter that consumes this ring.
///
/// **This is the contract's identity, and it is permanent.** The follow-up change
/// `add-windows-directshow-camera` registers this exact value under
/// `CLSID_VideoInputDeviceCategory`, and the desktop probes for it to decide whether a virtual
/// camera exists at all. A mismatch between the two is caught by neither build — not by the Rust
/// compiler, which never sees the filter, and not by the C++ compiler, which never sees this file
/// — so the value is written down here, in the shared document, rather than agreed later.
pub const FILTER_CLSID: &str = "{6D8DD393-D871-4498-A24F-4AFFEACFC106}";

/// Registry value, on the filter's CLSID key, holding the transport version that filter speaks.
///
/// Part of the same contract as [`FILTER_CLSID`] and written by the same follow-up change. It
/// exists because a version disagreement has to be reported *before* a stream starts: the desktop
/// is the producer, so it never reads a header a filter wrote and could not otherwise discover
/// that the installed component speaks a layout it does not. Registration records the answer where
/// the availability probe already looks.
pub const FILTER_VERSION_VALUE: &str = "TransportVersion";

/// Where `add-windows-directshow-camera` installs the filter, and the names it installs it under.
///
/// Contract with that change in the same way [`FILTER_CLSID`] is, and for the same reason: the
/// refusal this change ships has to name the command that resolves it, and a command needs a path.
/// Both architectures are required — a 32-bit application can only load a 32-bit filter.
pub const FILTER_INSTALL_DIR: &str = r"%ProgramFiles%\UnifiedStream";
/// File name of the 64-bit filter. See [`FILTER_INSTALL_DIR`].
pub const FILTER_DLL_X64: &str = "UnifiedStreamCamera64.dll";
/// File name of the 32-bit filter. See [`FILTER_INSTALL_DIR`].
pub const FILTER_DLL_X86: &str = "UnifiedStreamCamera32.dll";

/// Friendly name the registered filter carries, and the label the desktop shows for the device.
///
/// The same string the Linux sink's device path occupies in the UI, which is why [`crate::VideoSink`]
/// specifies an opaque platform-supplied label rather than a path — and the same string the Linux
/// setup guidance asks for as a card label, so the camera has one name across the product.
pub const FILTER_FRIENDLY_NAME: &str = crate::CAMERA_NODE_LABEL;

/// `b"USVC"`: this mapping belongs to UnifiedStream's virtual camera.
///
/// Separate from [`TRANSPORT_VERSION`] rather than folded into one tagged word, because "this is
/// not our mapping" and "this is ours and I am too old for it" have to produce different messages
/// to the user.
pub const MAGIC: u32 = u32::from_le_bytes(*b"USVC");

/// Layout version of everything in this module.
///
/// Bumped by any change to the header, the slot header, or the sequence protocol. A consumer built
/// against a different version declines rather than interpreting a layout it does not understand —
/// see [`AttachError::Version`].
pub const TRANSPORT_VERSION: u32 = 1;

/// `b"I420"`: planar YUV 4:2:0, the only payload format this version carries.
///
/// The desktop decodes JPEG on its own side of the boundary, so the component running inside other
/// applications' processes never contains a decoder fed by network-originated bytes.
pub const FOURCC_I420: u32 = u32::from_le_bytes(*b"I420");

/// Slots in the ring.
///
/// Four gives a consumer roughly three frame intervals — 100 ms at 30 fps — to complete a copy
/// before the producer laps it, and being lapped is detected rather than silently tolerated.
pub const SLOT_COUNT: u32 = 4;

/// Widest geometry the ring is allocated for: the application's existing cap.
pub const MAX_WIDTH: u32 = 1920;
/// Tallest geometry the ring is allocated for: the application's existing cap.
pub const MAX_HEIGHT: u32 = 1080;

/// Granularity every offset in the mapping is aligned to.
///
/// Slots start on a page so a consumer's copy out of one never shares a page with the slot the
/// producer is writing.
const PAGE_BYTES: usize = 4096;

/// Bytes one I420 frame occupies at [`MAX_WIDTH`] x [`MAX_HEIGHT`].
const MAX_PAYLOAD_BYTES: usize = (MAX_WIDTH as usize * MAX_HEIGHT as usize) * 3 / 2;

/// Round `value` up to the next multiple of `to`. `to` is a non-zero power of two here.
const fn align_up(value: usize, to: usize) -> usize {
    value.div_ceil(to) * to
}

/// Offset of slot 0 from the mapping base: the header, padded to a page.
pub const HEADER_BYTES: usize = PAGE_BYTES;

/// Stride from one slot to the next, its own header included.
pub const SLOT_BYTES: usize = align_up(size_of::<SlotHeader>() + MAX_PAYLOAD_BYTES, PAGE_BYTES);

/// Total bytes the mapping must be, about 11.9 MiB.
///
/// Sized once for the maximum geometry rather than for the negotiated one. A consumer holding the
/// section keeps it alive, so `CreateFileMappingW` against a name a surviving consumer still holds
/// returns that same, wrongly sized section — and the consumer never learns a new one exists.
/// A single fixed allocation removes the failure entirely, at the cost of twelve megabytes of
/// pagefile-backed commit while a stream runs.
pub const RING_BYTES: usize = HEADER_BYTES + SLOT_COUNT as usize * SLOT_BYTES;

/// [`RingHeader::flags`] bit 0: a camera stream is running.
const FLAG_RUNNING: u32 = 1 << 0;

/// How often the producer advances its heartbeat while a stream is running but idle.
///
/// A consumer cannot otherwise tell a quiet stream from a dead desktop, and the two call for
/// different pictures on screen.
pub const HEARTBEAT_INTERVAL_MS: u64 = 250;

/// How far the heartbeat may fall behind before a consumer concludes the producer is gone.
pub const HEARTBEAT_STALE_MS: u64 = 2_000;

/// The fixed header at the base of the mapping.
///
/// `#[repr(C)]` with explicitly sized, naturally aligned integers and no pointer-width field, so
/// every offset is the same in a 32-bit and a 64-bit process. The assertions below the definition
/// are what keep it that way.
#[repr(C)]
pub struct RingHeader {
    /// [`MAGIC`]. Present so a consumer can tell a foreign mapping from an incompatible one.
    ///
    /// Written last during initialisation and cleared first, so a consumer attaching while the
    /// producer is mid-initialisation sees "not ours" rather than a half-written header.
    pub magic: u32,
    /// [`TRANSPORT_VERSION`]. Present so a stale filter refuses instead of misreading.
    pub version: u32,
    /// Offset of slot 0 from the mapping base. Present so the consumer never hard-codes a value
    /// this file could change.
    pub header_bytes: u32,
    /// Slots in the ring, for the same reason as `header_bytes`.
    pub slot_count: u32,
    /// Stride from one slot to the next, for the same reason as `header_bytes`.
    pub slot_bytes: u32,
    /// [`FOURCC_I420`]. Present so a future payload format is a disagreement a consumer can
    /// report rather than a picture it renders wrongly.
    pub fourcc: u32,
    /// Width of the *current* stream, not of the allocation. Present because the mapping is sized
    /// for the maximum geometry and says nothing about what is being published into it.
    pub width: u32,
    /// Height of the current stream, for the same reason as `width`.
    pub height: u32,
    /// Bumped whenever geometry changes or a stream starts or stops. Present so a consumer that
    /// caches geometry knows exactly when to re-read it, instead of misreading a resolution change
    /// as a stream of malformed frames.
    pub generation: AtomicU64,
    /// Sequence of the most recently published frame; 0 before the first. Present so a consumer
    /// finds the freshest frame without scanning the slots.
    pub latest: AtomicU64,
    /// The producer's monotonic tick, in milliseconds. Present so "idle" and "gone" are different
    /// states rather than both being an absence of new frames.
    pub heartbeat_ms: AtomicU64,
    /// The producing process. Present for diagnosis only — nothing in the protocol reads it.
    pub producer_pid: u32,
    /// [`FLAG_RUNNING`] in bit 0. Present so a clean stop is observed immediately rather than
    /// after the heartbeat times out.
    pub flags: u32,
}

/// The header on each slot, immediately followed by that slot's payload.
#[repr(C)]
pub struct SlotHeader {
    /// Seqlock version of this slot: odd while the producer is writing it, even when stable.
    ///
    /// Counts writes to *this slot*, not frames — the frame number is [`RingHeader::latest`],
    /// which is how the consumer chose the slot in the first place. It exists so a reader can
    /// tell that the producer wrote here during its copy, which is the only way to detect a torn
    /// read without ever making the producer wait.
    pub sequence: AtomicU64,
    /// Capture time carried through from the media header, so the consumer can pace delivery.
    pub timestamp_us: u64,
    /// Payload bytes actually written. Present because the slot is sized for the maximum geometry
    /// and the current frame is usually smaller.
    pub bytes: u32,
    /// Reserved, zeroed. Present so a field can be added within this version's slot header
    /// without moving the payload.
    pub reserved: [u32; 3],
}

// The layout is the contract with a C++ consumer neither compiler checks. Asserting it here means
// a change that would break that consumer fails the Rust build first, and cannot be made silently.
const _: () = assert!(size_of::<RingHeader>() == 64);
const _: () = assert!(align_of::<RingHeader>() == 8);
const _: () = assert!(offset_of!(RingHeader, magic) == 0);
const _: () = assert!(offset_of!(RingHeader, version) == 4);
const _: () = assert!(offset_of!(RingHeader, header_bytes) == 8);
const _: () = assert!(offset_of!(RingHeader, slot_count) == 12);
const _: () = assert!(offset_of!(RingHeader, slot_bytes) == 16);
const _: () = assert!(offset_of!(RingHeader, fourcc) == 20);
const _: () = assert!(offset_of!(RingHeader, width) == 24);
const _: () = assert!(offset_of!(RingHeader, height) == 28);
const _: () = assert!(offset_of!(RingHeader, generation) == 32);
const _: () = assert!(offset_of!(RingHeader, latest) == 40);
const _: () = assert!(offset_of!(RingHeader, heartbeat_ms) == 48);
const _: () = assert!(offset_of!(RingHeader, producer_pid) == 56);
const _: () = assert!(offset_of!(RingHeader, flags) == 60);

const _: () = assert!(size_of::<SlotHeader>() == 32);
const _: () = assert!(align_of::<SlotHeader>() == 8);
const _: () = assert!(offset_of!(SlotHeader, sequence) == 0);
const _: () = assert!(offset_of!(SlotHeader, timestamp_us) == 8);
const _: () = assert!(offset_of!(SlotHeader, bytes) == 16);
const _: () = assert!(offset_of!(SlotHeader, reserved) == 20);

// The header must fit in its page, and a slot must hold the largest frame this build allows.
const _: () = assert!(size_of::<RingHeader>() <= HEADER_BYTES);
const _: () = assert!(SLOT_BYTES >= size_of::<SlotHeader>() + MAX_PAYLOAD_BYTES);
const _: () = assert!(SLOT_BYTES % PAGE_BYTES == 0);

/// Why the ring could not be created, or a frame could not be published into it.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TransportError {
    /// The mapping handed to the producer is smaller than [`RING_BYTES`].
    #[error("shared region is {len} bytes, the ring needs {needed}")]
    RegionTooSmall {
        /// Bytes the region actually has.
        len: usize,
        /// Bytes the ring needs, always [`RING_BYTES`].
        needed: usize,
    },
    /// The geometry is larger than the fixed allocation can carry.
    #[error("{width}x{height} exceeds the {MAX_WIDTH}x{MAX_HEIGHT} the ring is allocated for")]
    GeometryTooLarge {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// I420 subsamples chroma 2x2, so odd geometry cannot be represented.
    #[error("dimensions must be even, got {width}x{height}")]
    OddGeometry {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// A frame whose length is not what the current geometry calls for.
    #[error("frame is {got} bytes, {want} expected at the current geometry")]
    FrameSize {
        /// Bytes the caller offered.
        got: usize,
        /// Bytes an I420 frame occupies at the geometry the header advertises.
        want: usize,
    },
}

/// Why a consumer declined to read a mapping.
///
/// Three outcomes rather than one, because they are three different messages to the user: someone
/// else's mapping is a name collision, a version disagreement is a component that needs updating,
/// and a malformed header is a bug. None of them is a picture rendered wrongly.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AttachError {
    /// The magic is not [`MAGIC`]: this mapping does not belong to UnifiedStream, or the producer
    /// is part-way through initialising it.
    #[error("not a UnifiedStream camera mapping (magic {found:#010x})")]
    NotOurs {
        /// The magic actually found.
        found: u32,
    },
    /// Ours, and a layout this build does not understand. The consumer must decline rather than
    /// interpret it — a version disagreement is the component being unusable, exactly as an absent
    /// component is, and never a stream that merely produces incorrect video.
    #[error("mapping is transport version {found}, this build speaks version {expected}")]
    Version {
        /// Version the mapping advertises.
        found: u32,
        /// Version this build was compiled against, always [`TRANSPORT_VERSION`].
        expected: u32,
    },
    /// Ours, our version, and internally inconsistent.
    #[error("mapping is malformed: {0}")]
    Malformed(String),
    /// The mapping handed to the consumer is smaller than [`RING_BYTES`].
    #[error("shared region is {len} bytes, the ring needs {needed}")]
    RegionTooSmall {
        /// Bytes the region actually has.
        len: usize,
        /// Bytes the ring needs, always [`RING_BYTES`].
        needed: usize,
    },
}

/// A region of memory shared between the producing desktop and every consuming application.
///
/// Deliberately a raw base and length rather than a slice: the bytes are concurrently written by
/// another process, which no Rust reference type describes. Everything that touches the region
/// goes through the accessors below, which use atomics for the fields the protocol synchronises on
/// and volatile access for the rest, so the compiler never assumes a value it read once is still
/// current.
#[derive(Clone, Copy)]
pub struct RingRegion {
    base: *mut u8,
    len: usize,
}

// SAFETY: the region is shared memory. Being reachable from several threads — and several
// processes — is the entire point of it, and every access goes through the atomic and volatile
// accessors below rather than through a reference to the underlying bytes.
unsafe impl Send for RingRegion {}
// SAFETY: as above.
unsafe impl Sync for RingRegion {}

impl RingRegion {
    /// Describe a mapped region of at least [`RING_BYTES`] bytes.
    ///
    /// # Safety
    ///
    /// `base` must point at `len` readable bytes that stay mapped for as long as this value, or
    /// anything derived from it, is used, and must additionally be writable if a [`RingProducer`]
    /// is created over it. A [`RingConsumer`] only ever reads, which is what lets a consuming
    /// application map the section read-only — and it must, since the section grants consumers
    /// read access alone. The bytes may be concurrently accessed by other processes; nothing else
    /// in this process may hold a reference to them.
    #[must_use]
    pub const unsafe fn new(base: *mut u8, len: usize) -> Self {
        Self { base, len }
    }

    /// Bytes in the region.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the region is empty. Present because clippy asks for it beside [`Self::len`].
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Read one `u32` header field, named by its offset.
    ///
    /// Volatile because another process writes these, and a plain read would let the compiler hoist
    /// or reuse a value that has since changed.
    fn read_u32(&self, field_offset: usize) -> u32 {
        // SAFETY: `field_offset` is an `offset_of!` into `RingHeader`, which the region is large
        // enough to hold; the field is 4-byte aligned by the layout assertions above.
        unsafe { self.base.add(field_offset).cast::<u32>().read_volatile() }
    }

    /// Write one `u32` header field, named by its offset.
    fn write_u32(&self, field_offset: usize, value: u32) {
        // SAFETY: as [`Self::read_u32`].
        unsafe {
            self.base
                .add(field_offset)
                .cast::<u32>()
                .write_volatile(value);
        }
    }

    /// One of the header's `u64` fields, as the atomic the protocol synchronises on.
    fn header_atomic(&self, field_offset: usize) -> &AtomicU64 {
        // SAFETY: `field_offset` is an `offset_of!` into `RingHeader`, 8-byte aligned by the
        // layout assertions above, and within a region of at least `RING_BYTES` bytes. Atomic
        // access is the only access made to these four fields, here and in the C++ consumer.
        unsafe { &*self.base.add(field_offset).cast::<AtomicU64>() }
    }

    /// Base of slot `index`, which callers reduce modulo [`SLOT_COUNT`] first.
    fn slot(&self, index: u64) -> *mut u8 {
        let offset = HEADER_BYTES + (index % u64::from(SLOT_COUNT)) as usize * SLOT_BYTES;
        // SAFETY: `offset` is at most `RING_BYTES - SLOT_BYTES` by construction.
        unsafe { self.base.add(offset) }
    }

    /// The seqlock version of slot `index`.
    fn slot_sequence(&self, index: u64) -> &AtomicU64 {
        // SAFETY: `sequence` is at offset 0 of a page-aligned slot, so it is 8-byte aligned, and
        // the slot lies wholly inside the region.
        unsafe { &*self.slot(index).cast::<AtomicU64>() }
    }

    /// Where slot `index`'s payload starts.
    fn slot_payload(&self, index: u64) -> *mut u8 {
        // SAFETY: the payload begins one slot header in, and the slot is `SLOT_BYTES` long.
        unsafe { self.slot(index).add(size_of::<SlotHeader>()) }
    }

    /// Read one `u32` field of slot `index`'s header, named by its offset within [`SlotHeader`].
    fn read_slot_u32(&self, index: u64, field_offset: usize) -> u32 {
        // SAFETY: an `offset_of!` into `SlotHeader`, 4-byte aligned within a page-aligned slot.
        unsafe {
            self.slot(index)
                .add(field_offset)
                .cast::<u32>()
                .read_volatile()
        }
    }

    /// Write one `u32` field of slot `index`'s header.
    fn write_slot_u32(&self, index: u64, field_offset: usize, value: u32) {
        // SAFETY: as [`Self::read_slot_u32`].
        unsafe {
            self.slot(index)
                .add(field_offset)
                .cast::<u32>()
                .write_volatile(value);
        }
    }

    /// Read slot `index`'s timestamp.
    fn read_slot_timestamp(&self, index: u64) -> u64 {
        // SAFETY: `timestamp_us` is at offset 8 of a page-aligned slot, so it is 8-byte aligned.
        unsafe {
            self.slot(index)
                .add(offset_of!(SlotHeader, timestamp_us))
                .cast::<u64>()
                .read_volatile()
        }
    }

    /// Write slot `index`'s timestamp.
    fn write_slot_timestamp(&self, index: u64, value: u64) {
        // SAFETY: as [`Self::read_slot_timestamp`].
        unsafe {
            self.slot(index)
                .add(offset_of!(SlotHeader, timestamp_us))
                .cast::<u64>()
                .write_volatile(value);
        }
    }
}

/// Bytes an I420 frame occupies at `width` x `height`.
const fn i420_bytes(width: u32, height: u32) -> usize {
    let pixels = width as usize * height as usize;
    pixels + pixels / 2
}

/// The desktop's half of the transport: the single writer into the ring.
///
/// Never waits for a consumer and never reasons about one. Publishing is a claim on the next slot,
/// a copy, and two stores; whether anyone read the previous frame does not come into it.
pub struct RingProducer {
    region: RingRegion,
    /// Geometry the header currently advertises; frames must match it.
    format: VideoFormat,
    /// Sequence the next published frame will carry. Frames are numbered from 1, so
    /// [`RingHeader::latest`] of 0 unambiguously means "nothing published yet".
    next_sequence: u64,
    /// When the heartbeat was last advanced, so the idle path can honour
    /// [`HEARTBEAT_INTERVAL_MS`] without storing on every pass.
    last_heartbeat_ms: u64,
}

impl RingProducer {
    /// Initialise the ring in `region` for `format` and mark a stream running.
    ///
    /// Reclaims rather than assumes: a named section a surviving consumer still holds comes back
    /// from `CreateFileMappingW` as the *existing* section, so an already-initialised header is a
    /// normal case. Its generation is carried forward and bumped, which is what tells that
    /// consumer to re-read everything it cached.
    ///
    /// The caller holds the initialisation mutex across this call, so two desktop instances cannot
    /// race the header into an inconsistent state.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] when the region is too small or the geometry is one this
    /// allocation cannot carry.
    pub fn create(
        region: RingRegion,
        format: VideoFormat,
        now_ms: u64,
    ) -> Result<Self, TransportError> {
        if region.len() < RING_BYTES {
            return Err(TransportError::RegionTooSmall {
                len: region.len(),
                needed: RING_BYTES,
            });
        }
        check_geometry(format)?;

        // Carry the generation forward when this is a section a consumer kept alive, so its cached
        // view is invalidated exactly once rather than appearing unchanged across a restart.
        let previous_generation = if region.read_u32(offset_of!(RingHeader, magic)) == MAGIC
            && region.read_u32(offset_of!(RingHeader, version)) == TRANSPORT_VERSION
        {
            region
                .header_atomic(offset_of!(RingHeader, generation))
                .load(Ordering::Acquire)
        } else {
            0
        };

        // Clear the magic first: a consumer attaching during the writes below must see "not our
        // mapping" and retry, never a header that is half this producer's and half the last one's.
        region.write_u32(offset_of!(RingHeader, magic), 0);
        fence(Ordering::Release);

        region.write_u32(offset_of!(RingHeader, version), TRANSPORT_VERSION);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "both are compile-time constants far below u32::MAX"
        )]
        {
            region.write_u32(offset_of!(RingHeader, header_bytes), HEADER_BYTES as u32);
            region.write_u32(offset_of!(RingHeader, slot_bytes), SLOT_BYTES as u32);
        }
        region.write_u32(offset_of!(RingHeader, slot_count), SLOT_COUNT);
        region.write_u32(offset_of!(RingHeader, fourcc), FOURCC_I420);
        region.write_u32(offset_of!(RingHeader, width), format.width);
        region.write_u32(offset_of!(RingHeader, height), format.height);
        region.write_u32(offset_of!(RingHeader, producer_pid), std::process::id());
        region.write_u32(offset_of!(RingHeader, flags), FLAG_RUNNING);

        // Every slot restarts even and empty. A reclaimed section carries the previous producer's
        // sequences, and an odd one left behind by a process that died mid-write would stall a
        // consumer on that slot forever.
        for index in 0..u64::from(SLOT_COUNT) {
            region
                .slot_sequence(index)
                .store(0, std::sync::atomic::Ordering::Relaxed);
            region.write_slot_u32(index, offset_of!(SlotHeader, bytes), 0);
            region.write_slot_timestamp(index, 0);
            for reserved in 0..3_usize {
                region.write_slot_u32(
                    index,
                    offset_of!(SlotHeader, reserved) + reserved * size_of::<u32>(),
                    0,
                );
            }
        }

        region
            .header_atomic(offset_of!(RingHeader, latest))
            .store(0, Ordering::Relaxed);
        region
            .header_atomic(offset_of!(RingHeader, heartbeat_ms))
            .store(now_ms, Ordering::Relaxed);

        // Publish the initialised header: the generation bump releases every write above, and the
        // magic is written last so a consumer that sees it sees a complete header.
        region
            .header_atomic(offset_of!(RingHeader, generation))
            .store(previous_generation + 1, Ordering::Release);
        fence(Ordering::Release);
        region.write_u32(offset_of!(RingHeader, magic), MAGIC);

        Ok(Self {
            region,
            format,
            next_sequence: 1,
            last_heartbeat_ms: now_ms,
        })
    }

    /// Publish one decoded I420 frame, returning the sequence it was given.
    ///
    /// Never blocks and never fails because of a consumer. A reader part-way through copying the
    /// slot this frame lands in loses that read and detects it; nothing about that is visible here.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::FrameSize`] when the frame is not an I420 frame at the geometry
    /// the header advertises.
    pub fn publish(
        &mut self,
        frame: &[u8],
        timestamp_us: u64,
        now_ms: u64,
    ) -> Result<u64, TransportError> {
        let want = i420_bytes(self.format.width, self.format.height);
        if frame.len() != want {
            return Err(TransportError::FrameSize {
                got: frame.len(),
                want,
            });
        }

        let sequence = self.next_sequence;
        let slot = sequence;
        let version = self.region.slot_sequence(slot);

        // Relaxed: this is the only writer, so nothing here races another store to it.
        let stable = version.load(Ordering::Relaxed);
        version.store(stable.wrapping_add(1), Ordering::Relaxed);
        // Pairs with the consumer's Acquire fence after its copy: the odd version must be visible
        // before any payload byte is, or a reader could copy new bytes and still see the old
        // even version.
        fence(Ordering::Release);

        // SAFETY: `frame` is `want` bytes and the payload area of a slot is `MAX_PAYLOAD_BYTES`,
        // which `check_geometry` guarantees is at least `want`. The regions cannot overlap: one is
        // this process's heap, the other the shared mapping.
        unsafe {
            std::ptr::copy_nonoverlapping(
                frame.as_ptr(),
                self.region.slot_payload(slot),
                frame.len(),
            );
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "bounded by MAX_PAYLOAD_BYTES, far below u32::MAX"
        )]
        self.region
            .write_slot_u32(slot, offset_of!(SlotHeader, bytes), frame.len() as u32);
        self.region.write_slot_timestamp(slot, timestamp_us);

        // Release: everything written above is visible to any consumer that reads this even
        // version with Acquire and finds it unchanged after its copy.
        version.store(stable.wrapping_add(2), Ordering::Release);

        // Release, and after the slot: a consumer reads `latest` first, so it must not be able to
        // see this sequence before the slot that holds it is stable.
        self.region
            .header_atomic(offset_of!(RingHeader, latest))
            .store(sequence, Ordering::Release);
        self.region
            .header_atomic(offset_of!(RingHeader, heartbeat_ms))
            .store(now_ms, Ordering::Release);
        self.last_heartbeat_ms = now_ms;

        self.next_sequence += 1;
        Ok(sequence)
    }

    /// Advance the heartbeat if [`HEARTBEAT_INTERVAL_MS`] has passed, so a stream that is running
    /// but idle stays distinguishable from a desktop that has died.
    ///
    /// Called on the worker's idle path. Publishing advances the heartbeat too, so a busy stream
    /// never reaches the store below.
    pub fn tick(&mut self, now_ms: u64) {
        if now_ms.saturating_sub(self.last_heartbeat_ms) < HEARTBEAT_INTERVAL_MS {
            return;
        }
        self.region
            .header_atomic(offset_of!(RingHeader, heartbeat_ms))
            .store(now_ms, Ordering::Release);
        self.last_heartbeat_ms = now_ms;
    }

    /// Re-advertise the ring at a new geometry.
    ///
    /// The application layer stops and restarts the sink on a resolution change, so this is not on
    /// the ordinary path; it exists because the generation contract is defined in terms of geometry
    /// changing, and a consumer must observe one however it arises.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] when the geometry is one this allocation cannot carry.
    pub fn set_geometry(&mut self, format: VideoFormat) -> Result<(), TransportError> {
        check_geometry(format)?;
        self.region
            .write_u32(offset_of!(RingHeader, width), format.width);
        self.region
            .write_u32(offset_of!(RingHeader, height), format.height);
        self.format = format;
        // Release: the two writes above must be visible to any consumer that notices the bump.
        self.region
            .header_atomic(offset_of!(RingHeader, generation))
            .fetch_add(1, Ordering::Release);
        Ok(())
    }

    /// Mark the stream stopped.
    ///
    /// Clearing the flag and bumping the generation is what makes a clean stop observable
    /// immediately, instead of a consumer waiting out [`HEARTBEAT_STALE_MS`] to discover it.
    pub fn stop(&mut self) {
        self.region.write_u32(offset_of!(RingHeader, flags), 0);
        self.region
            .header_atomic(offset_of!(RingHeader, generation))
            .fetch_add(1, Ordering::Release);
    }

    /// Frames published since this producer was created.
    #[must_use]
    pub const fn frames_published(&self) -> u64 {
        self.next_sequence - 1
    }

    /// The geometry the header currently advertises.
    #[must_use]
    pub const fn format(&self) -> VideoFormat {
        self.format
    }
}

/// Reject geometry the fixed allocation cannot carry, or that I420 cannot represent.
fn check_geometry(format: VideoFormat) -> Result<(), TransportError> {
    if format.width % 2 != 0 || format.height % 2 != 0 {
        return Err(TransportError::OddGeometry {
            width: format.width,
            height: format.height,
        });
    }
    if format.width > MAX_WIDTH || format.height > MAX_HEIGHT {
        return Err(TransportError::GeometryTooLarge {
            width: format.width,
            height: format.height,
        });
    }
    Ok(())
}

/// What one read attempt produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    /// A complete frame, copied into the caller's buffer.
    Frame {
        /// Sequence the producer gave it.
        sequence: u64,
        /// Capture time carried from the media header.
        timestamp_us: u64,
        /// Width the payload is in, which is the geometry current when it was published.
        width: u32,
        /// Height the payload is in.
        height: u32,
    },
    /// Nothing has been published, or nothing new since the last frame this consumer accepted.
    NoFrame,
    /// The producer holds the slot right now. Costs this attempt, not the stream.
    BeingWritten,
    /// The producer wrote the slot during the copy. The bytes are discarded unread rather than
    /// delivered as a frame that is part one picture and part another.
    Torn,
}

/// A consuming application's half of the transport.
///
/// This is ordinary library code rather than a test helper, because it is the normative statement
/// of what the DirectShow filter must do: the C++ consumer is a transliteration of
/// [`RingConsumer::read`] and [`RingConsumer::liveness`], and a divergence between them is a bug
/// in the filter.
pub struct RingConsumer {
    region: RingRegion,
    /// Generation the cached geometry below belongs to.
    generation: u64,
    width: u32,
    height: u32,
    flags: u32,
    /// Sequence of the last frame accepted, so a repeat read reports [`ReadOutcome::NoFrame`]
    /// rather than handing the same picture over twice.
    last_accepted: u64,
}

impl RingConsumer {
    /// Validate the mapping and cache its geometry.
    ///
    /// # Errors
    ///
    /// Returns [`AttachError`], whose variants are deliberately distinguishable: a foreign
    /// mapping, an incompatible version, and a malformed header are three different things to
    /// tell the user, and none of them is a picture rendered wrongly.
    pub fn attach(region: RingRegion) -> Result<Self, AttachError> {
        if region.len() < RING_BYTES {
            return Err(AttachError::RegionTooSmall {
                len: region.len(),
                needed: RING_BYTES,
            });
        }

        let magic = region.read_u32(offset_of!(RingHeader, magic));
        if magic != MAGIC {
            return Err(AttachError::NotOurs { found: magic });
        }
        let version = region.read_u32(offset_of!(RingHeader, version));
        if version != TRANSPORT_VERSION {
            return Err(AttachError::Version {
                found: version,
                expected: TRANSPORT_VERSION,
            });
        }

        // Ours and our version, so the geometry constants must agree exactly. They are carried in
        // the header so a consumer never hard-codes them; disagreeing with them means one side is
        // not what it claims to be.
        let header_bytes = region.read_u32(offset_of!(RingHeader, header_bytes)) as usize;
        let slot_bytes = region.read_u32(offset_of!(RingHeader, slot_bytes)) as usize;
        let slot_count = region.read_u32(offset_of!(RingHeader, slot_count));
        let fourcc = region.read_u32(offset_of!(RingHeader, fourcc));
        if header_bytes != HEADER_BYTES || slot_bytes != SLOT_BYTES || slot_count != SLOT_COUNT {
            return Err(AttachError::Malformed(format!(
                "header says {header_bytes}-byte header and {slot_count} x {slot_bytes}-byte slots"
            )));
        }
        if fourcc != FOURCC_I420 {
            return Err(AttachError::Malformed(format!(
                "payload format {fourcc:#010x} is not I420"
            )));
        }

        let mut consumer = Self {
            region,
            generation: 0,
            width: 0,
            height: 0,
            flags: 0,
            last_accepted: 0,
        };
        consumer.sync();
        Ok(consumer)
    }

    /// Re-read the cached geometry and flags if the generation has moved.
    ///
    /// Acquire on the generation is what orders the plain reads that follow it against the
    /// producer's writes before the bump.
    fn sync(&mut self) -> bool {
        let generation = self
            .region
            .header_atomic(offset_of!(RingHeader, generation))
            .load(Ordering::Acquire);
        if generation == self.generation {
            return false;
        }
        self.generation = generation;
        self.width = self.region.read_u32(offset_of!(RingHeader, width));
        self.height = self.region.read_u32(offset_of!(RingHeader, height));
        self.flags = self.region.read_u32(offset_of!(RingHeader, flags));
        true
    }

    /// Copy the freshest unread frame into `out`.
    ///
    /// `out` is reused across calls so a consumer allocates once. It is left in an unspecified
    /// state for every outcome other than [`ReadOutcome::Frame`] — a discarded read is discarded,
    /// not repaired.
    pub fn read(&mut self, out: &mut Vec<u8>) -> ReadOutcome {
        self.read_inner(out, || {})
    }

    /// [`Self::read`], with a seam at the one instant that cannot be reached from outside.
    ///
    /// `during_copy` runs where a producer racing this reader would write: after the payload has
    /// been copied and before the version is re-read. It exists because the property the seqlock
    /// exists for — that a slot rewritten *during* a copy is rejected — cannot be produced
    /// deterministically from a single thread, and a property this important should not be tested
    /// only by a race that may or may not happen. In production the closure is empty and
    /// monomorphises away, so nothing of this reaches the compiled consumer path.
    fn read_inner(&mut self, out: &mut Vec<u8>, during_copy: impl FnOnce()) -> ReadOutcome {
        // A geometry change between two reads is observed here, so a frame is never reported at a
        // geometry it was not published in.
        self.sync();

        // Acquire: pairs with the producer's Release store, so the slot this sequence names is
        // already stable by the time it is visible here.
        let sequence = self
            .region
            .header_atomic(offset_of!(RingHeader, latest))
            .load(Ordering::Acquire);
        if sequence == 0 || sequence == self.last_accepted {
            return ReadOutcome::NoFrame;
        }

        let version = self.region.slot_sequence(sequence);
        let before = version.load(Ordering::Acquire);
        if before % 2 != 0 {
            return ReadOutcome::BeingWritten;
        }

        // Read inside the protected region, so it may be anything at all if the producer is
        // writing here right now. Clamped before it reaches a copy length, and the version
        // comparison below is what decides whether any of it counts.
        let bytes = (self
            .region
            .read_slot_u32(sequence, offset_of!(SlotHeader, bytes)) as usize)
            .min(MAX_PAYLOAD_BYTES);
        let timestamp_us = self.region.read_slot_timestamp(sequence);

        out.clear();
        out.reserve(bytes);
        // SAFETY: `bytes` is clamped to `MAX_PAYLOAD_BYTES`, which is the payload capacity of every
        // slot, and `out` has just been reserved for that many. The two regions cannot overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.region.slot_payload(sequence),
                out.as_mut_ptr(),
                bytes,
            );
            out.set_len(bytes);
        }

        during_copy();

        // Pairs with the producer's Release fence after it marked the slot odd: no byte of the copy
        // above may be reordered after this, or a torn read could pass the comparison below.
        fence(Ordering::Acquire);
        let after = version.load(Ordering::Relaxed);
        if before != after {
            return ReadOutcome::Torn;
        }

        self.last_accepted = sequence;
        ReadOutcome::Frame {
            sequence,
            timestamp_us,
            width: self.width,
            height: self.height,
        }
    }

    /// What the producer's state says should be on screen.
    ///
    /// A DirectShow source filter must keep handing buffers to its graph whatever the answer is —
    /// a filter that stops delivering hangs the application it lives in — so every state other
    /// than [`Liveness::Streaming`] means a frame the consumer chooses, never an absence of frames.
    pub fn liveness(&mut self, now_ms: u64) -> Liveness {
        self.sync();
        if self.flags & FLAG_RUNNING == 0 {
            return Liveness::Stopped;
        }
        let heartbeat = self
            .region
            .header_atomic(offset_of!(RingHeader, heartbeat_ms))
            .load(Ordering::Acquire);
        if now_ms.saturating_sub(heartbeat) > HEARTBEAT_STALE_MS {
            return Liveness::ProducerGone;
        }
        let latest = self
            .region
            .header_atomic(offset_of!(RingHeader, latest))
            .load(Ordering::Acquire);
        if latest > self.last_accepted {
            Liveness::Streaming
        } else {
            Liveness::Holding
        }
    }

    /// The geometry frames are currently being published at.
    #[must_use]
    pub const fn geometry(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The generation the cached geometry belongs to. Moves whenever geometry or the stream's
    /// lifetime changes.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }
}

/// What a consumer should be showing, derived from the producer's flags, heartbeat, and sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Running, alive, and new frames are arriving: deliver them.
    Streaming,
    /// Running and alive, but nothing new since the last frame accepted: hold that frame. This is
    /// the case a heartbeat exists to separate from [`Liveness::ProducerGone`].
    Holding,
    /// The producer stopped cleanly. Show a placeholder.
    Stopped,
    /// The heartbeat has not advanced for [`HEARTBEAT_STALE_MS`]: the desktop is gone without
    /// having stopped. Show a placeholder.
    ProducerGone,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ring backed by this process's own memory.
    ///
    /// Everything the protocol does is portable, so the whole of it is exercised without a section,
    /// a filter, or Windows — which is the point of keeping this module free of Win32.
    struct OwnedRing {
        _buf: Vec<u8>,
        region: RingRegion,
    }

    impl OwnedRing {
        fn new() -> Self {
            let mut buf = vec![0_u8; RING_BYTES];
            let base = buf.as_mut_ptr();
            // SAFETY: `buf` owns RING_BYTES bytes, is never reallocated, and outlives the region.
            let region = unsafe { RingRegion::new(base, RING_BYTES) };
            Self { _buf: buf, region }
        }
    }

    const VGA: VideoFormat = VideoFormat {
        width: 640,
        height: 480,
        max_fps: 30,
    };

    fn frame(format: VideoFormat, fill: u8) -> Vec<u8> {
        vec![fill; i420_bytes(format.width, format.height)]
    }

    #[test]
    fn the_allocation_should_hold_four_slots_of_the_maximum_geometry() {
        assert_eq!(SLOT_BYTES, 3_112_960);
        assert_eq!(RING_BYTES, 4096 + 4 * 3_112_960);
        assert!(SLOT_BYTES - size_of::<SlotHeader>() >= i420_bytes(MAX_WIDTH, MAX_HEIGHT));
    }

    #[test]
    fn a_published_frame_should_round_trip_byte_identically() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");

        let sent = frame(VGA, 0x5A);
        let sequence = producer.publish(&sent, 12_345, 0).expect("must publish");

        let mut received = Vec::new();
        assert_eq!(
            consumer.read(&mut received),
            ReadOutcome::Frame {
                sequence,
                timestamp_us: 12_345,
                width: 640,
                height: 480,
            }
        );
        assert_eq!(received, sent);
    }

    #[test]
    fn a_second_read_of_the_same_frame_should_report_nothing_new() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");
        producer
            .publish(&frame(VGA, 1), 0, 0)
            .expect("must publish");

        let mut buf = Vec::new();
        assert!(matches!(consumer.read(&mut buf), ReadOutcome::Frame { .. }));
        assert_eq!(consumer.read(&mut buf), ReadOutcome::NoFrame);
    }

    #[test]
    fn an_empty_ring_should_report_nothing_rather_than_a_zero_frame() {
        let ring = OwnedRing::new();
        let _producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");

        assert_eq!(consumer.read(&mut Vec::new()), ReadOutcome::NoFrame);
    }

    #[test]
    fn an_odd_sequence_should_be_refused_as_being_written() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");
        let sequence = producer
            .publish(&frame(VGA, 7), 0, 0)
            .expect("must publish");

        // Stand where a producer part-way through a write stands: the slot marked odd.
        ring.region
            .slot_sequence(sequence)
            .fetch_add(1, Ordering::Release);

        assert_eq!(consumer.read(&mut Vec::new()), ReadOutcome::BeingWritten);
    }

    #[test]
    fn a_slot_rewritten_during_the_copy_should_be_discarded() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");
        let sequence = producer
            .publish(&frame(VGA, 3), 0, 0)
            .expect("must publish");

        // A complete write of the same slot landing while the reader is copying it — exactly as
        // being lapped looks. The version moves by two, so it is even again and parity alone
        // cannot tell; only the comparison against the snapshot taken before the copy can.
        let outcome = consumer.read_inner(&mut Vec::new(), || {
            ring.region
                .slot_sequence(sequence)
                .fetch_add(2, Ordering::Release);
        });

        assert_eq!(outcome, ReadOutcome::Torn);
    }

    #[test]
    fn being_lapped_should_cost_one_frame_and_not_the_stream() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");

        let sequence = producer
            .publish(&frame(VGA, 1), 0, 0)
            .expect("must publish");
        let lapped = consumer.read_inner(&mut Vec::new(), || {
            ring.region
                .slot_sequence(sequence)
                .fetch_add(2, Ordering::Release);
        });
        assert_eq!(lapped, ReadOutcome::Torn);

        // The very next frame is delivered: a torn read is one frame lost, not a stream lost.
        let sent = frame(VGA, 2);
        producer.publish(&sent, 99, 0).expect("must publish");
        let mut received = Vec::new();
        assert!(matches!(
            consumer.read(&mut received),
            ReadOutcome::Frame {
                timestamp_us: 99,
                ..
            }
        ));
        assert_eq!(received, sent);
    }

    #[test]
    fn the_ring_should_wrap_and_keep_delivering() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");

        let mut received = Vec::new();
        for n in 0..(u64::from(SLOT_COUNT) * 3) {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "n is small by construction"
            )]
            let sent = frame(VGA, n as u8);
            producer.publish(&sent, n, 0).expect("must publish");
            assert!(matches!(
                consumer.read(&mut received),
                ReadOutcome::Frame { .. }
            ));
            assert_eq!(received, sent, "frame {n} must survive the wrap");
        }
        assert_eq!(producer.frames_published(), u64::from(SLOT_COUNT) * 3);
    }

    #[test]
    fn a_geometry_change_should_move_the_generation_and_the_reported_size() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");
        let before = consumer.generation();
        assert_eq!(consumer.geometry(), (640, 480));

        producer
            .set_geometry(VideoFormat::CAMERA_720P)
            .expect("720p must fit");
        producer
            .publish(&frame(VideoFormat::CAMERA_720P, 9), 0, 0)
            .expect("must publish");

        let mut received = Vec::new();
        assert_eq!(
            consumer.read(&mut received),
            ReadOutcome::Frame {
                sequence: 1,
                timestamp_us: 0,
                width: 1280,
                height: 720,
            }
        );
        assert!(consumer.generation() > before);
        assert_eq!(received.len(), i420_bytes(1280, 720));
    }

    #[test]
    fn a_frame_at_the_wrong_size_should_be_refused_rather_than_published() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");

        let err = producer
            .publish(&frame(VideoFormat::CAMERA_720P, 0), 0, 0)
            .expect_err("must refuse");
        assert!(matches!(err, TransportError::FrameSize { .. }));
        assert_eq!(producer.frames_published(), 0);
    }

    #[test]
    fn geometry_beyond_the_allocation_should_be_refused() {
        let ring = OwnedRing::new();
        let too_big = VideoFormat {
            width: 3840,
            height: 2160,
            max_fps: 30,
        };
        assert!(matches!(
            RingProducer::create(ring.region, too_big, 0),
            Err(TransportError::GeometryTooLarge { .. })
        ));

        let odd = VideoFormat {
            width: 641,
            height: 480,
            max_fps: 30,
        };
        assert!(matches!(
            RingProducer::create(ring.region, odd, 0),
            Err(TransportError::OddGeometry { .. })
        ));
    }

    #[test]
    fn a_foreign_mapping_should_be_refused_as_not_ours() {
        let ring = OwnedRing::new();
        // Untouched memory: no producer has ever initialised it.
        let err = RingConsumer::attach(ring.region)
            .err()
            .expect("an uninitialised region must be refused");
        assert_eq!(err, AttachError::NotOurs { found: 0 });
    }

    #[test]
    fn a_mismatched_version_should_be_refused_rather_than_read() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        producer
            .publish(&frame(VGA, 1), 0, 0)
            .expect("must publish");

        // Stand where a filter built against a later transport version stands.
        ring.region
            .write_u32(offset_of!(RingHeader, version), TRANSPORT_VERSION + 1);

        let err = RingConsumer::attach(ring.region)
            .err()
            .expect("a version disagreement must be refused");
        assert_eq!(
            err,
            AttachError::Version {
                found: TRANSPORT_VERSION + 1,
                expected: TRANSPORT_VERSION,
            }
        );
    }

    #[test]
    fn an_inconsistent_header_should_be_refused_as_malformed() {
        let ring = OwnedRing::new();
        let _producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        ring.region
            .write_u32(offset_of!(RingHeader, slot_count), SLOT_COUNT + 1);

        assert!(matches!(
            RingConsumer::attach(ring.region),
            Err(AttachError::Malformed(_))
        ));
    }

    #[test]
    fn re_initialising_a_held_section_should_move_the_generation() {
        let ring = OwnedRing::new();
        let first = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let consumer = RingConsumer::attach(ring.region).expect("must attach");
        let before = consumer.generation();
        drop(first);

        // What a restart against a section a surviving consumer still holds looks like.
        let _second = RingProducer::create(ring.region, VGA, 0).expect("must re-initialise");
        let mut held = consumer;
        assert_eq!(held.liveness(0), Liveness::Holding);
        assert!(
            held.generation() > before,
            "a reclaimed section must invalidate a consumer's cached view"
        );
    }

    #[test]
    fn a_stale_odd_slot_left_by_a_dead_producer_should_not_stall_the_next_one() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 0).expect("must initialise");
        let sequence = producer
            .publish(&frame(VGA, 1), 0, 0)
            .expect("must publish");
        // A producer killed part-way through a write leaves the slot odd forever.
        ring.region
            .slot_sequence(sequence)
            .fetch_add(1, Ordering::Release);
        drop(producer);

        let mut restarted = RingProducer::create(ring.region, VGA, 0).expect("must re-initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");
        let sent = frame(VGA, 2);
        restarted.publish(&sent, 0, 0).expect("must publish");

        let mut received = Vec::new();
        assert!(matches!(
            consumer.read(&mut received),
            ReadOutcome::Frame { .. }
        ));
        assert_eq!(received, sent);
    }

    #[test]
    fn liveness_should_separate_streaming_idle_stopped_and_gone() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 1_000).expect("must initialise");
        let mut consumer = RingConsumer::attach(ring.region).expect("must attach");

        // Running and idle: nothing published yet, but the producer is alive.
        assert_eq!(consumer.liveness(1_000), Liveness::Holding);

        // Running with a frame waiting.
        producer
            .publish(&frame(VGA, 1), 0, 1_100)
            .expect("must publish");
        assert_eq!(consumer.liveness(1_100), Liveness::Streaming);

        // Once read, the same state reads as holding the last frame rather than as a stall.
        assert!(matches!(
            consumer.read(&mut Vec::new()),
            ReadOutcome::Frame { .. }
        ));
        assert_eq!(consumer.liveness(1_100), Liveness::Holding);

        // Idle but ticking: still alive well past the interval, because the heartbeat advanced.
        producer.tick(2_000);
        assert_eq!(consumer.liveness(2_000), Liveness::Holding);

        // The heartbeat stops advancing and the producer never said stop: gone.
        assert_eq!(
            consumer.liveness(2_000 + HEARTBEAT_STALE_MS + 1),
            Liveness::ProducerGone
        );

        // A clean stop is observed immediately, without waiting the staleness out.
        producer.stop();
        assert_eq!(consumer.liveness(2_001), Liveness::Stopped);
    }

    #[test]
    fn the_idle_heartbeat_should_not_store_more_often_than_the_interval() {
        let ring = OwnedRing::new();
        let mut producer = RingProducer::create(ring.region, VGA, 1_000).expect("must initialise");
        let heartbeat = || {
            ring.region
                .header_atomic(offset_of!(RingHeader, heartbeat_ms))
                .load(Ordering::Acquire)
        };

        producer.tick(1_000 + HEARTBEAT_INTERVAL_MS - 1);
        assert_eq!(heartbeat(), 1_000, "too soon to be worth a store");
        producer.tick(1_000 + HEARTBEAT_INTERVAL_MS);
        assert_eq!(heartbeat(), 1_000 + HEARTBEAT_INTERVAL_MS);
    }

    #[test]
    fn a_reader_racing_a_writer_should_only_ever_accept_frames_that_were_published() {
        // The failure this guards against is a *wrong* frame, not a missing one: a torn read is a
        // frame that is part one picture and part another, and no parity check catches it if the
        // orderings in the sequence protocol are wrong. Every frame the consumer accepts here is
        // therefore checked byte for byte against the pattern the producer wrote.
        let ring = OwnedRing::new();
        let region = ring.region;

        // Small geometry: the point is to make the producer lap the reader constantly, and a
        // 720p copy is slow enough that it would barely wrap in the time this test may take.
        let format = VideoFormat {
            width: 64,
            height: 48,
            max_fps: 30,
        };
        let bytes = i420_bytes(format.width, format.height);
        let frames = 20_000_u64;

        // The reader is expected to lose frames — that is what being lapped means — so it cannot
        // stop on having seen the last one. It stops when the producer says it has finished and
        // there is nothing left unread.
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader_finished = std::sync::Arc::clone(&finished);

        let reader = std::thread::spawn(move || {
            let mut consumer = loop {
                match RingConsumer::attach(region) {
                    Ok(consumer) => break consumer,
                    Err(AttachError::NotOurs { .. }) => std::thread::yield_now(),
                    Err(e) => panic!("consumer must attach: {e}"),
                }
            };
            let mut accepted = 0_u64;
            let mut rejected = 0_u64;
            let mut buf = Vec::new();
            let mut last = 0_u64;
            loop {
                match consumer.read(&mut buf) {
                    ReadOutcome::Frame { sequence, .. } => {
                        assert_eq!(buf.len(), bytes, "frame {sequence} is the wrong length");
                        // Every byte of frame n is n: any splice of two frames shows up here.
                        #[allow(
                            clippy::cast_possible_truncation,
                            reason = "the pattern is mod 256"
                        )]
                        let want = sequence as u8;
                        assert!(
                            buf.iter().all(|&b| b == want),
                            "frame {sequence} was spliced with another frame"
                        );
                        assert!(sequence > last, "sequences must not go backwards");
                        last = sequence;
                        accepted += 1;
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
            (accepted, rejected)
        });

        let mut producer = RingProducer::create(region, format, 0).expect("must initialise");
        for n in 1..=frames {
            #[allow(clippy::cast_possible_truncation, reason = "the pattern is mod 256")]
            let payload = vec![n as u8; bytes];
            producer.publish(&payload, n, n).expect("must publish");
        }
        finished.store(true, Ordering::Release);

        let (accepted, _rejected) = reader.join().expect("reader must not fail");
        assert!(
            accepted > 0,
            "the reader must have seen frames, or the test proves nothing"
        );
    }
}
