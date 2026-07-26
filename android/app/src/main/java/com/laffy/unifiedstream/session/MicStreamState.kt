package com.laffy.unifiedstream.session

import com.laffy.unifiedstream.protocol.AudioParams
import com.laffy.unifiedstream.protocol.StreamRefusal

/**
 * Lifecycle of the microphone stream, driven by [SessionManager].
 *
 * Separate from [ConnectionState]: the session can be healthy while the mic is off, refused,
 * or broken, and the UI renders the two independently.
 */
sealed interface MicStreamState {
    /** Not streaming, and not trying to. */
    data object Inactive : MicStreamState

    /** `stream_start` sent; waiting for the desktop's answer. */
    data object Starting : MicStreamState

    /** The desktop accepted; audio frames may be sent. */
    data class Active(val params: AudioParams) : MicStreamState

    /** The desktop refused the stream. */
    data class Refused(val reason: StreamRefusal) : MicStreamState

    /** A local failure — capture died, or the desktop never answered. */
    data class Error(val message: String) : MicStreamState

    /** Whether audio frames may currently be sent. */
    val isActive: Boolean get() = this is Active

    /** Text to show the user, or null when the state needs no explanation. */
    val display: String?
        get() = when (this) {
            Inactive, Starting, is Active -> null
            is Refused -> when (reason) {
                StreamRefusal.NOT_NEGOTIATED -> "Device does not accept a microphone"
                StreamRefusal.UNSUPPORTED_CODEC -> "Device cannot decode this audio format"
                StreamRefusal.UNSUPPORTED_STREAM -> "Device does not recognise the microphone stream"
                StreamRefusal.BUSY -> "Device is busy"
                StreamRefusal.INTERNAL -> "Device audio output is unavailable"
                StreamRefusal.UNKNOWN -> "Device refused the microphone"
            }
            is Error -> message
        }
}
