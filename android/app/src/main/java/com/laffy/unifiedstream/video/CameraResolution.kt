package com.laffy.unifiedstream.video

/**
 * The resolutions the camera offers.
 *
 * All are sizes every CameraX device supports, all even-dimensioned (the desktop's I420
 * output subsamples chroma 2x2), and all within the desktop's 1080p acceptance cap.
 */
enum class CameraResolution(val label: String, val width: Int, val height: Int) {
    SD("480p", 640, 480),
    HD("720p", 1280, 720),
    FHD("1080p", 1920, 1080),
}
