package com.laffy.unifiedstream

import android.Manifest
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.laffy.unifiedstream.session.CameraStreamState
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.session.SessionService
import com.laffy.unifiedstream.ui.CameraControls
import com.laffy.unifiedstream.ui.CameraDestination
import com.laffy.unifiedstream.ui.DevicesDestination
import com.laffy.unifiedstream.ui.HomeDestination
import com.laffy.unifiedstream.ui.MicControls
import com.laffy.unifiedstream.ui.SettingsDestination
import com.laffy.unifiedstream.ui.SpeakerControls
import com.laffy.unifiedstream.ui.UnifiedStreamViewModel
import com.laffy.unifiedstream.ui.theme.UnifiedStreamTheme
import com.laffy.unifiedstream.video.CameraFacing

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            var darkTheme by rememberSaveable { mutableStateOf(true) }
            UnifiedStreamTheme(darkTheme = darkTheme) {
                UnifiedStreamApp(darkTheme = darkTheme, onThemeChange = { darkTheme = it })
            }
        }
    }
}

enum class AppDestination(val label: String) {
    HOME("Home"),
    CAMERA("Camera"),
    DEVICES("Devices"),
    SETTINGS("Settings"),
}

fun pushDestination(stack: List<String>, destination: AppDestination): List<String> =
    if (stack.lastOrNull() == destination.name) stack else stack + destination.name

fun popDestination(stack: List<String>): List<String> =
    if (stack.size > 1) stack.dropLast(1) else stack

@Composable
fun UnifiedStreamApp(
    darkTheme: Boolean,
    onThemeChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    viewModel: UnifiedStreamViewModel = viewModel(),
) {
    val context = LocalContext.current
    val identity by viewModel.identity.collectAsStateWithLifecycle()
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

    var backStack by rememberSaveable { mutableStateOf(listOf(AppDestination.DEVICES.name)) }
    val destination = AppDestination.valueOf(backStack.last())

    fun navigate(target: AppDestination) {
        backStack = pushDestination(backStack, target)
    }

    fun navigateBack() {
        if (backStack.size > 1) backStack = popDestination(backStack) else (context as? ComponentActivity)?.finish()
    }

    BackHandler { navigateBack() }

    val notificationPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    val micPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) viewModel.enableMic() else viewModel.onMicPermissionDenied()
    }
    val cameraPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) viewModel.enableCamera() else viewModel.onCameraPermissionDenied()
    }

    LaunchedEffect(Unit) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    DisposableEffect(destination) {
        if (destination == AppDestination.DEVICES) viewModel.startDiscovery()
        onDispose { if (destination == AppDestination.DEVICES) viewModel.stopDiscovery() }
    }

    LaunchedEffect(connection, cameraState is CameraStreamState.Active) {
        when (val state = connection) {
            is ConnectionState.Connected -> {
                if (destination == AppDestination.DEVICES) navigate(AppDestination.HOME)
                SessionService.start(context, state.peerName)
            }
            is ConnectionState.Connecting, is ConnectionState.Reconnecting -> {
                if (destination == AppDestination.DEVICES) navigate(AppDestination.HOME)
            }
            is ConnectionState.Failed -> SessionService.stop(context)
            ConnectionState.Idle, ConnectionState.Discovering -> SessionService.stop(context)
        }
    }

    val mic = MicControls(
        state = micState,
        level = micLevel,
        muted = micMuted,
        gain = micGain,
        noiseSuppression = micNoiseSuppression,
        noiseSuppressionAvailable = viewModel.micNoiseSuppressionAvailable,
        permissionNeeded = micPermissionNeeded,
        onToggle = { enable ->
            if (!enable) viewModel.disableMic()
            else if (viewModel.hasRecordPermission()) viewModel.enableMic()
            else micPermission.launch(Manifest.permission.RECORD_AUDIO)
        },
        onMuteToggle = viewModel::setMicMuted,
        onGainChange = viewModel::setMicGain,
        onNoiseSuppressionToggle = viewModel::setMicNoiseSuppression,
    )
    val camera = CameraControls(
        state = cameraState,
        facing = cameraFacing,
        resolution = cameraResolution,
        permissionNeeded = cameraPermissionNeeded,
        onToggle = { enable ->
            if (!enable) viewModel.disableCamera()
            else if (viewModel.hasCameraPermission()) viewModel.enableCamera()
            else cameraPermission.launch(Manifest.permission.CAMERA)
        },
        onFacingToggle = {
            viewModel.setCameraFacing(if (cameraFacing == CameraFacing.BACK) CameraFacing.FRONT else CameraFacing.BACK)
        },
        onResolutionSelect = viewModel::setCameraResolution,
        onPreviewSurface = viewModel::setCameraPreview,
    )
    val speaker = SpeakerControls(
        state = speakerState,
        level = speakerLevel,
        muted = speakerMuted,
        volume = speakerVolume,
        onToggle = { if (it) viewModel.enableSpeaker() else viewModel.disableSpeaker() },
        onMuteToggle = viewModel::setSpeakerMuted,
        onVolumeChange = viewModel::setSpeakerVolume,
    )

    Scaffold(
        modifier = modifier,
        topBar = {
            androidx.compose.foundation.layout.Column {
                UnifiedTopBar(
                    destination = destination,
                    onBack = ::navigateBack,
                    onSettings = { navigate(AppDestination.SETTINGS) },
                )
                if (connection is ConnectionState.Failed) {
                    Surface(color = MaterialTheme.colorScheme.errorContainer) {
                        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp)) {
                            Text(
                                text = (connection as ConnectionState.Failed).reason.display,
                                color = MaterialTheme.colorScheme.onErrorContainer,
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                    }
                }
            }
        },
    ) { padding ->
        val contentModifier = Modifier.padding(padding)
        when (destination) {
            AppDestination.HOME -> HomeDestination(
                connection = connection,
                dashboard = dashboard,
                mic = mic,
                camera = camera,
                speaker = speaker,
                onOpenDevices = { navigate(AppDestination.DEVICES) },
                onOpenCamera = { navigate(AppDestination.CAMERA) },
                modifier = contentModifier,
            )
            AppDestination.CAMERA -> CameraDestination(connection, camera, contentModifier)
            AppDestination.DEVICES -> DevicesDestination(
                devices = devices,
                discoveryState = discoveryState,
                offerManual = offerManual,
                manualError = manualError,
                connection = connection,
                onSelect = viewModel::connect,
                onAddManual = { host, port -> viewModel.addManualDevice(host, port) },
                onManualEdited = viewModel::clearManualError,
                modifier = contentModifier,
            )
            AppDestination.SETTINGS -> SettingsDestination(
                identity = identity,
                connection = connection,
                darkTheme = darkTheme,
                testReport = testReport,
                testRunning = testRunning,
                onThemeChange = onThemeChange,
                onStartTestStream = viewModel::startTestStream,
                onStopTestStream = viewModel::stopTestStream,
                onDisconnect = viewModel::disconnect,
                onCancelReconnect = viewModel::cancelReconnect,
                modifier = contentModifier,
            )
        }
    }
}

@Composable
fun UnifiedTopBar(
    destination: AppDestination,
    onBack: () -> Unit = {},
    onSettings: () -> Unit = {},
) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, tonalElevation = 2.dp) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp),
            verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
        ) {
            TextButton(
                onClick = onBack,
                modifier = Modifier.semantics { contentDescription = "Back" },
            ) { Text("‹ Back") }
            Text(
                text = destination.label,
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.weight(1f).padding(horizontal = 8.dp),
            )
            TextButton(
                onClick = onSettings,
                enabled = destination != AppDestination.SETTINGS,
                modifier = Modifier.semantics { contentDescription = "Open settings" },
            ) { Text("Settings") }
        }
    }
}
