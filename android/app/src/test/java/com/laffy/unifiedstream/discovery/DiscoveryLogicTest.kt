package com.laffy.unifiedstream.discovery

import com.laffy.unifiedstream.protocol.PROTOCOL_VERSION
import com.laffy.unifiedstream.protocol.TxtKeys
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TxtRecordParsingTest {

    private fun attrs(
        id: String? = "3c6e0b8a-9c15-4f8d-b0a1-2d3e4f506172",
        ver: String? = "1",
        name: String? = "cachy-desktop",
        caps: String? = "cam,mic,spk",
    ): Map<String, ByteArray?> = buildMap {
        id?.let { put(TxtKeys.ID, it.toByteArray()) }
        ver?.let { put(TxtKeys.VERSION, it.toByteArray()) }
        name?.let { put(TxtKeys.NAME, it.toByteArray()) }
        caps?.let { put(TxtKeys.CAPS, it.toByteArray()) }
    }

    @Test
    fun completeRecordShouldParseEveryField() {
        val device = parseTxtRecord(attrs(), "cachy-desktop", "192.168.1.10", 47810)
        assertNotNull(device)
        requireNotNull(device)
        assertEquals("3c6e0b8a-9c15-4f8d-b0a1-2d3e4f506172", device.id)
        assertEquals("cachy-desktop", device.name)
        assertEquals("192.168.1.10", device.host)
        assertEquals(47810, device.port)
        assertEquals(PROTOCOL_VERSION, device.protocolVersion)
        assertEquals(listOf("cam", "mic", "spk"), device.caps)
    }

    @Test
    fun recordWithoutAnIdShouldBeIgnored() {
        assertNull(parseTxtRecord(attrs(id = null), "cachy-desktop", "192.168.1.10", 47810))
    }

    @Test
    fun recordWithoutAVersionShouldBeIgnored() {
        assertNull(parseTxtRecord(attrs(ver = null), "cachy-desktop", "192.168.1.10", 47810))
    }

    @Test
    fun recordWithANonNumericVersionShouldBeIgnored() {
        assertNull(parseTxtRecord(attrs(ver = "one"), "cachy-desktop", "192.168.1.10", 47810))
    }

    @Test
    fun recordWithoutANameShouldFallBackToTheInstanceName() {
        val device = parseTxtRecord(attrs(name = null), "fallback-name", "192.168.1.10", 47810)
        assertEquals("fallback-name", device?.name)
    }

    @Test
    fun recordWithNoCapabilitiesShouldParseWithAnEmptyList() {
        val device = parseTxtRecord(attrs(caps = ""), "cachy-desktop", "192.168.1.10", 47810)
        assertEquals(emptyList<String>(), device?.caps)
    }

    @Test
    fun recordWithAnInvalidPortShouldBeIgnored() {
        assertNull(parseTxtRecord(attrs(), "cachy-desktop", "192.168.1.10", 0))
        assertNull(parseTxtRecord(attrs(), "cachy-desktop", "192.168.1.10", 70000))
    }

    @Test
    fun futureProtocolVersionShouldParseButBeMarkedIncompatible() {
        val device = parseTxtRecord(attrs(ver = "2"), "cachy-desktop", "192.168.1.10", 47810)
        assertNotNull(device)
        assertTrue(device?.isCompatible == false)
    }

    @Test
    fun supportsShouldReflectAdvertisedCapabilities() {
        val device = parseTxtRecord(attrs(caps = "cam,spk"), "d", "192.168.1.10", 47810)
        requireNotNull(device)
        assertTrue(device.supports("cam"))
        assertTrue(!device.supports("mic"))
    }
}

class DeduplicationTest {

    private fun device(
        id: String = "device-a",
        host: String = "192.168.1.10",
        instanceName: String = "cachy-desktop",
        manual: Boolean = false,
    ) = DiscoveredDevice(
        id = id,
        name = "cachy-desktop",
        host = host,
        port = 47810,
        protocolVersion = PROTOCOL_VERSION,
        caps = listOf("cam"),
        instanceName = instanceName,
        manual = manual,
    )

    @Test
    fun aNewDeviceShouldBeAppended() {
        val result = mergeDiscovered(emptyList(), device())
        assertEquals(1, result.size)
    }

    @Test
    fun theSameDeviceOnTwoInterfacesShouldCollapseToOneEntry() {
        val first = mergeDiscovered(emptyList(), device(host = "192.168.1.10"))
        val second = mergeDiscovered(first, device(host = "192.168.1.11"))
        assertEquals(1, second.size)
    }

    @Test
    fun anIpv4AddressShouldWinOverIpv6ForTheSameDevice() {
        val withIpv6 = mergeDiscovered(emptyList(), device(host = "fe80::1"))
        val merged = mergeDiscovered(withIpv6, device(host = "192.168.1.10"))
        assertEquals(1, merged.size)
        assertEquals("192.168.1.10", merged.first().host)
    }

    @Test
    fun anIpv6AddressShouldNotDisplaceAnExistingIpv4() {
        val withIpv4 = mergeDiscovered(emptyList(), device(host = "192.168.1.10"))
        val merged = mergeDiscovered(withIpv4, device(host = "fe80::1"))
        assertEquals("192.168.1.10", merged.first().host)
    }

    @Test
    fun distinctDevicesShouldBothBeKept() {
        val first = mergeDiscovered(emptyList(), device(id = "device-a"))
        val second = mergeDiscovered(first, device(id = "device-b"))
        assertEquals(2, second.size)
    }

    @Test
    fun discoveryShouldNotOverrideAManualEntry() {
        val manual = mergeDiscovered(emptyList(), device(host = "10.0.0.5", manual = true))
        val merged = mergeDiscovered(manual, device(host = "192.168.1.10"))
        assertEquals("10.0.0.5", merged.first().host)
    }

    @Test
    fun serviceLostShouldRemoveTheMatchingEntry() {
        val devices = listOf(device(id = "a", instanceName = "one"), device(id = "b", instanceName = "two"))
        val remaining = removeByInstanceName(devices, "one")
        assertEquals(1, remaining.size)
        assertEquals("b", remaining.first().id)
    }

    @Test
    fun serviceLostShouldNotRemoveAManualEntry() {
        val devices = listOf(device(id = "a", instanceName = "one", manual = true))
        assertEquals(1, removeByInstanceName(devices, "one").size)
    }

    @Test
    fun serviceLostForAnUnknownInstanceShouldChangeNothing() {
        val devices = listOf(device(id = "a", instanceName = "one"))
        assertEquals(devices, removeByInstanceName(devices, "other"))
    }
}

class ManualAddressTest {

    @Test
    fun aDottedQuadWithAValidPortShouldBeAccepted() {
        val result = ManualAddress.validate("192.168.1.10", "47810")
        assertEquals(ManualAddress.Validation.Valid("192.168.1.10", 47810), result)
    }

    @Test
    fun surroundingWhitespaceShouldBeTrimmed() {
        val result = ManualAddress.validate("  192.168.1.10 ", " 47810 ")
        assertEquals(ManualAddress.Validation.Valid("192.168.1.10", 47810), result)
    }

    @Test
    fun anEmptyHostShouldBeRejected() {
        assertEquals(
            ManualAddress.Validation.Invalid(ManualAddress.Error.EMPTY_HOST),
            ManualAddress.validate("", "47810"),
        )
    }

    @Test
    fun anOctetAboveTwoFiftyFiveShouldBeRejected() {
        assertEquals(
            ManualAddress.Validation.Invalid(ManualAddress.Error.MALFORMED_HOST),
            ManualAddress.validate("192.168.1.999", "47810"),
        )
    }

    @Test
    fun aTruncatedAddressShouldBeRejected() {
        assertEquals(
            ManualAddress.Validation.Invalid(ManualAddress.Error.MALFORMED_HOST),
            ManualAddress.validate("192.168.1", "47810"),
        )
    }

    @Test
    fun aNonNumericPortShouldBeRejected() {
        assertEquals(
            ManualAddress.Validation.Invalid(ManualAddress.Error.MALFORMED_PORT),
            ManualAddress.validate("192.168.1.10", "http"),
        )
    }

    @Test
    fun portZeroShouldBeRejected() {
        assertEquals(
            ManualAddress.Validation.Invalid(ManualAddress.Error.PORT_OUT_OF_RANGE),
            ManualAddress.validate("192.168.1.10", "0"),
        )
    }

    @Test
    fun aPortAboveTheSixteenBitRangeShouldBeRejected() {
        assertEquals(
            ManualAddress.Validation.Invalid(ManualAddress.Error.PORT_OUT_OF_RANGE),
            ManualAddress.validate("192.168.1.10", "65536"),
        )
    }

    @Test
    fun theBoundaryPortsShouldBeAccepted() {
        assertTrue(ManualAddress.validate("192.168.1.10", "1") is ManualAddress.Validation.Valid)
        assertTrue(ManualAddress.validate("192.168.1.10", "65535") is ManualAddress.Validation.Valid)
    }

    @Test
    fun aHostnameShouldBeAccepted() {
        assertTrue(
            ManualAddress.validate("cachy-desktop.local", "47810") is ManualAddress.Validation.Valid,
        )
    }

    @Test
    fun anIpv6LiteralShouldBeAccepted() {
        assertTrue(ManualAddress.validate("fe80::1", "47810") is ManualAddress.Validation.Valid)
    }

    @Test
    fun aScopedIpv6LiteralShouldBeRejected() {
        assertEquals(
            ManualAddress.Validation.Invalid(ManualAddress.Error.MALFORMED_HOST),
            ManualAddress.validate("fe80::1%wlan0", "47810"),
        )
    }

    @Test
    fun aHostnameWithALeadingHyphenShouldBeRejected() {
        assertEquals(
            ManualAddress.Validation.Invalid(ManualAddress.Error.MALFORMED_HOST),
            ManualAddress.validate("-bad.local", "47810"),
        )
    }

    @Test
    fun aManualDeviceShouldBeMarkedManualAndClaimNoCapabilities() {
        val device = ManualAddress.toDevice("192.168.1.10", 47810)
        assertTrue(device.manual)
        assertEquals(emptyList<String>(), device.caps)
        assertEquals("192.168.1.10", device.host)
    }

    @Test
    fun theDiscoveryTimeoutShouldBeTenSeconds() {
        assertEquals(10_000L, ManualAddress.DISCOVERY_TIMEOUT_MS)
    }
}
