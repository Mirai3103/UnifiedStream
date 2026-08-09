//------------------------------------------------------------------------------
// video.h — the two things the filter does to pixels: scale a ring frame into the connected
// geometry, and generate a picture when there is no ring frame to show.
//
// Both run on the host application's streaming thread, so neither allocates, neither touches a
// file, and neither reads an asset. There is no decoder here and there must never be one: decision
// 3 of `docs/design.windows.md` keeps JPEG on the desktop's side of the boundary precisely so this
// component — loaded into Zoom, Chrome, and Discord — never parses network-originated bytes.
//------------------------------------------------------------------------------

#ifndef UNIFIEDSTREAM_DSHOW_VIDEO_H
#define UNIFIEDSTREAM_DSHOW_VIDEO_H

#include <cstdint>

namespace unifiedstream {

/// Scale a full I420 frame into another I420 frame, preserving aspect ratio and letterboxing the
/// remainder with video black.
///
/// **The caller is expected to bypass this entirely when the geometries already match**, which is
/// the default configuration on both ends and the reason the cost of the general case is
/// acceptable. `ScaleOrCopyI420` below does that check.
///
/// Plane-wise and nearest-neighbour rather than averaging: this is a webcam picture on someone
/// else's streaming thread, and the sampling quality difference does not justify reading four
/// source pixels per destination pixel in a process this code is a guest in.
///
/// `src` must hold `I420Bytes(src_w, src_h)` bytes and `dst` `I420Bytes(dst_w, dst_h)`. All four
/// dimensions must be even, which I420's 2x2 chroma subsampling requires and both the producer and
/// the filter's own media types already guarantee.
void ScaleI420(const std::uint8_t* src, std::uint32_t src_w, std::uint32_t src_h,
               std::uint8_t* dst, std::uint32_t dst_w, std::uint32_t dst_h) noexcept;

/// `ScaleI420`, with the identity case short-circuited to a straight copy.
void ScaleOrCopyI420(const std::uint8_t* src, std::uint32_t src_w, std::uint32_t src_h,
                     std::uint8_t* dst, std::uint32_t dst_w, std::uint32_t dst_h) noexcept;

/// Fill an I420 frame with the placeholder: a flat field, generated, needing no asset, no font, and
/// no file access.
///
/// Shown whenever the producer has stopped cleanly, has gone without stopping, or has never
/// existed. Deliberately not black — a black picture reads as a broken camera, and this one is
/// meant to read as a camera that is working and has nothing to show yet.
void FillPlaceholderI420(std::uint8_t* dst, std::uint32_t width, std::uint32_t height) noexcept;

}  // namespace unifiedstream

#endif  // UNIFIEDSTREAM_DSHOW_VIDEO_H
