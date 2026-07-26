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
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.session.SessionService
import com.laffy.unifiedstream.ui.DeviceListScreen
import com.laffy.unifiedstream.ui.SessionScreen
import com.laffy.unifiedstream.ui.UnifiedStreamViewModel
import com.laffy.unifiedstream.ui.theme.UnifiedStreamTheme

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

    var screen by remember { mutableStateOf(Screen.Devices) }

    val notificationPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { /* The session runs either way; without it there is simply no notification. */ }

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
    LaunchedEffect(connection) {
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
            onStartTestStream = viewModel::startTestStream,
            onStopTestStream = viewModel::stopTestStream,
            onDisconnect = viewModel::disconnect,
            onCancelReconnect = viewModel::cancelReconnect,
            onBack = { screen = Screen.Devices },
            modifier = modifier,
        )
    }
}
