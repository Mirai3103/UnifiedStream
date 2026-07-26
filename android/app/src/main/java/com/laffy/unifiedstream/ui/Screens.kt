package com.laffy.unifiedstream.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.laffy.unifiedstream.audio.GAIN_MAX
import com.laffy.unifiedstream.audio.GAIN_MIN
import com.laffy.unifiedstream.audio.AudioLevel
import com.laffy.unifiedstream.discovery.DiscoveredDevice
import com.laffy.unifiedstream.discovery.DiscoveryState
import com.laffy.unifiedstream.discovery.ManualAddress
import androidx.camera.core.Preview
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.viewinterop.AndroidView
import com.laffy.unifiedstream.protocol.MAX_PAYLOAD
import com.laffy.unifiedstream.session.CameraStreamState
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.session.MicStreamState
import com.laffy.unifiedstream.session.SpeakerStreamState
import com.laffy.unifiedstream.video.CameraFacing
import com.laffy.unifiedstream.video.CameraResolution
import com.laffy.unifiedstream.telemetry.LinkQuality
import com.laffy.unifiedstream.transport.TestStreamReport
import com.laffy.unifiedstream.ui.theme.Degraded
import com.laffy.unifiedstream.ui.theme.Good
import com.laffy.unifiedstream.ui.theme.Poor
import com.laffy.unifiedstream.ui.theme.Surface1
import com.laffy.unifiedstream.ui.theme.Surface2
import com.laffy.unifiedstream.ui.theme.TextMuted

/** Everything the camera card renders and calls back into. */
data class CameraControls(
    val state: CameraStreamState = CameraStreamState.Inactive,
    val facing: CameraFacing = CameraFacing.BACK,
    val resolution: CameraResolution = CameraResolution.HD,
    val permissionNeeded: Boolean = false,
    val onToggle: (Boolean) -> Unit = {},
    val onFacingToggle: () -> Unit = {},
    val onResolutionSelect: (CameraResolution) -> Unit = {},
    /** Attaches the local preview surface while the card shows one; null detaches. */
    val onPreviewSurface: (Preview.SurfaceProvider?) -> Unit = {},
)

/** Everything the microphone card renders and calls back into. */
data class MicControls(
    val state: MicStreamState = MicStreamState.Inactive,
    val level: AudioLevel = AudioLevel(),
    val muted: Boolean = false,
    val gain: Float = 1f,
    val noiseSuppression: Boolean = false,
    val noiseSuppressionAvailable: Boolean = false,
    val permissionNeeded: Boolean = false,
    val onToggle: (Boolean) -> Unit = {},
    val onMuteToggle: (Boolean) -> Unit = {},
    val onGainChange: (Float) -> Unit = {},
    val onNoiseSuppressionToggle: (Boolean) -> Unit = {},
)

/** Everything the speaker card renders and calls back into. */
data class SpeakerControls(
    val state: SpeakerStreamState = SpeakerStreamState.Inactive,
    val level: AudioLevel = AudioLevel(),
    val muted: Boolean = false,
    val volume: Float = 1f,
    val onToggle: (Boolean) -> Unit = {},
    val onMuteToggle: (Boolean) -> Unit = {},
    val onVolumeChange: (Float) -> Unit = {},
)

@Composable
private fun SectionCard(
    title: String,
    modifier: Modifier = Modifier,
    trailing: @Composable (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = Surface1),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = title.uppercase(),
                    style = MaterialTheme.typography.labelMedium,
                    color = TextMuted,
                )
                trailing?.invoke()
            }
            content()
        }
    }
}

// --- Device list --------------------------------------------------------------------------

@Composable
fun DeviceListScreen(
    devices: List<DiscoveredDevice>,
    discoveryState: DiscoveryState,
    offerManual: Boolean,
    manualError: ManualAddress.Error?,
    onSelect: (DiscoveredDevice) -> Unit,
    onAddManual: (String, String) -> Unit,
    onManualEdited: () -> Unit,
    modifier: Modifier = Modifier,
) {
    LazyColumn(
        modifier = modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Text(
                text = "Devices",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(top = 8.dp),
            )
        }

        item { DiscoveryStatusRow(discoveryState, devices.isEmpty()) }

        items(devices, key = { it.id }) { device ->
            DeviceRow(device = device, onClick = { onSelect(device) })
        }

        if (offerManual) {
            item {
                ManualEntryCard(
                    error = manualError,
                    onSubmit = onAddManual,
                    onEdited = onManualEdited,
                )
            }
        }

        item { Spacer(Modifier.height(24.dp)) }
    }
}

@Composable
private fun DiscoveryStatusRow(state: DiscoveryState, empty: Boolean) {
    val message = when (state) {
        DiscoveryState.Idle -> "Not searching"
        DiscoveryState.NoNetwork ->
            "Not connected to Wi-Fi. Join the same network as your PC."
        DiscoveryState.Searching -> "Searching for PCs on this network…"
        DiscoveryState.Found -> if (empty) "Searching…" else "Tap a device to connect"
        DiscoveryState.TimedOut ->
            "No PCs found. Some networks block discovery — enter the address by hand."
        is DiscoveryState.Failed -> state.message
    }

    Row(verticalAlignment = Alignment.CenterVertically) {
        if (state is DiscoveryState.Searching) {
            CircularProgressIndicator(
                modifier = Modifier.size(14.dp),
                strokeWidth = 2.dp,
                color = MaterialTheme.colorScheme.primary,
            )
            Spacer(Modifier.width(10.dp))
        }
        Text(
            text = message,
            style = MaterialTheme.typography.bodyMedium,
            color = if (state is DiscoveryState.Failed) Poor else TextMuted,
        )
    }
}

@Composable
private fun DeviceRow(device: DiscoveredDevice, onClick: () -> Unit) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = device.isCompatible, onClick = onClick),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = Surface1),
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    text = device.name,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = "${device.host}:${device.port}",
                    style = MaterialTheme.typography.bodySmall,
                    color = TextMuted,
                    fontFamily = FontFamily.Monospace,
                )
                if (device.caps.isNotEmpty()) {
                    Text(
                        text = device.caps.joinToString(" · "),
                        style = MaterialTheme.typography.bodySmall,
                        color = TextMuted,
                    )
                }
                if (!device.isCompatible) {
                    Text(
                        text = "Incompatible version (v${device.protocolVersion})",
                        style = MaterialTheme.typography.bodySmall,
                        color = Poor,
                    )
                }
            }
            if (device.manual) {
                Text("manual", style = MaterialTheme.typography.labelSmall, color = TextMuted)
            }
        }
    }
}

@Composable
private fun ManualEntryCard(
    error: ManualAddress.Error?,
    onSubmit: (String, String) -> Unit,
    onEdited: () -> Unit,
) {
    var host by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("47810") }

    SectionCard(title = "Connect by address") {
        OutlinedTextField(
            value = host,
            onValueChange = {
                host = it
                onEdited()
            },
            label = { Text("IP address or hostname") },
            singleLine = true,
            isError = error == ManualAddress.Error.EMPTY_HOST ||
                error == ManualAddress.Error.MALFORMED_HOST,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = port,
            onValueChange = {
                port = it
                onEdited()
            },
            label = { Text("Port") },
            singleLine = true,
            isError = error == ManualAddress.Error.MALFORMED_PORT ||
                error == ManualAddress.Error.PORT_OUT_OF_RANGE,
            modifier = Modifier.fillMaxWidth(),
        )
        error?.let {
            Text(
                text = when (it) {
                    ManualAddress.Error.EMPTY_HOST -> "Enter an address"
                    ManualAddress.Error.MALFORMED_HOST -> "That is not a valid address"
                    ManualAddress.Error.MALFORMED_PORT -> "Port must be a number"
                    ManualAddress.Error.PORT_OUT_OF_RANGE -> "Port must be between 1 and 65535"
                },
                style = MaterialTheme.typography.bodySmall,
                color = Poor,
            )
        }
        Button(onClick = { onSubmit(host, port) }, modifier = Modifier.fillMaxWidth()) {
            Text("Connect")
        }
    }
}

// --- Session ------------------------------------------------------------------------------

@Composable
fun SessionScreen(
    connection: ConnectionState,
    dashboard: DashboardState,
    testReport: TestStreamReport?,
    testRunning: Boolean,
    mic: MicControls,
    camera: CameraControls,
    speaker: SpeakerControls,
    onStartTestStream: (Int, Int) -> Unit,
    onStopTestStream: () -> Unit,
    onDisconnect: () -> Unit,
    onCancelReconnect: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // Scrollable: the cards plus the test-stream metrics overflow a phone screen, and without
    // this the disconnect control at the bottom is simply unreachable.
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        ConnectionHeader(connection)

        SectionCard(
            title = "Link quality",
            trailing = { QualityBadge(dashboard.quality) },
        ) {
            if (connection.isConnected) {
                MetricGrid(dashboard)
            } else {
                Text(
                    text = "Metrics appear once a session is connected.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = TextMuted,
                )
            }
        }

        TestStreamCard(
            connected = connection.isConnected,
            report = testReport,
            running = testRunning,
            onStart = onStartTestStream,
            onStop = onStopTestStream,
        )

        CameraCard(connected = connection.isConnected, camera = camera)

        MicrophoneCard(connected = connection.isConnected, mic = mic)

        SpeakerCard(connected = connection.isConnected, speaker = speaker)

        when {
            connection is ConnectionState.Reconnecting ->
                OutlinedButton(onClick = onCancelReconnect, modifier = Modifier.fillMaxWidth()) {
                    Text("Cancel")
                }

            connection.isConnected ->
                OutlinedButton(onClick = onDisconnect, modifier = Modifier.fillMaxWidth()) {
                    Text("Disconnect")
                }

            else ->
                TextButton(onClick = onBack, modifier = Modifier.fillMaxWidth()) {
                    Text("Back to devices")
                }
        }

        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun TestStreamCard(
    connected: Boolean,
    report: TestStreamReport?,
    running: Boolean,
    onStart: (Int, Int) -> Unit,
    onStop: () -> Unit,
) {
    var rate by remember { mutableStateOf("60") }
    var frame by remember { mutableStateOf("4096") }
    val frameBytes = frame.toIntOrNull() ?: 0

    SectionCard(title = "Test stream") {
        Text(
            text = "No codecs yet — this proves the transport.",
            style = MaterialTheme.typography.bodySmall,
            color = TextMuted,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = rate,
                onValueChange = { rate = it },
                label = { Text("Rate (Hz)") },
                singleLine = true,
                enabled = !running,
                modifier = Modifier.weight(1f),
            )
            OutlinedTextField(
                value = frame,
                onValueChange = { frame = it },
                label = { Text("Frame (bytes)") },
                singleLine = true,
                enabled = !running,
                modifier = Modifier.weight(1f),
            )
        }
        if (frameBytes > MAX_PAYLOAD) {
            Text(
                text = "Above $MAX_PAYLOAD bytes, so this exercises fragmentation.",
                style = MaterialTheme.typography.bodySmall,
                color = TextMuted,
            )
        }
        Button(
            onClick = {
                if (running) {
                    onStop()
                } else {
                    onStart(rate.toIntOrNull() ?: 60, frameBytes.coerceAtLeast(8))
                }
            },
            enabled = connected,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(if (running) "Stop" else "Start")
        }
        report?.let {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Metric("Verified", it.verified.toString(), Modifier.weight(1f))
                Metric("Missing", it.missing.toString(), Modifier.weight(1f))
                Metric("Corrupt", it.corrupt.toString(), Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun CameraCard(connected: Boolean, camera: CameraControls) {
    val streaming = camera.state is CameraStreamState.Active
    val switchOn = streaming || camera.state is CameraStreamState.Starting

    SectionCard(
        title = "Camera",
        trailing = {
            Switch(
                checked = switchOn,
                onCheckedChange = camera.onToggle,
                enabled = connected,
            )
        },
    ) {
        when (val state = camera.state) {
            is CameraStreamState.Active -> {
                // Local preview so the user can frame the shot without looking at the PC.
                AndroidView(
                    factory = { context ->
                        PreviewView(context).apply {
                            implementationMode = PreviewView.ImplementationMode.COMPATIBLE
                        }
                    },
                    update = { view -> camera.onPreviewSurface(view.surfaceProvider) },
                    modifier = Modifier
                        .fillMaxWidth()
                        .aspectRatio(state.params.width.toFloat() / state.params.height)
                        .clip(RoundedCornerShape(12.dp))
                        .background(Surface2),
                )
                DisposableEffect(Unit) {
                    onDispose { camera.onPreviewSurface(null) }
                }

                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = "Streaming ${state.params.width}x${state.params.height}",
                        style = MaterialTheme.typography.titleSmall,
                        color = Good,
                    )
                    OutlinedButton(onClick = camera.onFacingToggle) {
                        Text(if (camera.facing == CameraFacing.BACK) "Front" else "Back")
                    }
                }

                ResolutionRow(camera)
            }

            is CameraStreamState.Starting -> Text(
                text = "Waiting for the PC to accept…",
                style = MaterialTheme.typography.bodyMedium,
                color = TextMuted,
            )

            else -> {
                val explanation = when {
                    camera.permissionNeeded ->
                        "Camera permission is required. Grant it to stream video."
                    !connected -> "Connect to a PC to use the phone as its webcam."
                    else -> state.display ?: "Use this phone as the PC's webcam."
                }
                Text(
                    text = explanation,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (state is CameraStreamState.Refused ||
                        state is CameraStreamState.Error || camera.permissionNeeded
                    ) {
                        Poor
                    } else {
                        TextMuted
                    },
                )
                if (connected) ResolutionRow(camera)
            }
        }
    }
}

/** One button per offered resolution; the current pick is filled. */
@Composable
private fun ResolutionRow(camera: CameraControls) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        CameraResolution.entries.forEach { resolution ->
            if (resolution == camera.resolution) {
                Button(
                    onClick = {},
                    modifier = Modifier.weight(1f),
                ) { Text(resolution.label) }
            } else {
                OutlinedButton(
                    onClick = { camera.onResolutionSelect(resolution) },
                    modifier = Modifier.weight(1f),
                ) { Text(resolution.label) }
            }
        }
    }
}

@Composable
private fun MicrophoneCard(connected: Boolean, mic: MicControls) {
    val streaming = mic.state is MicStreamState.Active
    val switchOn = streaming || mic.state is MicStreamState.Starting

    SectionCard(
        title = "Microphone",
        trailing = {
            Switch(
                checked = switchOn,
                onCheckedChange = mic.onToggle,
                enabled = connected,
            )
        },
    ) {
        when (val state = mic.state) {
            is MicStreamState.Active -> {
                MicLevelMeter(level = mic.level)

                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = if (mic.muted) "Muted" else "Live",
                        style = MaterialTheme.typography.titleSmall,
                        color = if (mic.muted) TextMuted else Good,
                    )
                    OutlinedButton(onClick = { mic.onMuteToggle(!mic.muted) }) {
                        Text(if (mic.muted) "Unmute" else "Mute")
                    }
                }

                Text(
                    text = "Gain %.1fx".format(mic.gain),
                    style = MaterialTheme.typography.bodySmall,
                    color = TextMuted,
                )
                Slider(
                    value = mic.gain,
                    onValueChange = mic.onGainChange,
                    valueRange = GAIN_MIN..GAIN_MAX,
                )

                if (mic.noiseSuppressionAvailable) {
                    Row(
                        Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = "Noise suppression",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Switch(
                            checked = mic.noiseSuppression,
                            onCheckedChange = mic.onNoiseSuppressionToggle,
                        )
                    }
                }
            }

            is MicStreamState.Starting -> Text(
                text = "Waiting for the PC to accept…",
                style = MaterialTheme.typography.bodyMedium,
                color = TextMuted,
            )

            else -> {
                val explanation = when {
                    mic.permissionNeeded ->
                        "Microphone permission is required. Grant it to stream your voice."
                    !connected -> "Connect to a PC to use the phone as its microphone."
                    else -> state.display ?: "Use this phone as the PC's microphone."
                }
                Text(
                    text = explanation,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (state is MicStreamState.Refused ||
                        state is MicStreamState.Error || mic.permissionNeeded
                    ) {
                        Poor
                    } else {
                        TextMuted
                    },
                )
            }
        }
    }
}

@Composable
private fun SpeakerCard(connected: Boolean, speaker: SpeakerControls) {
    val streaming = speaker.state is SpeakerStreamState.Active
    val switchOn = streaming || speaker.state is SpeakerStreamState.Requesting

    SectionCard(
        title = "Speaker",
        trailing = {
            Switch(
                checked = switchOn,
                onCheckedChange = speaker.onToggle,
                enabled = connected,
            )
        },
    ) {
        when (val state = speaker.state) {
            is SpeakerStreamState.Active -> {
                MicLevelMeter(level = speaker.level)

                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = if (speaker.muted) "Muted" else "Playing PC audio",
                        style = MaterialTheme.typography.titleSmall,
                        color = if (speaker.muted) TextMuted else Good,
                    )
                    OutlinedButton(onClick = { speaker.onMuteToggle(!speaker.muted) }) {
                        Text(if (speaker.muted) "Unmute" else "Mute")
                    }
                }

                Text(
                    text = "Volume ${(speaker.volume * 100).toInt()} %",
                    style = MaterialTheme.typography.bodySmall,
                    color = TextMuted,
                )
                Slider(
                    value = speaker.volume,
                    onValueChange = speaker.onVolumeChange,
                    valueRange = 0f..1f,
                )
            }

            is SpeakerStreamState.Requesting -> Text(
                text = "Waiting for the PC to start…",
                style = MaterialTheme.typography.bodyMedium,
                color = TextMuted,
            )

            else -> {
                val explanation = when {
                    !connected -> "Connect to a PC to play its audio on this phone."
                    else -> state.display ?: "Play the PC's audio through this phone."
                }
                Text(
                    text = explanation,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (state is SpeakerStreamState.Refused ||
                        state is SpeakerStreamState.Error
                    ) {
                        Poor
                    } else {
                        TextMuted
                    },
                )
            }
        }
    }
}

/** A simple horizontal level meter: RMS as the filled bar, peak as a tick. */
@Composable
private fun MicLevelMeter(level: AudioLevel) {
    Box(
        Modifier
            .fillMaxWidth()
            .height(8.dp)
            .clip(RoundedCornerShape(4.dp))
            .background(Surface2),
    ) {
        Box(
            Modifier
                .fillMaxWidth(level.rms.coerceIn(0f, 1f))
                .height(8.dp)
                .background(Good),
        )
        if (level.peak > 0.01f) {
            Row(Modifier.fillMaxWidth()) {
                Spacer(Modifier.weight(level.peak.coerceIn(0.01f, 0.99f)))
                Box(
                    Modifier
                        .width(2.dp)
                        .height(8.dp)
                        .background(Degraded),
                )
                Spacer(Modifier.weight((1f - level.peak).coerceIn(0.01f, 1f)))
            }
        }
    }
}

@Composable
private fun ConnectionHeader(connection: ConnectionState) {
    val (label, color) = when (connection) {
        is ConnectionState.Connected -> connection.peerName to Good
        is ConnectionState.Connecting -> "Connecting to ${connection.peerName}" to Degraded
        is ConnectionState.Reconnecting ->
            "Reconnecting (${connection.attempt}/${connection.maxAttempts})" to Degraded
        is ConnectionState.Failed -> connection.reason.display to Poor
        ConnectionState.Discovering -> "Searching" to TextMuted
        ConnectionState.Idle -> "Not connected" to TextMuted
    }

    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier
                    .size(10.dp)
                    .clip(CircleShape)
                    .background(color),
            )
            Spacer(Modifier.width(10.dp))
            Text(
                text = label,
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
            )
        }
        (connection as? ConnectionState.Connected)?.let { state ->
            Text(
                text = "session ${state.sessionId.toULong().toString(16)}",
                style = MaterialTheme.typography.bodySmall,
                color = TextMuted,
                fontFamily = FontFamily.Monospace,
            )
        }
    }
}

@Composable
private fun MetricGrid(dashboard: DashboardState) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Metric("Ping", dashboard.rttMs?.let { "%.1f ms".format(it) } ?: "—", Modifier.weight(1f))
            Metric("Jitter", "%.2f ms".format(dashboard.jitterMs), Modifier.weight(1f))
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Metric("Sent", "%.2f Mbps".format(dashboard.txMbps), Modifier.weight(1f))
            Metric("Received", "%.2f Mbps".format(dashboard.rxMbps), Modifier.weight(1f))
        }
        Metric("Packet loss", "%.2f %%".format(dashboard.lossPct), Modifier.fillMaxWidth())
    }
}

@Composable
private fun Metric(label: String, value: String, modifier: Modifier = Modifier) {
    Column(
        modifier
            .clip(RoundedCornerShape(12.dp))
            .background(Surface2)
            .padding(12.dp),
    ) {
        Text(label.uppercase(), style = MaterialTheme.typography.labelSmall, color = TextMuted)
        Spacer(Modifier.height(2.dp))
        Text(
            text = value,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )
    }
}

@Composable
private fun QualityBadge(quality: LinkQuality) {
    val color = when (quality) {
        LinkQuality.GOOD -> Good
        LinkQuality.DEGRADED -> Degraded
        LinkQuality.POOR -> Poor
        LinkQuality.UNKNOWN -> TextMuted
    }
    Text(
        text = quality.name.lowercase(),
        style = MaterialTheme.typography.labelMedium,
        color = color,
        fontWeight = FontWeight.SemiBold,
    )
}

@Composable
fun EmptyPlaceholder(text: String, modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text(
            text = text,
            color = TextMuted,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(32.dp),
        )
    }
}
