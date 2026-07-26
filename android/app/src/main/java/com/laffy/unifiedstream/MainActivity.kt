package com.laffy.unifiedstream

import android.Manifest
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.laffy.unifiedstream.session.CameraStreamState
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.session.SessionService
import com.laffy.unifiedstream.ui.CameraControls
import com.laffy.unifiedstream.ui.DeviceListScreen
import com.laffy.unifiedstream.ui.MicControls
import com.laffy.unifiedstream.ui.SessionScreen
import com.laffy.unifiedstream.ui.SpeakerControls
import com.laffy.unifiedstream.ui.UnifiedStreamViewModel
import com.laffy.unifiedstream.ui.theme.UnifiedStreamTheme
import com.laffy.unifiedstream.video.CameraFacing

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            UnifiedStreamTheme {
                Scaffold(modifier = Modifier.fillMaxSize()) { padding ->
                    UnifiedStreamApp(Modifier.padding(padding))
                }
            }
        }
    }
}

/** Which screen is showing. Two destinations do not justify a navigation library. */
private enum class Screen { Devices, Session }

@Composable
fun UnifiedStreamApp(
    modifier: Modifier = Modifier,
    viewModel: UnifiedStreamViewModel = viewModel(),
) {
    val context = LocalContext.current

    val devices by viewModel.devices.collectAsStateWithLifecycle()
    val discoveryState by viewModel.discoveryState.collectAsStateWithLifecycle()
    val offerManual by viewModel.offerManualEntry.collectAsStateWithLifecycle()
    val manualError by viewModel.manualError.collectAsStateWithLifecycle()
    val connection by viewModel.connection.collectAsStateWithLifecycle()
    val dashboard by viewModel.dashboard.collectAsStateWithLifecycle()
    val testReport by viewModel.testReport.collectAsStateWithLifecycle()
    val testRunning by viewModel.testRunning.collectAsStateWithLifecycle()
    val micState by viewModel.micState.collectAsStateWithLifecycle()
    val micLevel by viewModel.micLevel.collectAsStateWithLifecycle()
    val micMuted by viewModel.micMuted.collectAsStateWithLifecycle()
    val micGain by viewModel.micGain.collectAsStateWithLifecycle()
    val micNoiseSuppression by viewModel.micNoiseSuppression.collectAsStateWithLifecycle()
    val micPermissionNeeded by viewModel.micPermissionNeeded.collectAsStateWithLifecycle()
    val cameraState by viewModel.cameraState.collectAsStateWithLifecycle()
    val cameraFacing by viewModel.cameraFacing.collectAsStateWithLifecycle()
    val cameraResolution by viewModel.cameraResolution.collectAsStateWithLifecycle()
    val cameraPermissionNeeded by viewModel.cameraPermissionNeeded.collectAsStateWithLifecycle()
    val speakerState by viewModel.speakerState.collectAsStateWithLifecycle()
    val speakerLevel by viewModel.speakerLevel.collectAsStateWithLifecycle()
    val speakerMuted by viewModel.speakerMuted.collectAsStateWithLifecycle()
    val speakerVolume by viewModel.speakerVolume.collectAsStateWithLifecycle()

    var screen by remember { mutableStateOf(Screen.Devices) }

    val notificationPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { /* The session runs either way; without it there is simply no notification. */ }

    val micPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) viewModel.enableMic() else viewModel.onMicPermissionDenied()
    }

    val cameraPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) viewModel.enableCamera() else viewModel.onCameraPermissionDenied()
    }

    LaunchedEffect(Unit) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    // Browse only while the device list is showing; mDNS and its multicast lock are not free.
    DisposableEffect(screen) {
        if (screen == Screen.Devices) viewModel.startDiscovery()
        onDispose { if (screen == Screen.Devices) viewModel.stopDiscovery() }
    }

    // The foreground service is what keeps the session alive once the user leaves the app.
    // Re-started when the camera stream turns on so the service can pick up the camera
    // foreground type, which the platform requires for capture to survive backgrounding.
    LaunchedEffect(connection, cameraState is CameraStreamState.Active) {
        when (val state = connection) {
            is ConnectionState.Connected -> {
                screen = Screen.Session
                SessionService.start(context, state.peerName)
            }

            is ConnectionState.Connecting, is ConnectionState.Reconnecting ->
                screen = Screen.Session

            is ConnectionState.Failed -> SessionService.stop(context)

            ConnectionState.Idle -> {
                SessionService.stop(context)
                screen = Screen.Devices
            }

            ConnectionState.Discovering -> SessionService.stop(context)
        }
    }

    when (screen) {
        Screen.Devices -> DeviceListScreen(
            devices = devices,
            discoveryState = discoveryState,
            offerManual = offerManual,
            manualError = manualError,
            onSelect = viewModel::connect,
            onAddManual = { host, port -> viewModel.addManualDevice(host, port) },
            onManualEdited = viewModel::clearManualError,
            modifier = modifier,
        )

        Screen.Session -> SessionScreen(
            connection = connection,
            dashboard = dashboard,
            testReport = testReport,
            testRunning = testRunning,
            mic = MicControls(
                state = micState,
                level = micLevel,
                muted = micMuted,
                gain = micGain,
                noiseSuppression = micNoiseSuppression,
                noiseSuppressionAvailable = viewModel.micNoiseSuppressionAvailable,
                permissionNeeded = micPermissionNeeded,
                onToggle = { enable ->
                    if (!enable) {
                        viewModel.disableMic()
                    } else if (viewModel.hasRecordPermission()) {
                        viewModel.enableMic()
                    } else {
                        micPermission.launch(Manifest.permission.RECORD_AUDIO)
                    }
                },
                onMuteToggle = viewModel::setMicMuted,
                onGainChange = viewModel::setMicGain,
                onNoiseSuppressionToggle = viewModel::setMicNoiseSuppression,
            ),
            camera = CameraControls(
                state = cameraState,
                facing = cameraFacing,
                resolution = cameraResolution,
                permissionNeeded = cameraPermissionNeeded,
                onToggle = { enable ->
                    if (!enable) {
                        viewModel.disableCamera()
                    } else if (viewModel.hasCameraPermission()) {
                        viewModel.enableCamera()
                    } else {
                        cameraPermission.launch(Manifest.permission.CAMERA)
                    }
                },
                onFacingToggle = {
                    viewModel.setCameraFacing(
                        if (cameraFacing == CameraFacing.BACK) CameraFacing.FRONT else CameraFacing.BACK,
                    )
                },
                onResolutionSelect = viewModel::setCameraResolution,
                onPreviewSurface = viewModel::setCameraPreview,
            ),
            speaker = SpeakerControls(
                state = speakerState,
                level = speakerLevel,
                muted = speakerMuted,
                volume = speakerVolume,
                onToggle = { enable ->
                    if (enable) viewModel.enableSpeaker() else viewModel.disableSpeaker()
                },
                onMuteToggle = viewModel::setSpeakerMuted,
                onVolumeChange = viewModel::setSpeakerVolume,
            ),
            onStartTestStream = viewModel::startTestStream,
            onStopTestStream = viewModel::stopTestStream,
            onDisconnect = viewModel::disconnect,
            onCancelReconnect = viewModel::cancelReconnect,
            onBack = { screen = Screen.Devices },
            modifier = modifier,
        )
    }
}
