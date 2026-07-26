package com.laffy.unifiedstream.video

import android.content.Context
import android.graphics.ImageFormat
import android.graphics.Rect
import android.graphics.YuvImage
import android.util.Log
import android.util.Size
import androidx.camera.core.AspectRatio
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.AspectRatioStrategy
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import com.laffy.unifiedstream.protocol.VideoParams
import java.io.ByteArrayOutputStream
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

private const val TAG = "CameraCapture"

/** Which physical camera to capture from. */
enum class CameraFacing {
    FRONT,
    BACK;

    internal fun selector(): CameraSelector = when (this) {
        FRONT -> CameraSelector.DEFAULT_FRONT_CAMERA
        BACK -> CameraSelector.DEFAULT_BACK_CAMERA
    }
}

/**
 * Owns the CameraX session: YUV capture at the negotiated geometry, `max_fps` pacing, JPEG
 * encoding, and an optional local preview — the video counterpart of `MicCapture`.
 *
 * Capture binds to a lifecycle this class drives itself rather than to an Activity or the
 * process: the phone is a webcam precisely when the user has switched away, so the camera must
 * stay open while the app is backgrounded (the foreground service's `camera` type is what
 * makes the platform allow that). The caller is responsible for holding `CAMERA` before
 * [start]; a denied permission surfaces as a start failure, never a crash.
 *
 * @param onJpeg called on the analysis thread with each encoded frame, already paced to the
 *   negotiated frame-rate cap.
 * @param onFailure called at most once per [start] if capture cannot start or dies.
 * @param onActualResolution called at most once per [start] when the camera delivers a usable
 *   geometry other than the negotiated one. The observer renegotiates the stream to the
 *   delivered size (a §3.9.1 replacement start); no frames flow until the sizes agree, since
 *   the desktop's device format is fixed and mismatched frames would all be dropped there.
 */
class CameraCapture(
    private val context: Context,
    private val onJpeg: (ByteArray) -> Unit,
    private val onFailure: (String) -> Unit,
    private val onActualResolution: (width: Int, height: Int) -> Unit = { _, _ -> },
) {
    /** Manually driven lifecycle: RESUMED between [start] and [stop], nothing else. */
    private val lifecycleOwner = object : LifecycleOwner {
        val registry = LifecycleRegistry(this)
        override val lifecycle: Lifecycle get() = registry
    }

    private var provider: ProcessCameraProvider? = null
    private var preview: Preview? = null
    private var analysisExecutor: ExecutorService? = null
    private var surfaceProvider: Preview.SurfaceProvider? = null
    private var params: VideoParams? = null
    private var facing: CameraFacing = CameraFacing.BACK
    private var failed = false

    /** Set once a mismatched geometry has been reported, so renegotiation fires only once. */
    @Volatile
    private var mismatchReported = false

    @Volatile
    private var running = false

    /** Whether capture is currently running. */
    val isRunning: Boolean get() = running

    /**
     * Start capturing at the negotiated geometry. Asynchronous: frames begin flowing once the
     * camera opens, and failures arrive through `onFailure`. Must be called on the main
     * thread, as must every other method of this class.
     */
    fun start(params: VideoParams, facing: CameraFacing) {
        if (running) return
        running = true
        failed = false
        this.params = params
        this.facing = facing
        lifecycleOwner.registry.currentState = Lifecycle.State.RESUMED
        analysisExecutor = Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "camera-encode")
        }

        val future = ProcessCameraProvider.getInstance(context)
        future.addListener({
            if (!running) return@addListener
            val fresh = try {
                future.get()
            } catch (e: Exception) {
                fail("Camera is unavailable: ${e.message}")
                return@addListener
            }
            provider = fresh
            bind(fresh)
        }, ContextCompat.getMainExecutor(context))
    }

    /**
     * Switch between the front and back camera.
     *
     * Rebinds capture without touching the stream: the desktop sees at most a brief frame
     * gap, which it tolerates by holding the last frame (protocol §7).
     */
    fun setFacing(facing: CameraFacing) {
        if (this.facing == facing) return
        this.facing = facing
        val bound = provider ?: return
        if (running) bind(bound)
    }

    /** The camera currently selected. */
    val currentFacing: CameraFacing get() = facing

    /**
     * Attach or detach the local preview surface. The stream does not depend on this: a
     * backgrounded app streams with no surface attached.
     */
    fun setSurfaceProvider(provider: Preview.SurfaceProvider?) {
        surfaceProvider = provider
        preview?.setSurfaceProvider(provider)
    }

    /** Stop capturing and release the camera. Idempotent. */
    fun stop() {
        if (!running && provider == null) return
        running = false
        // Moving the lifecycle to CREATED is what makes CameraX close the camera — and with
        // it the platform's camera-in-use indicator — immediately.
        lifecycleOwner.registry.currentState = Lifecycle.State.CREATED
        provider?.unbindAll()
        provider = null
        preview = null
        params = null
        analysisExecutor?.shutdown()
        analysisExecutor = null
    }

    private fun bind(provider: ProcessCameraProvider) {
        val params = this.params ?: return
        val executor = analysisExecutor ?: return
        mismatchReported = false

        val encoder = FrameEncoder(
            maxFps = params.maxFps.coerceAtLeast(1),
            compress = ::compressNv21,
        )

        // The aspect-ratio strategy must match the requested geometry: CameraX's default is
        // 4:3 and it outranks the resolution strategy, so a 16:9 request would otherwise be
        // "fulfilled" with the nearest 4:3 size (1280x720 becoming 720x540).
        val aspect = if (params.width * 3 == params.height * 4) {
            AspectRatioStrategy(AspectRatio.RATIO_4_3, AspectRatioStrategy.FALLBACK_RULE_AUTO)
        } else {
            AspectRatioStrategy(AspectRatio.RATIO_16_9, AspectRatioStrategy.FALLBACK_RULE_AUTO)
        }
        val selector = ResolutionSelector.Builder()
            .setAspectRatioStrategy(aspect)
            .setResolutionStrategy(
                ResolutionStrategy(
                    Size(params.width, params.height),
                    ResolutionStrategy.FALLBACK_RULE_CLOSEST_LOWER_THEN_HIGHER,
                ),
            )
            .build()

        val analysis = ImageAnalysis.Builder()
            .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
            .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_YUV_420_888)
            .setResolutionSelector(selector)
            .build()

        analysis.setAnalyzer(executor) { image -> analyze(image, params, encoder) }

        // The preview follows the stream's aspect ratio so what the user frames is what the
        // desktop sees.
        val freshPreview = Preview.Builder()
            .setResolutionSelector(
                ResolutionSelector.Builder().setAspectRatioStrategy(aspect).build(),
            )
            .build()
        freshPreview.setSurfaceProvider(surfaceProvider)
        preview = freshPreview

        try {
            provider.unbindAll()
            provider.bindToLifecycle(lifecycleOwner, facing.selector(), freshPreview, analysis)
        } catch (e: Exception) {
            fail("Could not open the camera: ${e.message}")
        }
    }

    private fun analyze(image: ImageProxy, params: VideoParams, encoder: FrameEncoder) {
        image.use { frame ->
            if (!running) return
            // The desktop's device format is fixed at the negotiated geometry, so frames at
            // any other size would all be dropped over there. Some devices legitimately lack
            // the requested size and deliver the closest one instead — that is renegotiated
            // to the delivered geometry (§3.9.1 replacement), not treated as a failure.
            if (frame.width != params.width || frame.height != params.height) {
                if (mismatchReported) return
                mismatchReported = true
                if (frame.width % 2 == 0 && frame.height % 2 == 0) {
                    Log.i(
                        TAG,
                        "camera delivered ${frame.width}x${frame.height}, " +
                            "renegotiating from ${params.width}x${params.height}",
                    )
                    onActualResolution(frame.width, frame.height)
                } else {
                    // Odd geometry cannot be 4:2:0 subsampled anywhere in the pipeline.
                    fail("Camera delivered unusable ${frame.width}x${frame.height}")
                }
                return
            }
            val jpeg = try {
                encoder.encode(frame.width, frame.height, frame.imageInfo.timestamp / 1_000) {
                    yuv420888ToNv21(frame)
                }
            } catch (e: Exception) {
                // One bad frame is dropped, not fatal; the camera keeps delivering.
                Log.w(TAG, "frame encode failed", e)
                null
            }
            if (jpeg != null) onJpeg(jpeg)
        }
    }

    private fun fail(message: String) {
        if (failed) return
        failed = true
        Log.w(TAG, message)
        onFailure(message)
    }

    private fun compressNv21(nv21: ByteArray, width: Int, height: Int, quality: Int): ByteArray {
        val out = ByteArrayOutputStream(width * height / 8)
        YuvImage(nv21, ImageFormat.NV21, width, height, null)
            .compressToJpeg(Rect(0, 0, width, height), quality, out)
        return out.toByteArray()
    }

    /** Repack CameraX's three-plane YUV_420_888 into NV21 (Y plane, then interleaved VU). */
    private fun yuv420888ToNv21(image: ImageProxy): ByteArray {
        val width = image.width
        val height = image.height
        val out = ByteArray(width * height * 3 / 2)

        val yPlane = image.planes[0]
        val yBuffer = yPlane.buffer
        var outPos = 0
        if (yPlane.pixelStride == 1) {
            for (row in 0 until height) {
                yBuffer.position(row * yPlane.rowStride)
                yBuffer.get(out, outPos, width)
                outPos += width
            }
        } else {
            for (row in 0 until height) {
                var bufPos = row * yPlane.rowStride
                for (col in 0 until width) {
                    out[outPos++] = yBuffer.get(bufPos)
                    bufPos += yPlane.pixelStride
                }
            }
        }

        val uPlane = image.planes[1]
        val vPlane = image.planes[2]
        val uBuffer = uPlane.buffer
        val vBuffer = vPlane.buffer
        for (row in 0 until height / 2) {
            var uPos = row * uPlane.rowStride
            var vPos = row * vPlane.rowStride
            for (col in 0 until width / 2) {
                out[outPos++] = vBuffer.get(vPos)
                out[outPos++] = uBuffer.get(uPos)
                uPos += uPlane.pixelStride
                vPos += vPlane.pixelStride
            }
        }
        return out
    }
}
