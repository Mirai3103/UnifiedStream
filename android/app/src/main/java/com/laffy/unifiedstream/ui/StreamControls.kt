package com.laffy.unifiedstream.ui

import androidx.camera.core.Preview
import com.laffy.unifiedstream.audio.AudioLevel
import com.laffy.unifiedstream.session.CameraStreamState
import com.laffy.unifiedstream.session.MicStreamState
import com.laffy.unifiedstream.session.SpeakerStreamState
import com.laffy.unifiedstream.video.CameraFacing
import com.laffy.unifiedstream.video.CameraResolution

/** Everything the camera card renders and calls back into. */
data class CameraControls(
    val state: CameraStreamState = CameraStreamState.Inactive,
    val facing: CameraFacing = CameraFacing.BACK,
    val resolution: CameraResolution = CameraResolution.HD,
    val permissionNeeded: Boolean = false,
    val onToggle: (Boolean) -> Unit = {},
    val onFacingToggle: () -> Unit = {},
    val onResolutionSelect: (CameraResolution) -> Unit = {},
    /** Attaches the local preview surface while the card shows one; null detaches. */
    val onPreviewSurface: (Preview.SurfaceProvider?) -> Unit = {},
)

/** Everything the microphone card renders and calls back into. */
data class MicControls(
    val state: MicStreamState = MicStreamState.Inactive,
    val level: AudioLevel = AudioLevel(),
    val muted: Boolean = false,
    val gain: Float = 1f,
    val noiseSuppression: Boolean = false,
    val noiseSuppressionAvailable: Boolean = false,
    val permissionNeeded: Boolean = false,
    val onToggle: (Boolean) -> Unit = {},
    val onMuteToggle: (Boolean) -> Unit = {},
    val onGainChange: (Float) -> Unit = {},
    val onNoiseSuppressionToggle: (Boolean) -> Unit = {},
)

/** Everything the speaker card renders and calls back into. */
data class SpeakerControls(
    val state: SpeakerStreamState = SpeakerStreamState.Inactive,
    val level: AudioLevel = AudioLevel(),
    val muted: Boolean = false,
    val volume: Float = 1f,
    val onToggle: (Boolean) -> Unit = {},
    val onMuteToggle: (Boolean) -> Unit = {},
    val onVolumeChange: (Float) -> Unit = {},
)
