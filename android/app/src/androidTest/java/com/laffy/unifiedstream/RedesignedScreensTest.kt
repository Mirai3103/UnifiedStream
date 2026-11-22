package com.laffy.unifiedstream

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import com.laffy.unifiedstream.discovery.DiscoveredDevice
import com.laffy.unifiedstream.discovery.DiscoveryState
import com.laffy.unifiedstream.protocol.PROTOCOL_VERSION
import com.laffy.unifiedstream.session.FailureReason
import com.laffy.unifiedstream.video.CameraResolution
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.ui.CameraControls
import com.laffy.unifiedstream.ui.CameraDestination
import com.laffy.unifiedstream.ui.DashboardState
import com.laffy.unifiedstream.ui.DevicesDestination
import com.laffy.unifiedstream.ui.HomeDestination
import com.laffy.unifiedstream.ui.MicControls
import com.laffy.unifiedstream.ui.SettingsDestination
import com.laffy.unifiedstream.ui.SpeakerControls
import com.laffy.unifiedstream.ui.theme.UnifiedStreamTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class RedesignedScreensTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun titleBar_exposesBackAndSettingsActions() {
        var backPressed = false
        var settingsPressed = false
        compose.setContent {
            UnifiedStreamTheme {
                UnifiedTopBar(
                    destination = AppDestination.CAMERA,
                    subtitle = "Connected to Studio PC",
                    onBack = { backPressed = true },
                    onSettings = { settingsPressed = true },
                )
            }
        }

        // The large flexible bar lays the title out in both its expanded and collapsed form.
        compose.onAllNodesWithText("Camera").onFirst().assertIsDisplayed()
        compose.onAllNodesWithText("Connected to Studio PC").onFirst().assertIsDisplayed()
        compose.onNodeWithContentDescription("Back")
            .assertWidthIsAtLeast(48.dp)
            .assertHeightIsAtLeast(48.dp)
            .performClick()
        compose.onNodeWithContentDescription("Open settings")
            .assertWidthIsAtLeast(48.dp)
            .assertHeightIsAtLeast(48.dp)
            .performClick()
        compose.runOnIdle {
            assertEquals(true, backPressed)
            assertEquals(true, settingsPressed)
        }
    }

    @Test
    fun home_exposesAllThreeStreamsAndConnectionRecovery() {
        compose.setContent {
            UnifiedStreamTheme {
                HomeDestination(
                    connection = ConnectionState.Idle,
                    dashboard = DashboardState(),
                    mic = MicControls(),
                    camera = CameraControls(),
                    speaker = SpeakerControls(),
                    onOpenDevices = {},
                    onOpenCamera = {},
                )
            }
        }

        compose.onNodeWithText("Camera").assertIsDisplayed()
        compose.onNodeWithText("Microphone").assertIsDisplayed()
        compose.onNodeWithText("Speaker").assertIsDisplayed()
        compose.onNodeWithText("Find a computer").assertIsDisplayed()
    }

    @Test
    fun camera_exposesPreviewSettingsAndPermissionState() {
        compose.setContent {
            UnifiedStreamTheme {
                CameraDestination(
                    connection = ConnectionState.Idle,
                    camera = CameraControls(permissionNeeded = true),
                )
            }
        }

        compose.onNodeWithText("Camera permission is required.").assertIsDisplayed()
        compose.onNodeWithText("720p").assertIsDisplayed()
    }

    @Test
    fun devices_exposesTimeoutAndManualFallback() {
        compose.setContent {
            UnifiedStreamTheme {
                DevicesDestination(
                    devices = emptyList(),
                    discoveryState = DiscoveryState.TimedOut,
                    offerManual = true,
                    manualError = null,
                    connection = ConnectionState.Idle,
                    onSelect = {},
                    onAddManual = { _, _ -> },
                    onManualEdited = {},
                )
            }
        }

        compose.onNodeWithText("No computer answered discovery. You can connect by address below.").assertIsDisplayed()
        compose.onNodeWithText("Connect by address").assertIsDisplayed()
    }

    @Test
    fun settings_exposesThemeAndDiagnostics() {
        compose.setContent {
            UnifiedStreamTheme {
                SettingsDestination(
                    identity = null,
                    connection = ConnectionState.Idle,
                    darkTheme = true,
                    testReport = null,
                    testRunning = false,
                    onThemeChange = {},
                    onStartTestStream = { _, _ -> },
                    onStopTestStream = {},
                    onDisconnect = {},
                    onCancelReconnect = {},
                )
            }
        }

        compose.onNodeWithContentDescription("Dark theme").assertIsDisplayed()
        compose.onNodeWithText("Transport diagnostics").assertIsDisplayed()
    }
    @Test
    fun home_streamToggles_invokeTheirCallbacksWhenConnected() {
        var camera: Boolean? = null
        var mic: Boolean? = null
        var speaker: Boolean? = null
        compose.setContent {
            UnifiedStreamTheme {
                HomeDestination(
                    connection = ConnectionState.Connected(peerName = "Studio PC", sessionId = 1L),
                    dashboard = DashboardState(),
                    mic = MicControls(onToggle = { mic = it }),
                    camera = CameraControls(onToggle = { camera = it }),
                    speaker = SpeakerControls(onToggle = { speaker = it }),
                    onOpenDevices = {},
                    onOpenCamera = {},
                )
            }
        }

        compose.onNodeWithContentDescription("Camera stream").performClick()
        compose.onNodeWithContentDescription("Microphone stream").performClick()
        compose.onNodeWithContentDescription("Speaker stream").performClick()
        compose.runOnIdle {
            assertEquals(true, camera)
            assertEquals(true, mic)
            assertEquals(true, speaker)
        }
    }

    @Test
    fun home_failedConnection_showsReasonAndRecoveryAction() {
        compose.setContent {
            UnifiedStreamTheme {
                HomeDestination(
                    connection = ConnectionState.Failed(FailureReason.Rejected),
                    dashboard = DashboardState(),
                    mic = MicControls(),
                    camera = CameraControls(),
                    speaker = SpeakerControls(),
                    onOpenDevices = {},
                    onOpenCamera = {},
                )
            }
        }

        compose.onNodeWithText("Connection failed").assertIsDisplayed()
        compose.onNodeWithText("Pairing was declined").assertIsDisplayed()
        compose.onNodeWithText("Find a computer").assertIsDisplayed()
    }

    @Test
    fun camera_resolutionChoice_reportsSelection() {
        var selected: CameraResolution? = null
        compose.setContent {
            UnifiedStreamTheme {
                CameraDestination(
                    connection = ConnectionState.Connected(peerName = "Studio PC", sessionId = 1L),
                    camera = CameraControls(onResolutionSelect = { selected = it }),
                )
            }
        }

        compose.onNodeWithText("1080p").performClick()
        compose.runOnIdle { assertEquals(CameraResolution.FHD, selected) }
    }

    @Test
    fun devices_incompatibleDevice_isLabelledAndNotConnectable() {
        var selected: DiscoveredDevice? = null
        val device = DiscoveredDevice(
            id = "old",
            name = "Old PC",
            host = "192.168.1.5",
            port = 47810,
            protocolVersion = PROTOCOL_VERSION + 1,
            caps = emptyList(),
            instanceName = "old",
        )
        compose.setContent {
            UnifiedStreamTheme {
                DevicesDestination(
                    devices = listOf(device),
                    discoveryState = DiscoveryState.Found,
                    offerManual = false,
                    manualError = null,
                    connection = ConnectionState.Idle,
                    onSelect = { selected = it },
                    onAddManual = { _, _ -> },
                    onManualEdited = {},
                )
            }
        }

        compose.onNodeWithText("Incompatible protocol v${PROTOCOL_VERSION + 1}").assertIsDisplayed()
        compose.onNodeWithContentDescription("Connect to Old PC").performClick()
        compose.runOnIdle { assertEquals(null, selected) }
    }
}
