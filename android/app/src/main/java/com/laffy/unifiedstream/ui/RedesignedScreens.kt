package com.laffy.unifiedstream.ui

import androidx.camera.view.PreviewView
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
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
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import com.laffy.unifiedstream.audio.AudioLevel
import com.laffy.unifiedstream.discovery.DeviceIdentity
import com.laffy.unifiedstream.discovery.DiscoveredDevice
import com.laffy.unifiedstream.discovery.DiscoveryState
import com.laffy.unifiedstream.discovery.ManualAddress
import com.laffy.unifiedstream.protocol.MAX_PAYLOAD
import com.laffy.unifiedstream.session.CameraStreamState
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.session.MicStreamState
import com.laffy.unifiedstream.session.SpeakerStreamState
import com.laffy.unifiedstream.telemetry.LinkQuality
import com.laffy.unifiedstream.transport.TestStreamReport
import com.laffy.unifiedstream.ui.theme.Degraded
import com.laffy.unifiedstream.ui.theme.Good
import com.laffy.unifiedstream.ui.theme.Poor
import com.laffy.unifiedstream.ui.theme.RecordingCoral
import com.laffy.unifiedstream.ui.theme.TechnicalFont
import com.laffy.unifiedstream.video.CameraResolution
import java.util.Locale

@Composable
private fun DestinationList(
    modifier: Modifier = Modifier,
    content: androidx.compose.foundation.lazy.LazyListScope.() -> Unit,
) {
    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, top = 18.dp, end = 16.dp, bottom = 30.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        content = content,
    )
}

@Composable
private fun TonalCard(
    title: String,
    modifier: Modifier = Modifier,
    trailing: @Composable (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(26.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) {
        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
                trailing?.invoke()
            }
            content()
        }
    }
}

@Composable
private fun StateLabel(active: Boolean, activeText: String = "Live", inactiveText: String = "Off") {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
        Box(Modifier.size(8.dp).clip(CircleShape).background(if (active) Good else MaterialTheme.colorScheme.outline))
        Text(if (active) activeText else inactiveText, style = MaterialTheme.typography.labelMedium, color = if (active) Good else MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun MetricValue(label: String, value: String, modifier: Modifier = Modifier) {
    Column(
        modifier = modifier.clip(RoundedCornerShape(16.dp)).background(MaterialTheme.colorScheme.surfaceContainerHigh).padding(13.dp),
        verticalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Text(label.uppercase(), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.titleLarge, fontFamily = TechnicalFont, maxLines = 1)
    }
}

@Composable
private fun CompactMetrics(dashboard: DashboardState) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricValue("Latency", dashboard.rttMs?.let { String.format(Locale.US, "%.1f ms", it) } ?: "—", Modifier.weight(1f))
            MetricValue("Upload", String.format(Locale.US, "%.2f Mbps", dashboard.txMbps), Modifier.weight(1f))
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricValue("Loss", String.format(Locale.US, "%.2f %%", dashboard.lossPct), Modifier.weight(1f))
            MetricValue("Jitter", String.format(Locale.US, "%.2f ms", dashboard.jitterMs), Modifier.weight(1f))
        }
    }
}

@Composable
private fun LevelMeter(level: AudioLevel, label: String) {
    val percent = (level.rms.coerceIn(0f, 1f) * 100).toInt()
    val animatedLevel by animateFloatAsState(level.rms.coerceIn(0f, 1f), label = "$label animation")
    Box(
        Modifier.fillMaxWidth().height(12.dp).clip(RoundedCornerShape(6.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHighest)
            .semantics { contentDescription = "$label $percent percent" },
    ) {
        Box(Modifier.fillMaxWidth(animatedLevel).height(12.dp).background(MaterialTheme.colorScheme.primary))
    }
}

private fun connectionTitle(state: ConnectionState): String = when (state) {
    ConnectionState.Idle -> "Not connected"
    ConnectionState.Discovering -> "Looking for a PC"
    is ConnectionState.Connecting -> "Connecting to ${state.peerName}"
    is ConnectionState.Connected -> state.peerName
    is ConnectionState.Reconnecting -> "Reconnecting to ${state.peerName}"
    is ConnectionState.Failed -> "Connection failed"
}

private fun connectionDetail(state: ConnectionState): String = when (state) {
    ConnectionState.Idle -> "Choose a computer from Devices to start."
    ConnectionState.Discovering -> "Searching this trusted local network."
    is ConnectionState.Connecting -> "Pairing and negotiating available streams…"
    is ConnectionState.Connected -> "Connected · ${state.negotiatedCaps.joinToString(" · ").ifEmpty { "session active" }}"
    is ConnectionState.Reconnecting -> "Attempt ${state.attempt} of ${state.maxAttempts}"
    is ConnectionState.Failed -> state.reason.display
}

@Composable
fun HomeDestination(
    connection: ConnectionState,
    dashboard: DashboardState,
    mic: MicControls,
    camera: CameraControls,
    speaker: SpeakerControls,
    onOpenDevices: () -> Unit,
    onOpenCamera: () -> Unit,
    modifier: Modifier = Modifier,
) {
    DestinationList(modifier) {
        item {
            Card(shape = RoundedCornerShape(26.dp), colors = CardDefaults.cardColors(containerColor = if (connection.isConnected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainer)) {
                Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    StateLabel(connection.isConnected, "Connected", "Offline")
                    Text(connectionTitle(connection), style = MaterialTheme.typography.headlineSmall)
                    Text(connectionDetail(connection), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (!connection.isConnected && connection !is ConnectionState.Connecting && connection !is ConnectionState.Reconnecting) {
                        Button(onClick = onOpenDevices, modifier = Modifier.fillMaxWidth()) { Text("Find a computer") }
                    }
                }
            }
        }
        item { TonalCard("Network", trailing = { QualityPill(dashboard.quality) }) { CompactMetrics(dashboard) } }
        item {
            TonalCard("Camera", trailing = { Switch(checked = camera.state is CameraStreamState.Active || camera.state is CameraStreamState.Starting, enabled = connection.isConnected, onCheckedChange = camera.onToggle, modifier = Modifier.semantics { contentDescription = "Camera stream" }) }) {
                Text(camera.state.display ?: if (camera.state is CameraStreamState.Active) "Camera is streaming to the PC." else "Use this phone as the PC camera.", color = if (camera.state is CameraStreamState.Refused || camera.state is CameraStreamState.Error || camera.permissionNeeded) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
                TextButton(onClick = onOpenCamera) { Text("Open camera controls") }
            }
        }
        item { MicrophoneControls(connection.isConnected, mic) }
        item { SpeakerControlsCard(connection.isConnected, speaker) }
    }
}

@Composable
fun CameraDestination(
    connection: ConnectionState,
    camera: CameraControls,
    modifier: Modifier = Modifier,
) {
    val active = camera.state is CameraStreamState.Active
    DestinationList(modifier) {
        item {
            Card(shape = RoundedCornerShape(26.dp), colors = CardDefaults.cardColors(containerColor = Color(0xFF0A0D0D))) {
                Column {
                    Box(Modifier.fillMaxWidth().aspectRatio(camera.resolution.width.toFloat() / camera.resolution.height).background(Color(0xFF0A0D0D))) {
                        if (active) {
                            val state = camera.state as CameraStreamState.Active
                            AndroidView(
                                factory = { context -> PreviewView(context).apply { implementationMode = PreviewView.ImplementationMode.COMPATIBLE } },
                                update = { camera.onPreviewSurface(it.surfaceProvider) },
                                modifier = Modifier.fillMaxSize().semantics { contentDescription = "Live camera preview" },
                            )
                            DisposableEffect(Unit) { onDispose { camera.onPreviewSurface(null) } }
                            Row(Modifier.align(Alignment.TopStart).padding(14.dp).clip(RoundedCornerShape(999.dp)).background(Color.Black.copy(alpha = .62f)).padding(horizontal = 10.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                Box(Modifier.size(8.dp).clip(CircleShape).background(RecordingCoral))
                                Text("REC", color = Color.White, style = MaterialTheme.typography.labelMedium)
                            }
                            Text("${state.params.width} × ${state.params.height}", color = Color.White, style = MaterialTheme.typography.labelMedium, fontFamily = TechnicalFont, modifier = Modifier.align(Alignment.TopEnd).padding(14.dp).clip(RoundedCornerShape(999.dp)).background(Color.Black.copy(alpha = .62f)).padding(horizontal = 10.dp, vertical = 6.dp))
                        } else {
                            Column(Modifier.align(Alignment.Center), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                Box(Modifier.size(54.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary.copy(alpha = .16f)))
                                Text("Preview appears while streaming", color = Color.White.copy(alpha = .7f), style = MaterialTheme.typography.bodyMedium)
                            }
                        }
                    }
                    Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                        val explanation = camera.state.display ?: when {
                            camera.permissionNeeded -> "Camera permission is required."
                            !connection.isConnected -> "Connect to a PC before starting the camera."
                            active -> "The live preview matches the transmitted source."
                            camera.state is CameraStreamState.Starting -> "Waiting for the PC to accept the stream…"
                            else -> "Ready to stream MJPEG video."
                        }
                        Text(explanation, color = if (camera.permissionNeeded || camera.state is CameraStreamState.Refused || camera.state is CameraStreamState.Error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
                        Text("Resolution", style = MaterialTheme.typography.labelLarge)
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                            CameraResolution.entries.forEach { resolution ->
                                FilterChip(selected = camera.resolution == resolution, onClick = { camera.onResolutionSelect(resolution) }, label = { Text(resolution.label) }, modifier = Modifier.weight(1f))
                            }
                        }
                        OutlinedButton(onClick = camera.onFacingToggle, enabled = connection.isConnected, modifier = Modifier.fillMaxWidth()) { Text(if (camera.facing.name == "BACK") "Use front camera" else "Use back camera") }
                        Button(onClick = { camera.onToggle(!active) }, enabled = connection.isConnected, modifier = Modifier.fillMaxWidth()) { Text(if (active) "Stop camera" else "Start camera") }
                    }
                }
            }
        }
    }
}

@Composable
private fun MicrophoneControls(connected: Boolean, mic: MicControls) {
    val active = mic.state is MicStreamState.Active
    TonalCard("Microphone", trailing = { Switch(checked = active || mic.state is MicStreamState.Starting, enabled = connected, onCheckedChange = mic.onToggle, modifier = Modifier.semantics { contentDescription = "Microphone stream" }) }) {
        if (active) {
            LevelMeter(mic.level, "Microphone level")
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Text(if (mic.muted) "Muted" else "Streaming · ${(mic.state as MicStreamState.Active).params.sampleRate / 1000} kHz", color = if (mic.muted) MaterialTheme.colorScheme.onSurfaceVariant else Good)
                OutlinedButton(onClick = { mic.onMuteToggle(!mic.muted) }) { Text(if (mic.muted) "Unmute" else "Mute") }
            }
            Text("Gain ${String.format(Locale.US, "%.1f×", mic.gain)}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Slider(value = mic.gain, onValueChange = mic.onGainChange, valueRange = .5f..4f, modifier = Modifier.semantics { contentDescription = "Microphone gain" })
            if (mic.noiseSuppressionAvailable) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                    Text("Noise suppression")
                    Switch(checked = mic.noiseSuppression, onCheckedChange = mic.onNoiseSuppressionToggle)
                }
            }
        } else {
            Text(mic.state.display ?: if (mic.permissionNeeded) "Microphone permission is required." else if (!connected) "Connect to a PC to use the microphone." else "Use this phone as the PC microphone.", color = if (mic.permissionNeeded || mic.state is MicStreamState.Refused || mic.state is MicStreamState.Error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun SpeakerControlsCard(connected: Boolean, speaker: SpeakerControls) {
    val active = speaker.state is SpeakerStreamState.Active
    TonalCard("Speaker", trailing = { Switch(checked = active || speaker.state is SpeakerStreamState.Requesting, enabled = connected, onCheckedChange = speaker.onToggle, modifier = Modifier.semantics { contentDescription = "Speaker stream" }) }) {
        if (active) {
            LevelMeter(speaker.level, "Speaker level")
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Text(if (speaker.muted) "Muted" else "Playing PC audio", color = if (speaker.muted) MaterialTheme.colorScheme.onSurfaceVariant else Good)
                OutlinedButton(onClick = { speaker.onMuteToggle(!speaker.muted) }) { Text(if (speaker.muted) "Unmute" else "Mute") }
            }
            Text("Volume ${(speaker.volume * 100).toInt()}%", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Slider(value = speaker.volume, onValueChange = speaker.onVolumeChange, valueRange = 0f..1f, modifier = Modifier.semantics { contentDescription = "Speaker volume" })
        } else {
            Text(speaker.state.display ?: if (!connected) "Connect to a PC to play its audio here." else "Play PC audio through this phone.", color = if (speaker.state is SpeakerStreamState.Refused || speaker.state is SpeakerStreamState.Error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun QualityPill(quality: LinkQuality) {
    val color = when (quality) { LinkQuality.GOOD -> Good; LinkQuality.DEGRADED -> Degraded; LinkQuality.POOR -> Poor; LinkQuality.UNKNOWN -> MaterialTheme.colorScheme.onSurfaceVariant }
    Text(quality.name.lowercase().replaceFirstChar { it.uppercase() }, color = color, style = MaterialTheme.typography.labelMedium, modifier = Modifier.clip(RoundedCornerShape(999.dp)).background(color.copy(alpha = .12f)).padding(horizontal = 10.dp, vertical = 6.dp))
}

@Composable
fun DevicesDestination(
    devices: List<DiscoveredDevice>,
    discoveryState: DiscoveryState,
    offerManual: Boolean,
    manualError: ManualAddress.Error?,
    connection: ConnectionState,
    onSelect: (DiscoveredDevice) -> Unit,
    onAddManual: (String, String) -> Unit,
    onManualEdited: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var query by remember { mutableStateOf("") }
    var host by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("47810") }
    val filtered = devices.filter { it.name.contains(query, ignoreCase = true) || it.host.contains(query, ignoreCase = true) }
    DestinationList(modifier) {
        item { OutlinedTextField(value = query, onValueChange = { query = it }, label = { Text("Search devices") }, singleLine = true, modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Search nearby computers" }) }
        item {
            when (discoveryState) {
                DiscoveryState.Searching -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) { CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp); Text("Searching the local network…", color = MaterialTheme.colorScheme.onSurfaceVariant) }
                DiscoveryState.NoNetwork -> Notice("No Wi-Fi or local network is available. Connect to the same trusted LAN as your PC.", true)
                DiscoveryState.TimedOut -> Notice("No computer answered discovery. You can connect by address below.")
                is DiscoveryState.Failed -> Notice(discoveryState.message, true)
                DiscoveryState.Idle -> Text("Discovery is paused.", color = MaterialTheme.colorScheme.onSurfaceVariant)
                DiscoveryState.Found -> Text("${devices.size} computer${if (devices.size == 1) "" else "s"} found", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        if (filtered.isEmpty() && discoveryState == DiscoveryState.Found) item { Notice("No devices match your search.") }
        items(filtered, key = { it.id }) { device ->
            Card(
                modifier = Modifier.fillMaxWidth().clickable(enabled = device.isCompatible && !connection.isBusy) { onSelect(device) }.semantics { role = Role.Button; contentDescription = "Connect to ${device.name}" },
                shape = RoundedCornerShape(22.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
            ) {
                Row(Modifier.padding(17.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(13.dp)) {
                    Box(Modifier.size(44.dp).clip(RoundedCornerShape(14.dp)).background(MaterialTheme.colorScheme.primaryContainer), contentAlignment = Alignment.Center) { Text("PC", color = MaterialTheme.colorScheme.onPrimaryContainer, fontWeight = FontWeight.Bold) }
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                        Text(device.name, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text("${device.host}:${device.port}", style = MaterialTheme.typography.bodySmall, fontFamily = TechnicalFont, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text(if (device.isCompatible) device.caps.joinToString(" · ").ifEmpty { "Compatible" } else "Incompatible protocol v${device.protocolVersion}", style = MaterialTheme.typography.bodySmall, color = if (device.isCompatible) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.error)
                    }
                    if (connection.isBusy) CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp) else Text("›", style = MaterialTheme.typography.headlineSmall)
                }
            }
        }
        if (offerManual || discoveryState is DiscoveryState.NoNetwork || discoveryState is DiscoveryState.Failed) {
            item {
                TonalCard("Connect by address") {
                    OutlinedTextField(value = host, onValueChange = { host = it; onManualEdited() }, label = { Text("IP address or hostname") }, singleLine = true, isError = manualError == ManualAddress.Error.EMPTY_HOST || manualError == ManualAddress.Error.MALFORMED_HOST, modifier = Modifier.fillMaxWidth())
                    OutlinedTextField(value = port, onValueChange = { port = it; onManualEdited() }, label = { Text("Control port") }, singleLine = true, isError = manualError == ManualAddress.Error.MALFORMED_PORT || manualError == ManualAddress.Error.PORT_OUT_OF_RANGE, modifier = Modifier.fillMaxWidth())
                    manualError?.let { Text(manualErrorText(it), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
                    Button(onClick = { onAddManual(host, port) }, enabled = !connection.isBusy, modifier = Modifier.fillMaxWidth()) { Text("Connect") }
                }
            }
        }
    }
}

private fun manualErrorText(error: ManualAddress.Error): String = when (error) {
    ManualAddress.Error.EMPTY_HOST -> "Enter an address"
    ManualAddress.Error.MALFORMED_HOST -> "That is not a valid address"
    ManualAddress.Error.MALFORMED_PORT -> "Port must be a number"
    ManualAddress.Error.PORT_OUT_OF_RANGE -> "Port must be between 1 and 65535"
}

@Composable
private fun Notice(text: String, error: Boolean = false) {
    Text(text, color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(if (error) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.surfaceContainerHigh).padding(14.dp))
}

@Composable
fun SettingsDestination(
    identity: DeviceIdentity?,
    connection: ConnectionState,
    darkTheme: Boolean,
    testReport: TestStreamReport?,
    testRunning: Boolean,
    onThemeChange: (Boolean) -> Unit,
    onStartTestStream: (Int, Int) -> Unit,
    onStopTestStream: () -> Unit,
    onDisconnect: () -> Unit,
    onCancelReconnect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var rate by remember { mutableStateOf("60") }
    var frame by remember { mutableStateOf("4096") }
    val frameBytes = frame.toIntOrNull() ?: 0
    DestinationList(modifier) {
        item {
            TonalCard("Appearance") {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                    Column(Modifier.weight(1f)) { Text("Dark theme", style = MaterialTheme.typography.titleSmall); Text("Uses the deep forest palette.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    Switch(checked = darkTheme, onCheckedChange = onThemeChange, modifier = Modifier.semantics { contentDescription = "Dark theme"; stateDescription = if (darkTheme) "On" else "Off" })
                }
            }
        }
        item {
            TonalCard("This phone") {
                DetailRow("Name", identity?.name ?: "Loading…")
                DetailRow("Device ID", identity?.id ?: "—", true)
                DetailRow("Session", connectionTitle(connection))
                if (connection is ConnectionState.Connected) DetailRow("Session ID", connection.sessionId.toULong().toString(), true)
                when {
                    connection is ConnectionState.Reconnecting -> OutlinedButton(onClick = onCancelReconnect, modifier = Modifier.fillMaxWidth()) { Text("Cancel reconnect") }
                    connection.isConnected -> OutlinedButton(onClick = onDisconnect, modifier = Modifier.fillMaxWidth()) { Text("Disconnect") }
                }
            }
        }
        item {
            TonalCard("Transport diagnostics", trailing = { StateLabel(testRunning, "Running", "Stopped") }) {
                Text("A synthetic stream verifies fragmentation and integrity without changing media streams.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(value = rate, onValueChange = { rate = it }, label = { Text("Rate Hz") }, enabled = !testRunning, singleLine = true, modifier = Modifier.weight(1f))
                    OutlinedTextField(value = frame, onValueChange = { frame = it }, label = { Text("Frame bytes") }, enabled = !testRunning, singleLine = true, modifier = Modifier.weight(1f))
                }
                if (frameBytes > MAX_PAYLOAD) Text("This size exercises fragmentation.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Button(onClick = { if (testRunning) onStopTestStream() else onStartTestStream(rate.toIntOrNull() ?: 60, frameBytes.coerceAtLeast(8)) }, enabled = connection.isConnected, modifier = Modifier.fillMaxWidth()) { Text(if (testRunning) "Stop test" else "Start test") }
                testReport?.let { Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) { MetricValue("Verified", it.verified.toString(), Modifier.weight(1f)); MetricValue("Missing", it.missing.toString(), Modifier.weight(1f)); MetricValue("Corrupt", it.corrupt.toString(), Modifier.weight(1f)) } }
            }
        }
        item { Text("UnifiedStream 0.1.0 · Trusted LAN MVP", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp)) }
    }
}

@Composable
private fun DetailRow(label: String, value: String, technical: Boolean = false) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top) {
        Text(label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.width(80.dp))
        Text(value, style = MaterialTheme.typography.bodyMedium, fontFamily = if (technical) TechnicalFont else null, modifier = Modifier.weight(1f), overflow = TextOverflow.Ellipsis)
    }
}
