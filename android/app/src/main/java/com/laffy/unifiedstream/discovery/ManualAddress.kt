package com.laffy.unifiedstream.discovery

import com.laffy.unifiedstream.protocol.PROTOCOL_VERSION

/**
 * Manual peer entry, the fallback when mDNS is unavailable.
 *
 * AP client isolation and multicast filtering are common enough on guest and mesh networks
 * that discovery-only would strand real users, so this is a first-class path rather than a
 * debug affordance.
 */
object ManualAddress {

    /** How long discovery may find nothing before the UI offers manual entry. */
    const val DISCOVERY_TIMEOUT_MS: Long = 10_000

    /** Outcome of validating what the user typed. */
    sealed interface Validation {
        data class Valid(val host: String, val port: Int) : Validation
        data class Invalid(val error: Error) : Validation
    }

    /** Why an entered address was rejected. Rendered by the UI as a field error. */
    enum class Error {
        EMPTY_HOST,
        MALFORMED_HOST,
        MALFORMED_PORT,
        PORT_OUT_OF_RANGE,
    }

    /**
     * Validate a host and port typed by the user.
     *
     * Accepts dotted-quad IPv4, bracketed or bare IPv6, and hostnames. Rejects anything that
     * would fail at connect time, so the error surfaces in the form rather than as a timeout.
     */
    fun validate(hostInput: String, portInput: String): Validation {
        val host = hostInput.trim()
        if (host.isEmpty()) return Validation.Invalid(Error.EMPTY_HOST)
        if (!isPlausibleHost(host)) return Validation.Invalid(Error.MALFORMED_HOST)

        val port = portInput.trim().toIntOrNull()
            ?: return Validation.Invalid(Error.MALFORMED_PORT)
        if (port !in 1..65535) return Validation.Invalid(Error.PORT_OUT_OF_RANGE)

        return Validation.Valid(host, port)
    }

    /**
     * Build a device entry for a validated manual address.
     *
     * Capabilities are unknown until the handshake, so none are claimed here; the UI shows
     * what the peer reports once connected.
     */
    fun toDevice(host: String, port: Int): DiscoveredDevice = DiscoveredDevice(
        id = "manual:$host:$port",
        name = host,
        host = host,
        port = port,
        protocolVersion = PROTOCOL_VERSION,
        caps = emptyList(),
        instanceName = "manual:$host:$port",
        manual = true,
    )

    private fun isPlausibleHost(host: String): Boolean = when {
        host.startsWith("[") -> host.endsWith("]") && isIpv6(host.substring(1, host.length - 1))
        host.contains(':') -> isIpv6(host)
        host.all { it.isDigit() || it == '.' } -> isIpv4(host)
        else -> isHostname(host)
    }

    private fun isIpv4(host: String): Boolean {
        val parts = host.split('.')
        if (parts.size != 4) return false
        return parts.all { part ->
            part.isNotEmpty() &&
                part.length <= 3 &&
                part.all(Char::isDigit) &&
                (part.toIntOrNull() ?: return@all false) in 0..255
        }
    }

    private fun isIpv6(host: String): Boolean {
        if (host.isEmpty() || !host.contains(':')) return false
        // A zone index (fe80::1%wlan0) is legal but not useful for a typed peer address.
        if (host.contains('%')) return false
        if (host.count { it == ':' } > 7) return false
        if (host.contains(":::")) return false
        val groups = host.split(':')
        if (groups.count { it.isEmpty() } > 2) return false
        return groups.all { group ->
            group.isEmpty() || (group.length <= 4 && group.all { it.isDigit() || it in "abcdefABCDEF" })
        }
    }

    private fun isHostname(host: String): Boolean {
        if (host.length > 253) return false
        val labels = host.trimEnd('.').split('.')
        return labels.isNotEmpty() && labels.all { label ->
            label.isNotEmpty() &&
                label.length <= 63 &&
                !label.startsWith('-') &&
                !label.endsWith('-') &&
                label.all { it.isLetterOrDigit() || it == '-' }
        }
    }
}
