//------------------------------------------------------------------------------
// transport.cpp — the consumer half of the frame transport.
//
// A transliteration of `RingConsumer::attach`, `sync`, `read`, and `liveness` in
// `unifiedstream-video/src/transport.rs`. Read the two side by side; they are meant to be
// comparable line for line, and the memory orderings are the part that matters most, because they
// are the part no `static_assert` in `transport.h` can reach.
//
// **This translation unit is linked into both the filter and `ring_conform.exe`.** The conformance
// tool proves *this* code against the real Rust producer, not a copy of it — a conformance test
// against a duplicate proves the duplicate conforms.
//------------------------------------------------------------------------------

#include "transport.h"

#include <algorithm>
#include <cstring>

namespace unifiedstream {
namespace {

/// Read one `u32` field. Volatile because another process writes these, and a plain read would let
/// the compiler hoist or reuse a value that has since changed. `RingRegion::read_u32` in Rust.
std::uint32_t ReadU32(const std::uint8_t* at) noexcept {
    return *reinterpret_cast<const volatile std::uint32_t*>(at);
}

/// Read one non-atomic `u64` field — the slot timestamp, which lives inside the seqlock-protected
/// region and is validated by the sequence comparison rather than by an ordering of its own.
std::uint64_t ReadU64(const std::uint8_t* at) noexcept {
    return *reinterpret_cast<const volatile std::uint64_t*>(at);
}

/// One of the protocol's atomic `u64` fields, loaded with the given ordering.
///
/// The `const_cast` is safe and is the only way to name the field: `std::atomic_ref` binds to a
/// non-const lvalue, and the mapping is read-only. Nothing here ever stores, and the
/// `is_always_lock_free` assertion in `transport.h` is what guarantees the load stays a plain
/// aligned read rather than becoming a locked read-modify-write against a read-only page.
std::uint64_t LoadU64(const std::uint8_t* at, std::memory_order order) noexcept {
    auto* field = reinterpret_cast<std::uint64_t*>(const_cast<std::uint8_t*>(at));
    return std::atomic_ref<std::uint64_t>(*field).load(order);
}

/// Base of the slot a sequence number lands in. `RingRegion::slot`.
const std::uint8_t* SlotAt(const RingRegion& region, std::uint64_t sequence) noexcept {
    const std::size_t index = static_cast<std::size_t>(sequence % kSlotCount);
    return region.base() + kHeaderBytes + index * kSlotBytes;
}

}  // namespace

AttachStatus RingConsumer::Attach(RingRegion region, std::uint32_t* found) noexcept {
    if (found != nullptr) {
        *found = 0;
    }
    if (region.base() == nullptr || region.len() < kRingBytes) {
        return AttachStatus::RegionTooSmall;
    }

    const std::uint8_t* base = region.base();

    // The magic is written last during initialisation and cleared first, so this is also what a
    // filter attaching mid-initialisation sees. The caller treats it as a retry, not an error.
    const std::uint32_t magic = ReadU32(base + offsetof(RingHeader, magic));
    if (magic != kMagic) {
        if (found != nullptr) {
            *found = magic;
        }
        return AttachStatus::NotOurs;
    }

    const std::uint32_t version = ReadU32(base + offsetof(RingHeader, version));
    if (version != kTransportVersion) {
        if (found != nullptr) {
            *found = version;
        }
        return AttachStatus::Version;
    }

    // Ours and our version, so the geometry constants must agree exactly. They are carried in the
    // header so a consumer never hard-codes them; disagreeing with them means one side is not what
    // it claims to be.
    const std::uint32_t header_bytes = ReadU32(base + offsetof(RingHeader, header_bytes));
    const std::uint32_t slot_bytes = ReadU32(base + offsetof(RingHeader, slot_bytes));
    const std::uint32_t slot_count = ReadU32(base + offsetof(RingHeader, slot_count));
    const std::uint32_t fourcc = ReadU32(base + offsetof(RingHeader, fourcc));
    if (header_bytes != kHeaderBytes || slot_bytes != kSlotBytes || slot_count != kSlotCount) {
        return AttachStatus::Malformed;
    }
    if (fourcc != kFourccI420) {
        if (found != nullptr) {
            *found = fourcc;
        }
        return AttachStatus::Malformed;
    }

    *this = RingConsumer();
    region_ = region;
    Sync();
    return AttachStatus::Ok;
}

bool RingConsumer::Sync() noexcept {
    // Acquire, pairing with the producer's Release store of the generation in `RingProducer::create`
    // and its Release `fetch_add` in `set_geometry` and `stop`. It is what orders the plain reads
    // below against the producer's writes made before the bump.
    const std::uint64_t generation =
        LoadU64(region_.base() + offsetof(RingHeader, generation), std::memory_order_acquire);
    if (generation == generation_) {
        return false;
    }
    generation_ = generation;
    width_ = ReadU32(region_.base() + offsetof(RingHeader, width));
    height_ = ReadU32(region_.base() + offsetof(RingHeader, height));
    flags_ = ReadU32(region_.base() + offsetof(RingHeader, flags));
    return true;
}

ReadOutcome RingConsumer::Read(std::uint8_t* out, std::size_t out_capacity,
                               FrameInfo* info) noexcept {
    if (!attached()) {
        return ReadOutcome::NoFrame;
    }

    // A geometry change between two reads is observed here, so a frame is never reported at a
    // geometry it was not published in — and a producer that restarted against a section this
    // filter still holds alive bumps the same generation, so both cases land in one place.
    Sync();

    // Acquire, pairing with the producer's Release store of `latest` after the slot is stable, so
    // the slot this sequence names is already finished by the time the sequence is visible here.
    const std::uint64_t sequence =
        LoadU64(region_.base() + offsetof(RingHeader, latest), std::memory_order_acquire);
    if (sequence == 0 || sequence == last_accepted_) {
        return ReadOutcome::NoFrame;
    }

    const std::uint8_t* slot = SlotAt(region_, sequence);

    // Acquire, pairing with the producer's Release store of the even version at the end of
    // `publish`: everything it wrote into the slot is visible to a reader that sees this value.
    const std::uint64_t before =
        LoadU64(slot + offsetof(SlotHeader, sequence), std::memory_order_acquire);
    if (before % 2 != 0) {
        // Odd: the producer holds the slot right now. Costs this attempt, not the stream — and
        // there is no waiting path, because waiting here would be waiting inside someone else's
        // streaming thread.
        return ReadOutcome::BeingWritten;
    }

    // `bytes` is read from inside the seqlock-protected region, so it may be anything at all if the
    // producer is writing here right now. The version comparison below decides whether the copy
    // counts; this clamp is what keeps an in-flight value from being a buffer overrun first, and it
    // has to happen before the value reaches a copy length rather than after.
    const std::size_t claimed = ReadU32(slot + offsetof(SlotHeader, bytes));
    const std::size_t bytes = (std::min)((std::min)(claimed, kMaxPayloadBytes), out_capacity);
    const std::uint64_t timestamp_us = ReadU64(slot + offsetof(SlotHeader, timestamp_us));

    std::memcpy(out, slot + sizeof(SlotHeader), bytes);

    // Acquire fence, pairing with the producer's Release fence after it marked the slot odd at the
    // start of `publish`. Without it the copy above could be reordered after the load below, and a
    // torn read would pass the comparison — the single ordering in this file that no layout
    // assertion and no single-threaded test can catch.
    std::atomic_thread_fence(std::memory_order_acquire);
    const std::uint64_t after =
        LoadU64(slot + offsetof(SlotHeader, sequence), std::memory_order_relaxed);
    if (before != after) {
        // The producer wrote this slot during the copy. Parity alone would not catch it: a complete
        // rewrite moves the version by two and leaves it even, which is exactly what being lapped
        // looks like. Discard the bytes unread rather than deliver a picture assembled from two
        // frames.
        return ReadOutcome::Torn;
    }

    last_accepted_ = sequence;
    if (info != nullptr) {
        info->sequence = sequence;
        info->timestamp_us = timestamp_us;
        info->width = width_;
        info->height = height_;
        info->bytes = static_cast<std::uint32_t>(bytes);
    }
    return ReadOutcome::Frame;
}

Liveness RingConsumer::GetLiveness(std::uint64_t now_ms) noexcept {
    if (!attached()) {
        return Liveness::ProducerGone;
    }
    Sync();

    // A clean stop clears the flag and bumps the generation, so it is observed here immediately
    // rather than after the staleness bound expires.
    if ((flags_ & kFlagRunning) == 0) {
        return Liveness::Stopped;
    }

    // Acquire, pairing with the producer's Release store in `publish` and `tick`.
    const std::uint64_t heartbeat =
        LoadU64(region_.base() + offsetof(RingHeader, heartbeat_ms), std::memory_order_acquire);
    // Saturating, as the Rust is: the tick count is another process's clock and may read as ahead
    // of ours by a scheduler quantum, which must not underflow into "gone".
    const std::uint64_t behind = now_ms > heartbeat ? now_ms - heartbeat : 0;
    if (behind > kHeartbeatStaleMs) {
        return Liveness::ProducerGone;
    }

    const std::uint64_t latest =
        LoadU64(region_.base() + offsetof(RingHeader, latest), std::memory_order_acquire);
    return latest > last_accepted_ ? Liveness::Streaming : Liveness::Holding;
}

}  // namespace unifiedstream
