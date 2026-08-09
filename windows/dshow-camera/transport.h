//------------------------------------------------------------------------------
// transport.h — the consuming half of the camera frame transport, in C++.
//
// This is a transliteration of `unifiedstream-video/src/transport.rs`, which is the normative
// statement of the protocol and says so in its own module documentation. Every constant, every
// field offset, and every memory ordering below has a counterpart there, named in a comment.
//
// Neither compiler sees both halves. The Rust build asserts the layout with
// `const _: () = assert!(...)`; this file asserts the same numbers with `static_assert`, so a field
// reordered on either side fails a build rather than producing wrong pixels. Layout is only half
// the contract, though: the memory orderings have no layout and therefore no static assertion, and
// a wrong one compiles cleanly and tears rarely. That half is covered by `ring_conform.exe`, which
// runs this exact translation unit against the real Rust producer in CI.
//
// Two constraints shape the code.
//
// **The mapping is read-only.** The section grants consumers `GENERIC_READ` and carries a Low
// mandatory label with no-write-up, so a store of any kind — including the locked read-modify-write
// a non-lock-free atomic would use — faults. Everything here loads and nothing stores, and the
// `is_always_lock_free` assertion below is what keeps a target where that stops being true from
// building at all.
//
// **The same offsets must hold in the x86 and the x64 build.** One 64-bit producer serves consumers
// of both architectures, so there is no pointer, no `size_t`, and no field whose size or alignment
// varies with the target in either structure.
//------------------------------------------------------------------------------

#ifndef UNIFIEDSTREAM_DSHOW_TRANSPORT_H
#define UNIFIEDSTREAM_DSHOW_TRANSPORT_H

#include <atomic>
#include <cstddef>
#include <cstdint>

namespace unifiedstream {

// ---------------------------------------------------------------------------
// Constants. Each one mirrors the Rust item named beside it; the literals are written out rather
// than derived, so a divergence is visible in a diff of this file against that one.
// ---------------------------------------------------------------------------

/// `transport::MAGIC` — `u32::from_le_bytes(*b"USVC")`.
inline constexpr std::uint32_t kMagic = 0x43565355u;

/// `transport::TRANSPORT_VERSION`.
inline constexpr std::uint32_t kTransportVersion = 1u;

/// `transport::FOURCC_I420` — `u32::from_le_bytes(*b"I420")`.
inline constexpr std::uint32_t kFourccI420 = 0x30323449u;

/// `transport::SLOT_COUNT`.
inline constexpr std::uint32_t kSlotCount = 4u;

/// `transport::HEADER_BYTES` — the header, padded to a page.
inline constexpr std::size_t kHeaderBytes = 4096u;

/// `transport::SLOT_BYTES` — one slot header plus a maximum-geometry payload, rounded to a page.
inline constexpr std::size_t kSlotBytes = 3112960u;

/// `transport::MAX_WIDTH`.
inline constexpr std::uint32_t kMaxWidth = 1920u;

/// `transport::MAX_HEIGHT`.
inline constexpr std::uint32_t kMaxHeight = 1080u;

/// Bytes one I420 frame occupies at the maximum geometry: `transport::MAX_PAYLOAD_BYTES`.
inline constexpr std::size_t kMaxPayloadBytes =
    static_cast<std::size_t>(kMaxWidth) * kMaxHeight * 3u / 2u;

/// `transport::RING_BYTES` — the whole mapping, about 11.9 MiB.
inline constexpr std::size_t kRingBytes = kHeaderBytes + kSlotCount * kSlotBytes;

/// `transport::FLAG_RUNNING` — `RingHeader::flags` bit 0.
inline constexpr std::uint32_t kFlagRunning = 1u << 0;

/// `transport::HEARTBEAT_INTERVAL_MS` — how often the producer ticks while running but idle.
inline constexpr std::uint64_t kHeartbeatIntervalMs = 250u;

/// `transport::HEARTBEAT_STALE_MS` — how far behind the heartbeat may fall before "gone".
inline constexpr std::uint64_t kHeartbeatStaleMs = 2000u;

/// `platform::windows::SECTION_NAME` — session-local, and deliberately carrying no version, so a
/// mismatched filter reports a disagreement rather than failing to find the mapping at all.
inline constexpr const wchar_t* kSectionName = L"Local\\UnifiedStream.Camera";

/// `transport::FILTER_CLSID`, as text. The binary GUID lives in `filter.h`.
inline constexpr const wchar_t* kFilterClsidText = L"{6D8DD393-D871-4498-A24F-4AFFEACFC106}";

/// `transport::FILTER_VERSION_VALUE` — the registry value the desktop's probe reads.
inline constexpr const wchar_t* kFilterVersionValue = L"TransportVersion";

/// `transport::FILTER_FRIENDLY_NAME`, which is `CAMERA_NODE_LABEL`: the camera has one name across
/// the product.
inline constexpr const wchar_t* kFilterFriendlyName = L"UnifiedStream Camera";

// ---------------------------------------------------------------------------
// Layout. `RingHeader` and `SlotHeader` describe bytes another process writes; nothing here is ever
// instantiated, and the definitions exist so `offsetof` can be asserted against the Rust numbers.
//
// The atomic fields are declared as plain `std::uint64_t` rather than as `std::atomic<...>`. That
// is deliberate: it keeps the layout obviously identical on both architectures — MSVC's
// `std::atomic<std::uint64_t>` alignment on x86 is a detail this contract should not depend on —
// and every access to them goes through the acquire loads below instead.
// ---------------------------------------------------------------------------

/// `transport::RingHeader`.
struct RingHeader {
    std::uint32_t magic;          // kMagic; written last during init, cleared first
    std::uint32_t version;        // kTransportVersion
    std::uint32_t header_bytes;   // kHeaderBytes
    std::uint32_t slot_count;     // kSlotCount
    std::uint32_t slot_bytes;     // kSlotBytes
    std::uint32_t fourcc;         // kFourccI420
    std::uint32_t width;          // geometry of the current stream, not of the allocation
    std::uint32_t height;         //
    std::uint64_t generation;     // atomic; bumped on a geometry change, a start, or a stop
    std::uint64_t latest;         // atomic; sequence of the freshest published frame, 0 if none
    std::uint64_t heartbeat_ms;   // atomic; the producer's monotonic tick
    std::uint32_t producer_pid;   // diagnosis only; nothing in the protocol reads it
    std::uint32_t flags;          // kFlagRunning in bit 0
};

/// `transport::SlotHeader`, immediately followed by that slot's payload.
struct SlotHeader {
    std::uint64_t sequence;      // atomic; seqlock version, odd while the producer is writing
    std::uint64_t timestamp_us;  // capture time, carried through from the media header
    std::uint32_t bytes;         // payload bytes written; inside the protected region, so untrusted
    std::uint32_t reserved[3];   // zeroed
};

// The same numbers as the `const _: () = assert!` block in `transport.rs`. A field reordered on
// either side must fail a build.
static_assert(sizeof(RingHeader) == 64, "RingHeader must be 64 bytes, as transport.rs asserts");
static_assert(alignof(RingHeader) == 8, "RingHeader must be 8-byte aligned on both architectures");
static_assert(offsetof(RingHeader, magic) == 0);
static_assert(offsetof(RingHeader, version) == 4);
static_assert(offsetof(RingHeader, header_bytes) == 8);
static_assert(offsetof(RingHeader, slot_count) == 12);
static_assert(offsetof(RingHeader, slot_bytes) == 16);
static_assert(offsetof(RingHeader, fourcc) == 20);
static_assert(offsetof(RingHeader, width) == 24);
static_assert(offsetof(RingHeader, height) == 28);
static_assert(offsetof(RingHeader, generation) == 32);
static_assert(offsetof(RingHeader, latest) == 40);
static_assert(offsetof(RingHeader, heartbeat_ms) == 48);
static_assert(offsetof(RingHeader, producer_pid) == 56);
static_assert(offsetof(RingHeader, flags) == 60);

static_assert(sizeof(SlotHeader) == 32, "SlotHeader must be 32 bytes, as transport.rs asserts");
static_assert(alignof(SlotHeader) == 8, "SlotHeader must be 8-byte aligned on both architectures");
static_assert(offsetof(SlotHeader, sequence) == 0);
static_assert(offsetof(SlotHeader, timestamp_us) == 8);
static_assert(offsetof(SlotHeader, bytes) == 16);
static_assert(offsetof(SlotHeader, reserved) == 20);

// The derived sizes, asserted for the same reason the Rust asserts them.
static_assert(sizeof(RingHeader) <= kHeaderBytes);
static_assert(kSlotBytes >= sizeof(SlotHeader) + kMaxPayloadBytes);
static_assert(kSlotBytes % 4096 == 0);
static_assert(kMaxPayloadBytes == 3110400u);
static_assert(kRingBytes == 12455936u);

// The mapping is read-only. A `std::atomic_ref<std::uint64_t>` that is not lock-free would fall
// back to a locked read-modify-write or an external lock, and the first of those faults on a
// read-only page — a crash inside whichever application loaded this filter. Fail the build instead.
static_assert(std::atomic_ref<std::uint64_t>::is_always_lock_free,
              "64-bit atomic loads must be lock-free: the shared mapping is read-only");
static_assert(std::atomic_ref<std::uint64_t>::required_alignment <= 8,
              "every atomic field in the layout is 8-byte aligned and no more");

// ---------------------------------------------------------------------------
// Outcomes. Each mirrors a Rust enum, and the variants are kept distinguishable for the reason the
// `virtual-camera-component` spec gives: they are different messages, not one failure.
// ---------------------------------------------------------------------------

/// `transport::AttachError`, plus the success case.
enum class AttachStatus {
    /// Attached; the geometry is cached.
    Ok,
    /// The region handed over is smaller than `kRingBytes`.
    RegionTooSmall,
    /// The magic is not `kMagic`. **Not an error to report to the user**: the producer clears the
    /// magic first during initialisation and writes it last, so a filter attaching mid-init sees
    /// exactly this. Retry.
    NotOurs,
    /// Ours, and a layout this build does not understand. Terminal for this attachment — retrying
    /// can only fail identically, and the desktop reports the disagreement from the registry before
    /// a stream ever starts.
    Version,
    /// Ours, our version, and internally inconsistent. A bug, not a version skew.
    Malformed,
};

/// `transport::ReadOutcome`.
enum class ReadOutcome {
    /// A complete frame, copied into the caller's buffer.
    Frame,
    /// Nothing published, or nothing new since the last frame accepted.
    NoFrame,
    /// The producer holds the slot right now. Costs this attempt, not the stream.
    BeingWritten,
    /// The producer wrote the slot during the copy. Discarded unread rather than delivered as a
    /// picture assembled from two frames.
    Torn,
};

/// `transport::Liveness`.
enum class Liveness {
    /// Running, alive, new frames arriving.
    Streaming,
    /// Running and alive, nothing new: hold the last frame. The case the heartbeat exists to
    /// separate from `ProducerGone`.
    Holding,
    /// The producer stopped cleanly. Observed immediately, without waiting out the staleness bound.
    Stopped,
    /// The heartbeat has not advanced for `kHeartbeatStaleMs`: the desktop is gone.
    ProducerGone,
};

/// What a successful read produced. `transport::ReadOutcome::Frame`'s payload.
struct FrameInfo {
    std::uint64_t sequence;
    std::uint64_t timestamp_us;
    /// Geometry the payload is in: the geometry current when it was published, not the geometry the
    /// graph connected at.
    std::uint32_t width;
    std::uint32_t height;
    /// Payload bytes copied out, already clamped to the slot's capacity.
    std::uint32_t bytes;
};

// ---------------------------------------------------------------------------
// The region and the consumer.
// ---------------------------------------------------------------------------

/// A mapped region of at least `kRingBytes` read-only bytes, shared with the producing desktop.
///
/// `transport::RingRegion`. A base and a length rather than anything typed, because the bytes are
/// concurrently written by another process and no reference type describes that.
class RingRegion {
public:
    RingRegion() noexcept = default;
    RingRegion(const std::uint8_t* base, std::size_t len) noexcept : base_(base), len_(len) {}

    [[nodiscard]] const std::uint8_t* base() const noexcept { return base_; }
    [[nodiscard]] std::size_t len() const noexcept { return len_; }
    [[nodiscard]] bool valid() const noexcept { return base_ != nullptr && len_ >= kRingBytes; }

private:
    const std::uint8_t* base_ = nullptr;
    std::size_t len_ = 0;
};

/// A consuming application's half of the transport.
///
/// `transport::RingConsumer`, and a divergence from it is a bug in this filter rather than a
/// difference of opinion.
class RingConsumer {
public:
    RingConsumer() noexcept = default;

    /// Validate the mapping and cache its geometry. `transport::RingConsumer::attach`.
    ///
    /// On anything other than `AttachStatus::Ok` the consumer is left detached and `found` carries
    /// the value that disagreed — the magic for `NotOurs`, the version for `Version` — so a caller
    /// can log what it saw.
    AttachStatus Attach(RingRegion region, std::uint32_t* found) noexcept;

    /// Whether `Attach` has succeeded and no terminal failure has detached this consumer since.
    [[nodiscard]] bool attached() const noexcept { return region_.valid(); }

    /// Forget the mapping. Used where a version disagreement makes the attachment terminal.
    void Detach() noexcept { *this = RingConsumer(); }

    /// Copy the freshest unread frame into `out`. `transport::RingConsumer::read`.
    ///
    /// `out` is the caller's buffer, reused across calls: nothing on this path allocates, because
    /// it runs on the streaming thread of an application this code is a guest in. It is left in an
    /// unspecified state for every outcome other than `ReadOutcome::Frame`.
    ReadOutcome Read(std::uint8_t* out, std::size_t out_capacity, FrameInfo* info) noexcept;

    /// What the producer's state says should be on screen. `transport::RingConsumer::liveness`.
    Liveness GetLiveness(std::uint64_t now_ms) noexcept;

    /// The geometry frames are currently being published at.
    [[nodiscard]] std::uint32_t width() const noexcept { return width_; }
    [[nodiscard]] std::uint32_t height() const noexcept { return height_; }

    /// The generation the cached geometry belongs to. Moves whenever the geometry changes or a
    /// stream starts or stops.
    [[nodiscard]] std::uint64_t generation() const noexcept { return generation_; }

private:
    /// Re-read the cached geometry and flags if the generation has moved.
    /// `transport::RingConsumer::sync`. Returns whether anything changed.
    bool Sync() noexcept;

    RingRegion region_;
    std::uint64_t generation_ = 0;
    std::uint32_t width_ = 0;
    std::uint32_t height_ = 0;
    std::uint32_t flags_ = 0;
    /// Sequence of the last frame accepted, so a repeat read reports `NoFrame` rather than handing
    /// the same picture over twice.
    std::uint64_t last_accepted_ = 0;
};

/// Bytes an I420 frame occupies at `width` x `height`. `transport::i420_bytes`.
constexpr std::size_t I420Bytes(std::uint32_t width, std::uint32_t height) noexcept {
    const std::size_t pixels = static_cast<std::size_t>(width) * height;
    return pixels + pixels / 2;
}

}  // namespace unifiedstream

#endif  // UNIFIEDSTREAM_DSHOW_TRANSPORT_H
