//------------------------------------------------------------------------------
// ring_conform.cpp — the cross-toolchain conformance test.
//
// The C++ consumer against the real Rust producer, over the real named section, verifying every
// accepted frame byte for byte against the pattern the producer published.
//
// **This is the single highest-value test in the change**, and the reason it exists is worth
// stating plainly. `transport.h`'s `static_assert`s catch layout drift and nothing else. Memory
// ordering has no layout: a consumer that reads `latest` relaxed, or skips the acquire fence
// between the copy and the second sequence read, compiles cleanly, passes every one of those
// assertions, and produces a torn frame perhaps once an hour on one machine in ten. Nothing in
// either build observes both halves at once except this, and a single-threaded unit test cannot
// tear — the race has to be real.
//
// It links `transport.cpp` itself, not a copy of it. A conformance test against a duplicate proves
// the duplicate conforms.
//
// Run alongside `cargo run -p unifiedstream-video --example ring_smoke -- --publish-only`. Exits
// non-zero when any frame was accepted with wrong contents, and when no frame was accepted at all —
// a test that accepted nothing proves nothing.
//------------------------------------------------------------------------------

#include <windows.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>

#include "section.h"
#include "transport.h"

using unifiedstream::AttachStatus;
using unifiedstream::CameraSection;
using unifiedstream::FrameInfo;
using unifiedstream::I420Bytes;
using unifiedstream::Liveness;
using unifiedstream::ReadOutcome;
using unifiedstream::RingConsumer;
using unifiedstream::TickMs;

namespace {

/// How long to wait for the producer to create the section before giving up.
constexpr std::uint64_t kDefaultWaitMs = 120'000;

/// How long to keep reading after the producer stopped or vanished, so nothing already published is
/// left unread.
constexpr std::uint64_t kDrainMs = 250;

/// The pattern the publisher writes: every byte of frame n is n mod 256.
std::uint8_t ByteFor(std::uint64_t sequence) noexcept {
    return static_cast<std::uint8_t>(sequence & 0xFFu);
}

const char* Architecture() noexcept {
#if defined(_WIN64)
    return "x64";
#else
    return "x86";
#endif
}

}  // namespace

int main(int argc, char** argv) {
    std::uint64_t wait_ms = kDefaultWaitMs;
    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], "--wait-ms") == 0 && i + 1 < argc) {
            wait_ms = std::strtoull(argv[++i], nullptr, 10);
        }
    }

    std::printf("ring_conform (%s): waiting up to %llu ms for the producer\n", Architecture(),
                static_cast<unsigned long long>(wait_ms));
    std::fflush(stdout);

    CameraSection section;
    RingConsumer consumer;
    const std::uint64_t deadline = TickMs() + wait_ms;

    // Attach. `NotOurs` is a retry, not a failure: the producer clears the magic first during
    // initialisation and writes it last, so a consumer arriving mid-initialisation sees exactly it.
    while (true) {
        if (TickMs() > deadline) {
            std::fprintf(stderr, "no producer appeared within the wait window\n");
            return 2;
        }
        if (!section.mapped() && !section.Open()) {
            ::Sleep(10);
            continue;
        }
        std::uint32_t found = 0;
        const AttachStatus status = consumer.Attach(section.region(), &found);
        if (status == AttachStatus::Ok) {
            break;
        }
        if (status == AttachStatus::NotOurs) {
            ::Sleep(1);
            continue;
        }
        // The three failures are reported distinctly, because they are three different problems.
        switch (status) {
            case AttachStatus::Version:
                std::fprintf(stderr,
                             "the mapping is transport version %u, this build speaks version %u\n",
                             found, unifiedstream::kTransportVersion);
                break;
            case AttachStatus::Malformed:
                std::fprintf(stderr, "the mapping is internally inconsistent\n");
                break;
            case AttachStatus::RegionTooSmall:
                std::fprintf(stderr, "the mapping is smaller than the ring needs\n");
                break;
            default:
                break;
        }
        return 2;
    }

    std::printf("attached: %ux%u, generation %llu\n", consumer.width(), consumer.height(),
                static_cast<unsigned long long>(consumer.generation()));
    std::fflush(stdout);

    std::vector<std::uint8_t> buffer(unifiedstream::kMaxPayloadBytes, 0);

    std::uint64_t accepted = 0;
    std::uint64_t torn = 0;
    std::uint64_t in_flight = 0;
    std::uint64_t wrong = 0;
    std::uint64_t wrong_length = 0;
    std::uint64_t backwards = 0;
    std::uint64_t first_sequence = 0;
    std::uint64_t last_sequence = 0;

    std::uint64_t finish_at = 0;  // set once the producer stops or goes, to allow a short drain
    bool finished = false;

    while (!finished) {
        const std::uint64_t now = TickMs();
        if (now > deadline + kDrainMs) {
            std::fprintf(stderr, "the producer never stopped within the wait window\n");
            break;
        }

        FrameInfo info{};
        const ReadOutcome outcome = consumer.Read(buffer.data(), buffer.size(), &info);
        switch (outcome) {
            case ReadOutcome::Frame: {
                const std::size_t expected = I420Bytes(info.width, info.height);
                if (info.bytes != expected) {
                    ++wrong_length;
                    std::fprintf(stderr,
                                 "frame %llu is %u bytes, %zu expected at %ux%u\n",
                                 static_cast<unsigned long long>(info.sequence), info.bytes,
                                 expected, info.width, info.height);
                    break;
                }
                // Byte for byte against the published pattern. A frame spliced from two others
                // shows up here and nowhere else — the sequence protocol is supposed to make this
                // impossible, and this is the only place that claim is tested against a real race.
                const std::uint8_t want = ByteFor(info.sequence);
                bool intact = true;
                for (std::size_t i = 0; i < info.bytes; ++i) {
                    if (buffer[i] != want) {
                        intact = false;
                        break;
                    }
                }
                if (!intact) {
                    ++wrong;
                    std::fprintf(stderr, "frame %llu was spliced with another frame\n",
                                 static_cast<unsigned long long>(info.sequence));
                    break;
                }
                if (first_sequence == 0) {
                    first_sequence = info.sequence;
                }
                if (info.sequence <= last_sequence) {
                    ++backwards;
                    std::fprintf(stderr, "frame %llu arrived after %llu\n",
                                 static_cast<unsigned long long>(info.sequence),
                                 static_cast<unsigned long long>(last_sequence));
                }
                last_sequence = info.sequence;
                ++accepted;
                break;
            }
            case ReadOutcome::Torn:
                ++torn;
                break;
            case ReadOutcome::BeingWritten:
                ++in_flight;
                break;
            case ReadOutcome::NoFrame: {
                // Nothing to read. This is the only place the producer's state is consulted, and
                // the only thing that ends the run.
                const Liveness liveness = consumer.GetLiveness(now);
                if (liveness == Liveness::Stopped || liveness == Liveness::ProducerGone) {
                    if (finish_at == 0) {
                        finish_at = now + kDrainMs;
                    } else if (now >= finish_at) {
                        finished = true;
                    }
                }
                ::SwitchToThread();
                break;
            }
        }
    }

    // Frames the producer published between the first and the last this consumer accepted, that it
    // never saw at all: the cost of being lapped, which is expected and is not a failure.
    const std::uint64_t span =
        last_sequence >= first_sequence && first_sequence != 0 ? last_sequence - first_sequence + 1
                                                               : 0;
    const std::uint64_t lapped = span > accepted ? span - accepted : 0;

    std::printf("accepted %llu, rejected as torn %llu, rejected as in-flight %llu, lapped %llu\n",
                static_cast<unsigned long long>(accepted), static_cast<unsigned long long>(torn),
                static_cast<unsigned long long>(in_flight),
                static_cast<unsigned long long>(lapped));
    std::printf("frames accepted with wrong contents: %llu\n",
                static_cast<unsigned long long>(wrong + wrong_length));

    if (wrong + wrong_length > 0) {
        std::fprintf(stderr,
                     "FAIL: %llu frames were accepted with wrong contents. The C++ consumer and the "
                     "Rust producer disagree about the sequence protocol.\n",
                     static_cast<unsigned long long>(wrong + wrong_length));
        return 1;
    }
    if (backwards > 0) {
        std::fprintf(stderr, "FAIL: %llu frames arrived out of order\n",
                     static_cast<unsigned long long>(backwards));
        return 1;
    }
    if (accepted == 0) {
        std::fprintf(stderr, "FAIL: no frame was accepted, so this run proves nothing\n");
        return 1;
    }

    std::printf("ok\n");
    return 0;
}
