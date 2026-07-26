package com.laffy.unifiedstream.session

import com.laffy.unifiedstream.protocol.StreamRefusal
import com.laffy.unifiedstream.protocol.VideoParams

/**
 * Lifecycle of the camera stream, driven by [SessionManager].
 *
 * Mirrors [MicStreamState]: the phone is the source for both streams, and the UI renders the
 * camera independently of connection health.
 */
sealed interface CameraStreamState {
    /** Not streaming, and not trying to. */
    data object Inactive : CameraStreamState

    /** `stream_start` sent; waiting for the desktop's answer. */
    data object Starting : CameraStreamState

    /** The desktop accepted; video frames may be sent. */
    data class Active(val params: VideoParams) : CameraStreamState

    /** The desktop refused the stream. */
    data class Refused(val reason: StreamRefusal) : CameraStreamState

    /** A local failure — capture died, permission denied, or the desktop never answered. */
    data class Error(val message: String) : CameraStreamState

    /** Whether video frames may currently be sent. */
    val isActive: Boolean get() = this is Active

    /** Text to show the user, or null when the state needs no explanation. */
    val display: String?
        get() = when (this) {
            Inactive, Starting, is Active -> null
            is Refused -> when (reason) {
                StreamRefusal.NOT_NEGOTIATED -> "Device does not accept a camera"
                StreamRefusal.UNSUPPORTED_CODEC -> "Device cannot decode this video format"
                StreamRefusal.UNSUPPORTED_STREAM -> "Device does not recognise the camera stream"
                StreamRefusal.BUSY -> "Device is busy"
                StreamRefusal.INTERNAL -> "Device virtual camera is unavailable"
                StreamRefusal.UNKNOWN -> "Device refused the camera"
            }
            is Error -> message
        }
}
