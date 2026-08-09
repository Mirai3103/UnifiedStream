//------------------------------------------------------------------------------
// video.cpp — see video.h.
//------------------------------------------------------------------------------

#include "video.h"

#include <cstring>

#include "transport.h"

namespace unifiedstream {
namespace {

/// Video black in I420: luma 16, chroma centred. Filling the letterbox with 0 instead would give a
/// blacker-than-black bar that some encoders clip oddly.
constexpr std::uint8_t kBlackY = 16;
constexpr std::uint8_t kNeutralC = 128;

/// The placeholder's colour: a flat, mid-dark slate. Distinguishable at a glance from both a black
/// picture (which reads as a broken camera) and from real video.
constexpr std::uint8_t kPlaceholderY = 52;
constexpr std::uint8_t kPlaceholderU = 132;
constexpr std::uint8_t kPlaceholderV = 122;

/// Nearest-neighbour scale of one plane, stepping the source in 16.16 fixed point so the inner loop
/// carries no division.
void ScalePlane(const std::uint8_t* src, std::uint32_t src_w, std::uint32_t src_h,
                std::uint8_t* dst, std::uint32_t dst_stride, std::uint32_t dst_x,
                std::uint32_t dst_y, std::uint32_t out_w, std::uint32_t out_h) noexcept {
    if (out_w == 0 || out_h == 0 || src_w == 0 || src_h == 0) {
        return;
    }
    const std::uint32_t x_step = (src_w << 16) / out_w;
    const std::uint32_t y_step = (src_h << 16) / out_h;

    std::uint32_t src_y_fixed = 0;
    for (std::uint32_t y = 0; y < out_h; ++y) {
        std::uint32_t sy = src_y_fixed >> 16;
        if (sy >= src_h) {
            sy = src_h - 1;
        }
        const std::uint8_t* src_row = src + static_cast<std::size_t>(sy) * src_w;
        std::uint8_t* dst_row =
            dst + static_cast<std::size_t>(dst_y + y) * dst_stride + dst_x;

        std::uint32_t src_x_fixed = 0;
        for (std::uint32_t x = 0; x < out_w; ++x) {
            std::uint32_t sx = src_x_fixed >> 16;
            if (sx >= src_w) {
                sx = src_w - 1;
            }
            dst_row[x] = src_row[sx];
            src_x_fixed += x_step;
        }
        src_y_fixed += y_step;
    }
}

}  // namespace

void ScaleI420(const std::uint8_t* src, std::uint32_t src_w, std::uint32_t src_h,
               std::uint8_t* dst, std::uint32_t dst_w, std::uint32_t dst_h) noexcept {
    if (src == nullptr || dst == nullptr || src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0) {
        return;
    }

    // Fit the source inside the destination, preserving aspect ratio. Rounded down to even in both
    // dimensions, because the chroma planes are half-resolution and an odd inner rectangle would
    // put luma and chroma on different grids.
    std::uint64_t inner_w = static_cast<std::uint64_t>(dst_w);
    std::uint64_t inner_h =
        static_cast<std::uint64_t>(dst_w) * src_h / src_w;
    if (inner_h > dst_h) {
        inner_h = dst_h;
        inner_w = static_cast<std::uint64_t>(dst_h) * src_w / src_h;
    }
    std::uint32_t fit_w = static_cast<std::uint32_t>(inner_w) & ~1u;
    std::uint32_t fit_h = static_cast<std::uint32_t>(inner_h) & ~1u;
    if (fit_w == 0) {
        fit_w = 2;
    }
    if (fit_h == 0) {
        fit_h = 2;
    }
    const std::uint32_t off_x = ((dst_w - fit_w) / 2) & ~1u;
    const std::uint32_t off_y = ((dst_h - fit_h) / 2) & ~1u;

    const std::size_t dst_luma = static_cast<std::size_t>(dst_w) * dst_h;
    const std::size_t dst_chroma = dst_luma / 4;
    std::uint8_t* dst_y_plane = dst;
    std::uint8_t* dst_u_plane = dst + dst_luma;
    std::uint8_t* dst_v_plane = dst_u_plane + dst_chroma;

    // Letterbox first, then draw over the middle. Painting the whole frame is one linear pass and
    // is cheaper than working out and filling four rectangles.
    std::memset(dst_y_plane, kBlackY, dst_luma);
    std::memset(dst_u_plane, kNeutralC, dst_chroma * 2);

    const std::size_t src_luma = static_cast<std::size_t>(src_w) * src_h;
    const std::size_t src_chroma = src_luma / 4;
    const std::uint8_t* src_y_plane = src;
    const std::uint8_t* src_u_plane = src + src_luma;
    const std::uint8_t* src_v_plane = src_u_plane + src_chroma;

    ScalePlane(src_y_plane, src_w, src_h, dst_y_plane, dst_w, off_x, off_y, fit_w, fit_h);
    ScalePlane(src_u_plane, src_w / 2, src_h / 2, dst_u_plane, dst_w / 2, off_x / 2, off_y / 2,
               fit_w / 2, fit_h / 2);
    ScalePlane(src_v_plane, src_w / 2, src_h / 2, dst_v_plane, dst_w / 2, off_x / 2, off_y / 2,
               fit_w / 2, fit_h / 2);
}

void ScaleOrCopyI420(const std::uint8_t* src, std::uint32_t src_w, std::uint32_t src_h,
                     std::uint8_t* dst, std::uint32_t dst_w, std::uint32_t dst_h) noexcept {
    if (src == nullptr || dst == nullptr) {
        return;
    }
    if (src_w == dst_w && src_h == dst_h) {
        // The default configuration on both ends: the phone publishes at the geometry the
        // application connected at, and the scaler costs nothing at all.
        std::memcpy(dst, src, I420Bytes(dst_w, dst_h));
        return;
    }
    ScaleI420(src, src_w, src_h, dst, dst_w, dst_h);
}

void FillPlaceholderI420(std::uint8_t* dst, std::uint32_t width, std::uint32_t height) noexcept {
    if (dst == nullptr || width == 0 || height == 0) {
        return;
    }
    const std::size_t luma = static_cast<std::size_t>(width) * height;
    const std::size_t chroma = luma / 4;
    std::memset(dst, kPlaceholderY, luma);
    std::memset(dst + luma, kPlaceholderU, chroma);
    std::memset(dst + luma + chroma, kPlaceholderV, chroma);
}

}  // namespace unifiedstream
