package com.laffy.unifiedstream.session

import com.laffy.unifiedstream.protocol.AudioParams
import com.laffy.unifiedstream.protocol.StreamRefusal

/**
 * Lifecycle of the speaker stream, driven by [SessionManager].
 *
 * The phone is the *sink* here — the inverse of the microphone — so "on" means asking the
 * desktop to start sending via `stream_request`, then accepting its `stream_start`.
 */
sealed interface SpeakerStreamState {
    /** Not playing, and not trying to. */
    data object Inactive : SpeakerStreamState

    /** `stream_request` sent; waiting for the desktop's `stream_start`. */
    data object Requesting : SpeakerStreamState

    /** The stream is accepted; audio frames are being played. */
    data class Active(val params: AudioParams) : SpeakerStreamState

    /** The desktop refused to start the stream. */
    data class Refused(val reason: StreamRefusal) : SpeakerStreamState

    /** A local failure — playback died, or the desktop never answered. */
    data class Error(val message: String) : SpeakerStreamState

    /** Whether audio frames should currently be decoded and played. */
    val isActive: Boolean get() = this is Active

    /** Text to show the user, or null when the state needs no explanation. */
    val display: String?
        get() = when (this) {
            Inactive, Requesting, is Active -> null
            is Refused -> when (reason) {
                StreamRefusal.NOT_NEGOTIATED -> "PC does not offer a speaker stream"
                StreamRefusal.UNSUPPORTED_CODEC -> "PC cannot encode a format this phone plays"
                StreamRefusal.UNSUPPORTED_STREAM -> "PC does not recognise the speaker stream"
                StreamRefusal.BUSY -> "PC is busy"
                StreamRefusal.INTERNAL -> "PC audio capture is unavailable"
                StreamRefusal.UNKNOWN -> "PC refused the speaker stream"
            }
            is Error -> message
        }
}
