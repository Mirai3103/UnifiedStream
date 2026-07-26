package com.laffy.unifiedstream.discovery

import com.laffy.unifiedstream.protocol.PROTOCOL_VERSION
import com.laffy.unifiedstream.protocol.TxtKeys

/**
 * A desktop found on the local network, ready to be connected to.
 *
 * [id] is the desktop's stable device UUID from the TXT record, and is the identity used for
 * deduplication — a machine on both Ethernet and Wi-Fi advertises twice but is one device.
 */
data class DiscoveredDevice(
    val id: String,
    val name: String,
    val host: String,
    val port: Int,
    val protocolVersion: Int,
    val caps: List<String>,
    /** mDNS instance name, used to correlate a later service-lost event back to this entry. */
    val instanceName: String,
    /** True when this peer was typed in by hand rather than discovered. */
    val manual: Boolean = false,
) {
    /** Whether this build can talk to this device at all. */
    val isCompatible: Boolean get() = protocolVersion == PROTOCOL_VERSION

    /** Whether the device advertises support for a given capability token. */
    fun supports(cap: String): Boolean = cap in caps
}

/**
 * Parse the four required TXT keys.
 *
 * Returns null when any required key is missing or malformed — a record we cannot identify is
 * one we cannot deduplicate, so it is dropped rather than shown as a second phantom device.
 */
fun parseTxtRecord(
    attributes: Map<String, ByteArray?>,
    instanceName: String,
    host: String,
    port: Int,
): DiscoveredDevice? {
    fun value(key: String): String? =
        attributes[key]?.let { runCatching { String(it, Charsets.UTF_8) }.getOrNull() }

    val id = value(TxtKeys.ID)?.takeIf { it.isNotBlank() } ?: return null
    val version = value(TxtKeys.VERSION)?.trim()?.toIntOrNull() ?: return null
    val name = value(TxtKeys.NAME)?.takeIf { it.isNotBlank() } ?: instanceName
    val caps = value(TxtKeys.CAPS)
        ?.split(',')
        ?.map { it.trim() }
        ?.filter { it.isNotEmpty() }
        ?: emptyList()

    if (port !in 1..65535) return null

    return DiscoveredDevice(
        id = id,
        name = name,
        host = host,
        port = port,
        protocolVersion = version,
        caps = caps,
        instanceName = instanceName,
    )
}

/**
 * Fold a newly resolved device into the list, collapsing duplicates by [DiscoveredDevice.id].
 *
 * When the same desktop resolves on several interfaces, an IPv4 address wins: link-local IPv6
 * needs a scope id that a plain socket connect will not have.
 */
fun mergeDiscovered(
    current: List<DiscoveredDevice>,
    incoming: DiscoveredDevice,
): List<DiscoveredDevice> {
    val existing = current.firstOrNull { it.id == incoming.id }
        ?: return current + incoming

    val replace = when {
        // A hand-typed entry is the user's explicit intent; discovery does not override it.
        existing.manual -> false
        isIpv4(existing.host) && !isIpv4(incoming.host) -> false
        !isIpv4(existing.host) && isIpv4(incoming.host) -> true
        else -> existing != incoming
    }

    return if (replace) current.map { if (it.id == incoming.id) incoming else it } else current
}

/** Drop every entry advertised under [instanceName], for an mDNS service-lost event. */
fun removeByInstanceName(
    current: List<DiscoveredDevice>,
    instanceName: String,
): List<DiscoveredDevice> = current.filterNot { it.instanceName == instanceName && !it.manual }

private fun isIpv4(host: String): Boolean =
    host.count { it == '.' } == 3 && !host.contains(':')
