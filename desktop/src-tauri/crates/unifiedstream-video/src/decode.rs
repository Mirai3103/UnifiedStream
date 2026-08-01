//! MJPEG frame decoding: network JPEG bytes to the planar I420 frames v4l2 consumers expect.
//!
//! Pure Rust end to end (`zune-jpeg`), so a hostile frame can at worst return an error —
//! decode failures drop the frame, never the stream, per protocol §7.2.

use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::VideoError;

/// Chroma value used where an edge sample is missing; 128 is "no color" in both Cb and Cr.
const NEUTRAL_CHROMA: u8 = 128;

/// Decode one JPEG frame to planar I420 (YUV 4:2:0) at the negotiated geometry.
///
/// The frame must decode to exactly `width` x `height`: the loopback device's format is fixed
/// while a writer holds it, so a mismatched frame cannot be presented and is treated as
/// undecodable. Dimensions must be even — guaranteed by the resolutions the stream offers.
///
/// # Errors
///
/// Returns [`VideoError::Decode`] when the bytes are not a decodable JPEG or the decoded
/// dimensions do not match the negotiated ones.
pub fn decode_jpeg_to_i420(jpeg: &[u8], width: u32, height: u32) -> Result<Vec<u8>, VideoError> {
    // YCbCr output skips the decoder's RGB conversion; the device wants YUV anyway.
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::YCbCr);
    let mut decoder = JpegDecoder::new_with_options(jpeg, options);
    let pixels = decoder
        .decode()
        .map_err(|e| VideoError::Decode(e.to_string()))?;
    let (decoded_w, decoded_h) = decoder
        .dimensions()
        .ok_or_else(|| VideoError::Decode("decoder reported no dimensions".to_owned()))?;
    if (decoded_w, decoded_h) != (width as usize, height as usize) {
        return Err(VideoError::Decode(format!(
            "frame is {decoded_w}x{decoded_h}, stream negotiated {width}x{height}"
        )));
    }

    let (w, h) = (width as usize, height as usize);
    if pixels.len() != w * h * 3 {
        return Err(VideoError::Decode(format!(
            "decoder returned {} bytes for a {w}x{h} YCbCr frame",
            pixels.len()
        )));
    }

    Ok(ycbcr444_to_i420(&pixels, w, h))
}

/// Downsample interleaved YCbCr 4:4:4 to planar I420, averaging each 2x2 chroma block.
///
/// Callers guarantee even dimensions and `pixels.len() == w * h * 3`.
fn ycbcr444_to_i420(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 3 / 2);

    // Y plane: every first byte of each YCbCr triple.
    for triple in pixels.chunks_exact(3) {
        if let [y, _, _] = triple {
            out.push(*y);
        }
    }

    // Chroma planes: one Cb and one Cr per 2x2 pixel block, averaged.
    let row = w * 3;
    let chroma_at = |x: usize, y: usize, offset: usize| -> u32 {
        u32::from(
            pixels
                .get(y * row + x * 3 + offset)
                .copied()
                .unwrap_or(NEUTRAL_CHROMA),
        )
    };
    for offset in [1_usize, 2] {
        for by in (0..h).step_by(2) {
            for bx in (0..w).step_by(2) {
                let sum = chroma_at(bx, by, offset)
                    + chroma_at(bx + 1, by, offset)
                    + chroma_at(bx, by + 1, offset)
                    + chroma_at(bx + 1, by + 1, offset);
                #[allow(clippy::cast_possible_truncation, reason = "mean of four u8 fits u8")]
                out.push((sum / 4) as u8);
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a solid-color RGB image as a baseline JPEG.
    fn solid_jpeg(w: u16, h: u16, rgb: [u8; 3]) -> Vec<u8> {
        let mut jpeg = Vec::new();
        let encoder = jpeg_encoder::Encoder::new(&mut jpeg, 90);
        let pixels: Vec<u8> = std::iter::repeat(rgb)
            .take(usize::from(w) * usize::from(h))
            .flatten()
            .collect();
        encoder
            .encode(&pixels, w, h, jpeg_encoder::ColorType::Rgb)
            .expect("test JPEG must encode");
        jpeg
    }

    #[test]
    fn a_solid_gray_frame_should_decode_to_flat_planes() {
        let jpeg = solid_jpeg(64, 48, [128, 128, 128]);
        let i420 = decode_jpeg_to_i420(&jpeg, 64, 48).expect("must decode");
        assert_eq!(i420.len(), 64 * 48 * 3 / 2);

        // Gray has neutral chroma; luma sits near 128. JPEG is lossy, hence the tolerance.
        let (y_plane, chroma) = i420.split_at(64 * 48);
        assert!(
            y_plane.iter().all(|&v| v.abs_diff(128) <= 4),
            "luma must be flat gray"
        );
        assert!(
            chroma.iter().all(|&v| v.abs_diff(128) <= 4),
            "chroma must be neutral"
        );
    }

    #[test]
    fn a_red_frame_should_have_high_cr() {
        let jpeg = solid_jpeg(32, 32, [255, 0, 0]);
        let i420 = decode_jpeg_to_i420(&jpeg, 32, 32).expect("must decode");
        let cr_plane = &i420[32 * 32 + (16 * 16)..];
        assert!(
            cr_plane.iter().all(|&v| v > 200),
            "red must land in the top of the Cr range"
        );
    }

    #[test]
    fn garbage_bytes_should_fail_decode_not_panic() {
        let garbage = vec![0xAB_u8; 4096];
        assert!(matches!(
            decode_jpeg_to_i420(&garbage, 64, 48),
            Err(VideoError::Decode(_))
        ));
    }

    #[test]
    fn a_truncated_jpeg_should_fail_decode_not_panic() {
        let mut jpeg = solid_jpeg(64, 48, [10, 200, 30]);
        jpeg.truncate(jpeg.len() / 3);
        assert!(decode_jpeg_to_i420(&jpeg, 64, 48).is_err());
    }

    #[test]
    fn a_frame_at_the_wrong_dimensions_should_be_refused() {
        let jpeg = solid_jpeg(64, 48, [128, 128, 128]);
        let err = decode_jpeg_to_i420(&jpeg, 1280, 720).expect_err("must refuse");
        assert!(matches!(err, VideoError::Decode(_)));
    }

    #[test]
    fn an_empty_payload_should_fail_cleanly() {
        assert!(decode_jpeg_to_i420(&[], 64, 48).is_err());
    }
}
