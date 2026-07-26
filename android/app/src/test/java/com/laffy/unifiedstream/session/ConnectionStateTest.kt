package com.laffy.unifiedstream.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectionStateTest {

    private fun connected() = ConnectionState.Connected(
        peerName = "cachy-desktop",
        sessionId = 42,
        negotiatedCaps = listOf("cam", "mic"),
        mediaPort = 47811,
    )

    @Test
    fun isConnectedShouldBeTrueOnlyWhenConnected() {
        assertTrue(connected().isConnected)
        assertTrue(!ConnectionState.Idle.isConnected)
        assertTrue(!ConnectionState.Discovering.isConnected)
        assertTrue(!ConnectionState.Connecting("x").isConnected)
    }

    @Test
    fun isBusyShouldCoverConnectingAndReconnecting() {
        assertTrue(ConnectionState.Connecting("x").isBusy)
        assertTrue(ConnectionState.Reconnecting("x", 1, MAX_RECONNECT_ATTEMPTS).isBusy)
        assertTrue(!connected().isBusy)
        assertTrue(!ConnectionState.Idle.isBusy)
    }

    @Test
    fun sessionIdShouldBeAvailableOnlyWhileConnected() {
        assertEquals(42L, connected().activeSessionId)
        assertNull(ConnectionState.Idle.activeSessionId)
        assertNull(ConnectionState.Reconnecting("x", 1, 5).activeSessionId)
    }

    @Test
    fun peerNameShouldBeExposedWhileReconnecting() {
        assertEquals(
            "cachy-desktop",
            ConnectionState.Reconnecting("cachy-desktop", 2, 5).peerDisplayName,
        )
    }

    @Test
    fun peerNameShouldBeAbsentWhenFailed() {
        assertNull(ConnectionState.Failed(FailureReason.Busy).peerDisplayName)
    }

    @Test
    fun peerNameShouldBeAbsentWhenIdleOrDiscovering() {
        assertNull(ConnectionState.Idle.peerDisplayName)
        assertNull(ConnectionState.Discovering.peerDisplayName)
    }

    @Test
    fun backoffShouldFollowTheDocumentedSchedule() {
        assertEquals(500L, backoffForAttempt(1))
        assertEquals(1_000L, backoffForAttempt(2))
        assertEquals(2_000L, backoffForAttempt(3))
        assertEquals(4_000L, backoffForAttempt(4))
        assertEquals(8_000L, backoffForAttempt(5))
    }

    @Test
    fun backoffShouldBeExhaustedPastTheLastAttempt() {
        assertNull(backoffForAttempt(MAX_RECONNECT_ATTEMPTS + 1))
    }

    @Test
    fun backoffShouldRejectAttemptZero() {
        assertNull(backoffForAttempt(0))
    }

    @Test
    fun theBackoffScheduleShouldMatchTheAttemptCount() {
        assertEquals(MAX_RECONNECT_ATTEMPTS, RECONNECT_BACKOFF_MS.size)
    }

    @Test
    fun backoffShouldIncreaseMonotonically() {
        for (i in 1 until RECONNECT_BACKOFF_MS.size) {
            assertTrue(
                "attempt ${i + 1} must wait longer than attempt $i",
                RECONNECT_BACKOFF_MS[i] > RECONNECT_BACKOFF_MS[i - 1],
            )
        }
    }

    @Test
    fun everyFailureReasonShouldRenderHumanReadableText() {
        val reasons = listOf(
            FailureReason.Unreachable("timeout"),
            FailureReason.VersionMismatch("v2"),
            FailureReason.Rejected,
            FailureReason.Busy,
            FailureReason.ReconnectExhausted,
            FailureReason.Other("something"),
        )
        for (reason in reasons) {
            assertTrue("$reason must have display text", reason.display.isNotBlank())
        }
    }

    @Test
    fun busyShouldExplainThatAnotherPhoneHoldsTheDevice() {
        assertEquals(
            "Device is already paired with another phone",
            FailureReason.Busy.display,
        )
    }
}
