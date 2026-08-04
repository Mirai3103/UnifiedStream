package com.laffy.unifiedstream

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import com.laffy.unifiedstream.discovery.DiscoveryState
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
                    onBack = { backPressed = true },
                    onSettings = { settingsPressed = true },
                )
            }
        }

        compose.onNodeWithText("Camera").assertIsDisplayed()
        compose.onNodeWithContentDescription("Back").performClick()
        compose.onNodeWithContentDescription("Open settings").performClick()
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
}
