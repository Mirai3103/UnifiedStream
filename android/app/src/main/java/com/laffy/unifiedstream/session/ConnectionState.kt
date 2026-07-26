package com.laffy.unifiedstream.session

/**
 * Why a connection attempt ended in [ConnectionState.Failed].
 *
 * Mirrors `unifiedstream_net::session::FailureReason`.
 */
sealed interface FailureReason {
    /** The control connection could not be opened. */
    data class Unreachable(val detail: String) : FailureReason

    /** The peer speaks a protocol version this build does not support. */
    data class VersionMismatch(val detail: String) : FailureReason

    /** The desktop user declined pairing, or the prompt timed out. */
    data object Rejected : FailureReason

    /** The desktop already has an active session with another phone. */
    data object Busy : FailureReason

    /** Every reconnection attempt failed. */
    data object ReconnectExhausted : FailureReason

    /** Anything else, carrying its display text. */
    data class Other(val detail: String) : FailureReason

    /** Text to show the user. */
    val display: String
        get() = when (this) {
            is Unreachable -> "Could not reach device: $detail"
            is VersionMismatch -> "Incompatible version: $detail"
            Rejected -> "Pairing was declined"
            Busy -> "Device is already paired with another phone"
            ReconnectExhausted -> "Could not reconnect"
            is Other -> detail
        }
}

/**
 * Lifecycle of a connection to a peer.
 *
 * The UI never infers status from socket state — it reads this. That is what keeps the phone
 * and the PC showing the same thing for the same condition.
 */
sealed interface ConnectionState {
    /** Nothing in progress. */
    data object Idle : ConnectionState

    /** Browsing for peers, none selected. */
    data object Discovering : ConnectionState

    /** Control connection opening, or handshake in flight. */
    data class Connecting(val peerName: String) : ConnectionState

    /** Session established; media may flow. */
    data class Connected(
        val peerName: String,
        val sessionId: Long,
        val negotiatedCaps: List<String> = emptyList(),
        val mediaPort: Int = 0,
    ) : ConnectionState

    /** Session dropped; retrying with backoff. */
    data class Reconnecting(
        val peerName: String,
        val attempt: Int,
        val maxAttempts: Int,
    ) : ConnectionState

    /** Gave up. Carries what went wrong so the UI can show it. */
    data class Failed(val reason: FailureReason) : ConnectionState

    /** Whether media may be sent in this state. */
    val isConnected: Boolean get() = this is Connected

    /** Whether a connection attempt is in flight. */
    val isBusy: Boolean get() = this is Connecting || this is Reconnecting

    /**
     * The active session identifier, if any.
     *
     * Named distinctly from [Connected.sessionId] because a supertype property may not share a
     * name with a subtype's constructor property.
     */
    val activeSessionId: Long? get() = (this as? Connected)?.sessionId

    /** Peer name to display, if a peer is known in this state. */
    val peerDisplayName: String?
        get() = when (this) {
            is Connecting -> peerName
            is Connected -> peerName
            is Reconnecting -> peerName
            Idle, Discovering, is Failed -> null
        }
}

/**
 * Reconnection backoff schedule, in milliseconds.
 *
 * Five attempts. Beyond this, a phone that has genuinely left the network is just burning
 * battery, so we stop and let the user decide.
 */
val RECONNECT_BACKOFF_MS: LongArray = longArrayOf(500, 1_000, 2_000, 4_000, 8_000)

/** Number of reconnection attempts before giving up. */
const val MAX_RECONNECT_ATTEMPTS: Int = 5

/**
 * Backoff delay before a given 1-based attempt, in milliseconds.
 *
 * Returns null once the attempts are exhausted.
 */
fun backoffForAttempt(attempt: Int): Long? =
    RECONNECT_BACKOFF_MS.getOrNull(attempt - 1)

private fun LongArray.getOrNull(index: Int): Long? =
    if (index in indices) this[index] else null
