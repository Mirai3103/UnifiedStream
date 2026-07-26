package com.laffy.unifiedstream.video

/** JPEG quality for camera frames: 720p lands around 40–80 KB, ~10–20 Mbps at 30 fps. */
const val CAMERA_JPEG_QUALITY = 70

/**
 * The camera-free half of the capture pipeline: frame-rate pacing and JPEG encoding, with the
 * actual compressor injected so the logic is unit-testable without Android — the
 * `MicController` pattern.
 *
 * Pacing enforces the stream's negotiated `max_fps` (protocol §7.1): a camera delivering
 * faster than the cap has its excess frames skipped before the encode cost is paid. The
 * scheduler is credit-based rather than a naive `now - last >= interval`, which with jittery
 * camera timestamps would systematically undershoot the target rate.
 *
 * @param maxFps upper bound on encoded frames per second. Must be positive.
 * @param quality JPEG quality passed to the compressor.
 * @param compress turns one NV21 frame into JPEG bytes; `YuvImage.compressToJpeg` in
 *   production, a stub in tests.
 */
class FrameEncoder(
    private val maxFps: Int,
    private val quality: Int = CAMERA_JPEG_QUALITY,
    private val compress: (nv21: ByteArray, width: Int, height: Int, quality: Int) -> ByteArray,
) {
    init {
        require(maxFps > 0) { "maxFps must be positive, got $maxFps" }
    }

    private val intervalUs: Long = 1_000_000L / maxFps

    /** When the next frame becomes due, or [Long.MIN_VALUE] before the first frame. */
    private var nextDueUs: Long = Long.MIN_VALUE

    /**
     * Encode one captured frame, or return null when `max_fps` pacing says to skip it.
     *
     * The frame bytes come from a supplier so a skipped frame pays neither the YUV
     * conversion nor the JPEG cost — at 60 fps paced to 30, that is half the work gone.
     *
     * @param nowUs a monotonic clock in microseconds; only differences matter.
     */
    fun encode(width: Int, height: Int, nowUs: Long, nv21: () -> ByteArray): ByteArray? {
        if (!shouldEncode(nowUs)) return null
        return compress(nv21(), width, height, quality)
    }

    /**
     * Whether a frame arriving at [nowUs] is within the frame-rate budget, consuming the
     * budget when it is.
     */
    fun shouldEncode(nowUs: Long): Boolean {
        if (nowUs < nextDueUs) return false
        // After a gap longer than one interval the schedule restarts from now — crediting the
        // idle time would let a burst exceed max_fps.
        nextDueUs = if (nextDueUs == Long.MIN_VALUE || nowUs - nextDueUs > intervalUs) {
            nowUs + intervalUs
        } else {
            nextDueUs + intervalUs
        }
        return true
    }
}
