//------------------------------------------------------------------------------
// section.cpp — see section.h.
//------------------------------------------------------------------------------

#include "section.h"

namespace unifiedstream {

std::uint64_t TickMs() noexcept {
    return ::GetTickCount64();
}

bool CameraSection::Open() noexcept {
    Close();

    // `FILE_MAP_READ` and nothing more. A consumer asking for write access is refused by the
    // descriptor, and a filter that asked for it would fail to attach on every machine.
    HANDLE section = ::OpenFileMappingW(FILE_MAP_READ, FALSE, kSectionName);
    if (section == nullptr) {
        // No producer has created the section. The ordinary state before the user starts a stream:
        // the caller retries on its timer and shows the placeholder meanwhile.
        return false;
    }

    const void* view = ::MapViewOfFile(section, FILE_MAP_READ, 0, 0, kRingBytes);
    if (view == nullptr) {
        ::CloseHandle(section);
        return false;
    }

    section_ = section;
    view_ = view;
    return true;
}

void CameraSection::Close() noexcept {
    if (view_ != nullptr) {
        ::UnmapViewOfFile(view_);
        view_ = nullptr;
    }
    if (section_ != nullptr) {
        ::CloseHandle(section_);
        section_ = nullptr;
    }
}

}  // namespace unifiedstream
