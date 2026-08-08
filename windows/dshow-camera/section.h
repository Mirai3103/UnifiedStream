//------------------------------------------------------------------------------
// section.h — opening the desktop's named section, read-only.
//
// The Win32 half of attaching, kept out of `transport.cpp` for the same reason `transport.rs` keeps
// Win32 out of itself: the protocol is portable and the kernel objects are not. Shared by the
// filter and by `ring_conform.exe`, which attaches to the same real section.
//------------------------------------------------------------------------------

#ifndef UNIFIEDSTREAM_DSHOW_SECTION_H
#define UNIFIEDSTREAM_DSHOW_SECTION_H

#include <windows.h>

#include <cstdint>

#include "transport.h"

namespace unifiedstream {

/// Milliseconds on the only clock every process in the session agrees on.
///
/// The heartbeat is compared across a process boundary, so it cannot be a per-process timer. The
/// producer measures its heartbeat on `GetTickCount64` too; the comparison is meaningless otherwise.
std::uint64_t TickMs() noexcept;

/// A read-only view of `Local\UnifiedStream.Camera`, unmapped and closed on destruction.
///
/// **Read-only is the contract, not a precaution.** The section's descriptor grants consumers
/// `GENERIC_READ` and carries a Low mandatory label with no-write-up, so asking for write access is
/// refused — which is the intended shape of the boundary.
class CameraSection {
public:
    CameraSection() noexcept = default;
    ~CameraSection() noexcept { Close(); }

    CameraSection(const CameraSection&) = delete;
    CameraSection& operator=(const CameraSection&) = delete;

    /// Try to open and map the section. Returns false when no producer has ever created it, which
    /// is the ordinary state before the user starts a stream and is not an error.
    bool Open() noexcept;

    /// Unmap and close. Safe to call when already closed.
    void Close() noexcept;

    [[nodiscard]] bool mapped() const noexcept { return view_ != nullptr; }

    /// The mapped bytes, as the transport's portable view of them. Invalid once `Close` has run.
    [[nodiscard]] RingRegion region() const noexcept {
        return RingRegion(static_cast<const std::uint8_t*>(view_), mapped() ? kRingBytes : 0);
    }

private:
    HANDLE section_ = nullptr;
    const void* view_ = nullptr;
};

}  // namespace unifiedstream

#endif  // UNIFIEDSTREAM_DSHOW_SECTION_H
